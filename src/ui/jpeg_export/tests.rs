use super::*;
use quickgui::{Application, WindowOptions};

fn editor() -> Editor {
    let mut e = Editor::with_test_document();
    e.tabs = vec![Session::new(Document::new(16, 16).unwrap(), None).into()];
    e.open_jpeg(Some(PathBuf::from("unused.jpg")));
    // Hold the worker so readiness can be exercised deterministically.
    e.jpeg_export.as_mut().unwrap().running = true;
    e
}

#[test]
fn jpeg_sheet_over_hue_exports_committed_pixels_and_restores_the_panel() {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(16, 16).unwrap();
    compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
    e.tabs = vec![Session::new(doc.clone(), None).into()];
    e.open_pixel_adjustment(Kind::HueSaturation).unwrap();
    e.update_form_field(0, "90");
    let preview = e.session().document.clone();
    e.open_jpeg(Some(PathBuf::from("unused.jpg")));
    assert_eq!(e.jpeg_export.as_ref().unwrap().document, doc);
    e.jpeg_export.as_mut().unwrap().running = true;
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("JPEG over Hue").size(1500., 900.), e)
        .unwrap();
    let window = view.window_handle();
    assert!(cx.element_bounds(window, "floating-panel-close").is_ok());
    cx.focus(window, "jpeg-quality").unwrap();
    cx.simulate_keystrokes(window, "home right").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.jpeg_export.as_ref().unwrap().options.quality, 1);
        assert_eq!(e.session().document, preview);
    })
    .unwrap();
    cx.update(view, |e, cx| {
        let bytes = complete(e);
        e.finish_jpeg(cx).unwrap();
        let Some(super::super::file_jobs::FileJob::ExportJpeg { bytes: exported, .. }) = e.file_job.take() else {
            panic!("JPEG must queue its encoded preview");
        };
        assert_eq!(exported, bytes);
        assert!(e.adjustment_edit.is_some());
        assert!(e.retained_panel.is_none());
        assert!(matches!(&e.modal, Some(Form::Edit { action: Action::EditAdjustment, fields, .. }) if fields[0].1 == "90"));
        assert_eq!(e.session().document, preview);
        assert_eq!(e.session().committed_document(), &doc);
    }).unwrap();
}
fn complete(e: &mut Editor) -> Vec<u8> {
    let edit = e.jpeg_export.as_mut().unwrap();
    let source = Arc::new(compositor::render::render(&edit.document, 16, 16));
    let preview = Preview::render(&source, 72., edit.options).unwrap();
    let bytes = preview.bytes.clone();
    edit.running = false;
    edit.accept(source, preview);
    if let Some(Form::Edit { error, .. }) = &mut e.modal {
        error.clear();
    }
    bytes
}

#[test]
fn stale_preview_cannot_export_and_current_preview_queues_identical_bytes() {
    let mut e = editor();
    let edit = e.jpeg_export.as_ref().unwrap();
    let source = Arc::new(image::RgbaImage::new(16, 16));
    let old = Preview::render(&source, 72., edit.options).unwrap();
    e.set_jpeg_background([30, 100, 170]);
    let edit = e.jpeg_export.as_mut().unwrap();
    edit.accept(source, old);
    assert!(edit.preview.is_none());
    assert!(edit.queued.is_some());
    let original = e.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("JPEG").size(1280., 900.), e)
        .unwrap();
    let window = view.window_handle();
    assert!(cx.click(window, "form-apply").is_err());
    cx.focus(window, "jpeg-quality").unwrap();
    cx.simulate_keystrokes(window, "enter").unwrap();
    cx.read(view, |e| {
        assert!(e.modal.is_some());
        assert!(e.file_job.is_none());
    })
    .unwrap();
    let bytes = cx
        .update(view, |e, cx| {
            let bytes = complete(e);
            e.changed(cx);
            bytes
        })
        .unwrap();
    // Capture the queued job before workspace rendering starts its worker.
    let job = cx
        .update(view, |e, cx| {
            e.finish_jpeg(cx).unwrap();
            e.file_job.take().unwrap()
        })
        .unwrap();
    let super::super::file_jobs::FileJob::ExportJpeg {
        bytes: exported,
        quality,
        ..
    } = job
    else {
        panic!("Expected encoded JPEG export")
    };
    assert_eq!(bytes, exported);
    assert!(quality <= 100);
    cx.read(view, |e| {
        assert!(e.modal.is_none());
        assert!(e.jpeg_export.is_none());
        assert_eq!(e.session().document, original);
    })
    .unwrap();
}

