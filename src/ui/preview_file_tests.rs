use super::*;
use quickgui::{Application, WindowOptions};

fn hue_editor(path: Option<PathBuf>) -> Editor {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(8, 8).unwrap();
    compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
    e.tabs = vec![Session::new(doc, path).into()];
    e.open_pixel_adjustment(Kind::HueSaturation).unwrap();
    if let Some(Form::Edit { fields, .. }) = &mut e.modal {
        fields[0].1 = "90".into();
    }
    e.preview_adjustment().unwrap();
    e
}

#[test]
fn png_export_keeps_hue_and_writes_committed_pixels() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("original.png");
    let mut e = hue_editor(None);
    let baseline = e.session().committed_document().clone();
    let preview = e.session().document.clone();
    e.export_png_to(path.clone()).unwrap();
    e.file_job.take().unwrap().run(&[]).unwrap();
    assert_eq!(
        image::open(path).unwrap().into_rgba8(),
        **baseline.layers[0].raster().unwrap()
    );
    assert_eq!(e.session().document, preview);
    assert!(e.adjustment_edit.is_some());
    assert!(matches!(
        e.modal,
        Some(Form::Edit {
            action: Action::EditAdjustment,
            ..
        })
    ));
}

#[test]
fn save_shortcut_keeps_hue_and_saves_committed_pixels() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Preview.comp");
    let e = hue_editor(Some(path.clone()));
    let baseline = e.session().committed_document().clone();
    let preview = e.session().document.clone();
    assert_ne!(preview, baseline);
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Save beneath Hue").size(1500., 900.), e)
        .unwrap();
    cx.update(view, |e, cx| {
        e.form_key(&Key::Character("s".into()), Modifiers::CONTROL, cx);
        assert!(e.adjustment_edit.is_some());
        assert!(e.modal.is_some());
        let job = e.file_job.take().expect("Ctrl+S must queue Save");
        let completed = job.run(&[]).unwrap();
        e.pending = false;
        e.finish_file_job(completed, cx).unwrap();
        assert_eq!(e.session().document, preview);
        assert!(!e.session().dirty());
        e.cancel_adjustment();
        assert_eq!(e.session().document, baseline);
        assert!(!e.session().dirty());
    })
    .unwrap();
    assert_eq!(project::load(&path).unwrap(), baseline);
}

#[test]
fn import_under_hue_preserves_preview_and_has_independent_history() {
    let e = hue_editor(None);
    let baseline = e.session().committed_document().clone();
    let preview = e.session().document.clone();
    let mut imported = compositor::document::Layer::blank("Imported", 2, 2);
    imported.content = compositor::document::LayerContent::Raster(Some(Arc::new(
        image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255])),
    )));
    let imported_id = imported.id;
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Import beneath Hue").size(1500., 900.),
            e,
        )
        .unwrap();
    cx.update(view, |e, cx| {
        e.finish_file_job(
            Completed::Imported {
                session: e.session().id,
                layers: vec![imported],
                center: None,
                projects: Vec::new(),
                psds: Vec::new(),
                raws: Vec::new(),
            },
            cx,
        )
        .unwrap();
        assert!(e.adjustment_edit.is_some());
        assert!(e.modal.is_some());
        assert_eq!(e.session().document.layers.len(), 2);
        assert_eq!(e.session().document.active, Some(imported_id));
        assert_eq!(e.session().document.layers[0], preview.layers[0]);
        assert_eq!(
            e.session().committed_document().layers[0],
            baseline.layers[0]
        );
        assert_eq!(e.session().undo_label(), Some("Import Images"));
        e.cancel_adjustment();
        assert_eq!(e.session().document.layers[0], baseline.layers[0]);
        assert_eq!(e.session().document.active, Some(imported_id));
        e.session_mut().undo();
        assert_eq!(e.session().document, baseline);
    })
    .unwrap();
}

#[test]
fn png_command_rejects_other_formats_without_replacing_files_or_the_preview() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("existing.jpg");
    std::fs::write(&path, b"preserved file").unwrap();
    let mut e = hue_editor(None);
    let original = e.session().document.clone();
    assert!(e.export_png_to(path.clone()).is_err());
    assert!(
        e.file_job.is_none(),
        "PNG must not queue a different format"
    );
    assert!(
        e.jpeg_export.is_none(),
        "PNG must not switch to the JPEG sheet"
    );
    assert_eq!(e.session().document, original);
    assert!(e.adjustment_edit.is_some());
    assert_eq!(std::fs::read(path).unwrap(), b"preserved file");
}
