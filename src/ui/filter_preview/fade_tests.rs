use super::*;
use image::{Rgba, RgbaImage};
use quickgui::{Application, WindowOptions};

fn editor() -> Editor {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(256, 160).unwrap();
    doc.layers[0].content = compositor::document::LayerContent::Raster(Some(Arc::new(
        RgbaImage::from_fn(256, 160, |x, y| Rgba([x as u8, (y + 50) as u8, 160, 180])),
    )));
    e.tabs = vec![Session::new(doc, None).into()];
    e.session_mut()
        .edit("Invert", |doc| {
            doc.layers[0].content = compositor::document::LayerContent::Raster(Some(Arc::new(
                RgbaImage::from_fn(256, 160, |x, y| {
                    Rgba([255 - x as u8, 205 - y as u8, 95, 180])
                }),
            )));
            Ok(())
        })
        .unwrap();
    e
}

// TestAppContext has no worker pool. Run the actual Fade operation on a worker
// thread, then deliver its result through the same revision gate as production.
fn preview(e: &mut Editor) {
    let edit = e.filter_edit.as_mut().unwrap();
    let Settings::Fade(source, amount) = edit.desired.clone().unwrap() else {
        panic!("Wrong preview");
    };
    let (id, revision) = (edit.id, edit.revision);
    let mut document = edit.original.clone();
    edit.work = Work::Running;
    let output = std::thread::spawn(move || {
        source.apply(&mut document, amount).unwrap();
        document.validate().unwrap();
        PreviewOutput {
            document,
            subject: None,
            camera_scope: None,
        }
    })
    .join()
    .unwrap();
    e.receive_filter_preview(id, revision, Ok(output));
}

#[test]
fn fade_previews_never_compound_and_invalid_drafts_preserve_source() {
    let mut e = editor();
    let original = e.session().document.clone();
    let source = e.session().fade().unwrap();
    e.open_fade().unwrap();
    for amount in [25., 75., 0., 50.] {
        e.update_form_field(0, &amount.to_string());
        preview(&mut e);
        let mut expected = original.clone();
        source.apply(&mut expected, amount / 100.).unwrap();
        assert_eq!(e.session().document, expected);
        assert_eq!(e.session().committed_document(), &original);
        assert!(!e.can_use_history());
        assert!(!e.action_available(Action::Undo));
        assert!(!e.action_available(Action::Redo));
        assert!(!e.action_available(Action::History));
    }
    let last_preview = e.session().document.clone();
    for invalid in ["", "NaN", "-1", "101"] {
        e.update_form_field(0, invalid);
        assert!(e.filter_edit.as_ref().unwrap().desired.is_none());
        assert!(e.apply_form(Action::Fade, vec![invalid.into()]).is_err());
        assert_eq!(e.session().document, last_preview);
        assert_eq!(e.session().committed_document(), &original);
    }
    e.update_form_field(0, "80");
    preview(&mut e);
    e.cancel_filter();
    e.finish_form();
    assert_eq!(e.session().document, original);
    assert_eq!(e.session().undo_label(), Some("Invert"));
    assert!(e.can_fade());
}

#[test]
fn fade_applies_latest_opacity_as_one_undoable_job_and_full_opacity_is_no_op() {
    for amount in ["0", "50", "100"] {
        let mut e = editor();
        let original = e.session().document.clone();
        let mut expected = original.clone();
        e.session()
            .fade()
            .unwrap()
            .apply(&mut expected, amount.parse::<f64>().unwrap() / 100.)
            .unwrap();
        e.open_fade().unwrap();
        e.update_form_field(0, "25");
        preview(&mut e);
        e.apply_form(Action::Fade, vec![amount.into()]).unwrap();
        assert!(e.pending);
        assert!(e.filter_applying());
        let job = e.job.take().unwrap();
        let completion = job.completion();
        let input = e.job_source(&job).unwrap();
        assert_eq!(
            input, original,
            "Job launch must use committed pixels, never the visible preview"
        );
        assert_ne!(input, e.session().document);
        let result = std::thread::spawn(move || job.run(input)).join().unwrap();
        e.complete_job(e.session().id, original.clone(), result, completion)
            .unwrap();
        assert_eq!(e.session().document, expected);
        assert!(e.filter_edit.is_none());
        assert!(e.modal.is_none());
        if amount == "100" {
            assert_eq!(e.session().undo_label(), Some("Invert"));
        } else {
            assert_eq!(e.session().undo_label(), Some("Fade"));
            e.session_mut().undo();
            assert_eq!(e.session().document, original);
            e.session_mut().redo();
            assert_eq!(e.session().document, expected);
        }
    }
}

#[test]
fn fade_menu_eligibility_and_panel_controls_cancel_without_changing_history() {
    let mut e = editor();
    assert!(e.action_available(Action::Fade));
    let original = e.session().document.clone();
    e.open_fade().unwrap();
    e.update_form_field(0, "50");
    preview(&mut e);
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(WindowOptions::new("Fade").size(1280., 900.), e)
        .unwrap();
    let window = view.window_handle();
    assert!(cx.element_bounds(window, "parameter-0").is_ok());
    assert!(cx.element_bounds(window, "filter-preview").is_ok());
    cx.simulate_keystrokes(window, "ctrl-z").unwrap();
    cx.read(view, |e| {
        assert!(e.editing_fade());
        assert_eq!(e.session().undo_label(), Some("Invert"));
        assert_eq!(e.session().committed_document(), &original);
    })
    .unwrap();
    cx.click(window, "filter-preview").unwrap();
    cx.read(view, |e| assert_eq!(e.session().document, original))
        .unwrap();
    cx.click(window, "filter-preview").unwrap();
    cx.read(view, |e| assert_ne!(e.session().document, original))
        .unwrap();
    cx.click(window, "form-cancel").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().document, original);
        assert!(e.filter_edit.is_none());
        assert_eq!(e.session().undo_label(), Some("Invert"));
    })
    .unwrap();
    let mut no_edit = Editor::with_test_document();
    assert!(!no_edit.action_available(Action::Fade));
    assert!(no_edit.open_fade().is_err());
    let mut e = editor();
    e.session_mut()
        .edit("Rename", |doc| {
            doc.layers[0].name = "Renamed".into();
            Ok(())
        })
        .unwrap();
    assert!(!e.action_available(Action::Fade));
}