#[test]
fn jpeg_slider_and_background_picker_preserve_document_and_cancelled_drafts() {
    let e = editor();
    let original = e.session().document.clone();
    let palette = [e.tools.brush.color, e.tools.background];
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("JPEG colors").size(1280., 900.), e)
        .unwrap();
    let window = view.window_handle();
    for (key, quality) in [("home", 0), ("end", 100)] {
        cx.focus(window, "jpeg-quality").unwrap();
        cx.simulate_keystrokes(window, key).unwrap();
        assert_eq!(
            cx.read(view, |e| e.jpeg_export.as_ref().unwrap().options.quality)
                .unwrap(),
            quality
        );
    }
    for (button, expected) in [("form-cancel", [255; 3]), ("form-apply", [18, 171, 52])] {
        cx.click(window, "jpeg-background").unwrap();
        cx.focus(window, 51_003_u64).unwrap();
        cx.simulate_keystrokes(window, "ctrl-a").unwrap();
        cx.simulate_input(window, "#12ab34").unwrap();
        cx.click(window, button).unwrap();
        cx.read(view, |e| {
            assert!(matches!(
                e.modal,
                Some(Form::Edit {
                    action: Action::ExportJpeg,
                    ..
                })
            ));
            assert_eq!(e.jpeg_background(), Some(expected));
            assert_eq!(e.jpeg_export.as_ref().unwrap().options.quality, 100);
            assert_eq!(e.session().document, original);
            assert_eq!([e.tools.brush.color, e.tools.background], palette);
        })
        .unwrap();
    }
    cx.click(window, "form-cancel").unwrap();
    cx.read(view, |e| {
        assert!(e.jpeg_export.is_none());
        assert!(e.file_job.is_none());
    })
    .unwrap();
}

#[test]
fn jpeg_failure_replaces_progress_with_footer_error_and_retry_recovers() {
    let e = editor();
    let original = e.session().document.clone();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(WindowOptions::new("JPEG feedback").size(1500., 900.), e)
        .unwrap();
    let window = view.window_handle();
    assert!(cx.element_bounds(window, "jpeg-preview-progress").is_ok());
    cx.update(view, |e, cx| {
        let edit = e.jpeg_export.as_mut().unwrap();
        edit.running = false;
        edit.queued = None;
        let Some(Form::Edit { error, .. }) = &mut e.modal else {
            panic!("JPEG sheet must remain open");
        };
        *error = "JPEG preview failed. Change quality to retry.".into();
        e.changed(cx);
    })
    .unwrap();
    assert!(
        cx.element_bounds(window, "jpeg-preview-progress").is_err(),
        "A failed preview must stop showing progress"
    );
    let feedback = cx.element_bounds(window, "jpeg-status").unwrap();
    let cancel = cx.element_bounds(window, "form-cancel").unwrap();
    assert!(
        (feedback.y + feedback.height / 2. - cancel.y - cancel.height / 2.).abs() < 1.,
        "The error belongs in the button footer"
    );
    let labels = cx
        .accessibility_update(window)
        .unwrap()
        .nodes
        .into_iter()
        .filter_map(|(_, node)| node.label().map(str::to_owned))
        .collect::<Vec<_>>();
    assert!(
        labels
            .iter()
            .any(|label| label == "JPEG preview failed. Change quality to retry.")
    );
    assert!(
        !labels
            .iter()
            .any(|label| label.contains("Updating preview"))
    );
    assert!(cx.click(window, "form-apply").is_err());
    cx.focus(window, "jpeg-quality").unwrap();
    cx.simulate_keystrokes(window, "enter").unwrap();
    cx.read(view, |e| {
        assert!(e.modal.is_some());
        assert!(e.file_job.is_none());
        assert_eq!(e.session().document, original);
    })
    .unwrap();
    cx.update(view, |e, _| e.jpeg_export.as_mut().unwrap().running = true)
        .unwrap();
    cx.simulate_keystrokes(window, "home right").unwrap();
    assert!(cx.element_bounds(window, "jpeg-preview-progress").is_ok());
    cx.read(view, |e| {
        assert!(matches!(&e.modal, Some(Form::Edit { error, .. }) if error.is_empty()));
        assert!(!e.jpeg_ready());
    })
    .unwrap();
    let bytes = cx
        .update(view, |e, cx| {
            let bytes = complete(e);
            e.changed(cx);
            bytes
        })
        .unwrap();
    assert!(bytes.len() < 1000);
    let expected = format!("{} bytes · encoded preview, fitted to window", bytes.len());
    assert!(
        cx.accessibility_update(window)
            .unwrap()
            .nodes
            .iter()
            .any(|(_, node)| node.label() == Some(expected.as_str()))
    );
    assert!(cx.element_bounds(window, "jpeg-preview-progress").is_err());
    assert!(cx.read(view, Editor::jpeg_ready).unwrap());
    cx.click(window, "form-cancel").unwrap();
    cx.read(view, |e| {
        assert!(e.jpeg_export.is_none());
        assert!(e.file_job.is_none());
        assert_eq!(e.session().document, original);
    })
    .unwrap();
}
