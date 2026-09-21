use super::*;
use alerts::Operation;
use quickgui::{Application, WindowOptions};

#[test]
fn clone_without_a_source_shows_one_paint_error_and_preserves_pixels() {
    let mut editor = Editor::with_test_document();
    let mut original = Document::new(8, 8).unwrap();
    compositor::edits::fill(&mut original, [80, 120, 160, 255], false, false).unwrap();
    editor.tabs = vec![Session::new(original.clone(), None).into()];
    editor.tools.tool = Tool::Clone;
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Clone error").size(1500., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    let canvas = cx.element_bounds(window, "canvas").unwrap();
    let start = quickgui::Point::new(canvas.x + canvas.width / 2., canvas.y + canvas.height / 2.);
    cx.simulate_pointer_drag(
        window,
        "canvas",
        start,
        quickgui::Point::new(start.x + 10., start.y),
    )
    .unwrap();
    cx.read(view, |e| {
        assert_eq!(e.errors.len(), 1);
        assert_eq!(e.errors.front().unwrap().operation, Operation::Paint);
        assert!(e.errors.front().unwrap().message.contains("source"));
        assert_eq!(e.session().document, original);
        assert!(e.gesture.is_none());
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
    cx.click(window, "error-ok").unwrap();
    cx.read(view, |e| {
        assert!(e.errors.is_empty());
        assert_eq!(e.tools.tool, Tool::Clone);
        assert_eq!(e.session().document, original);
    })
    .unwrap();
}

#[test]
fn failed_save_alert_keeps_hue_draft_and_consumes_only_its_own_dismissal() {
    for acknowledgement in ["enter", "escape", "click"] {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(8, 8).unwrap();
        compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
        editor.tabs =
            vec![Session::new(doc.clone(), Some(directory.path().join("Preview.comp"))).into()];
        editor.open_pixel_adjustment(Kind::HueSaturation).unwrap();
        if let Some(Form::Edit { fields, .. }) = &mut editor.modal {
            fields[0].1 = "90".into();
        }
        editor.preview_adjustment().unwrap();
        let preview = editor.session().document.clone();
        assert_ne!(preview, doc);
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Error over Hue").size(1500., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        cx.update(view, |e, cx| e.action(Action::Save, cx)).unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("error-ok".into()));
        cx.read(view, |e| {
            let failure = e.errors.front().unwrap();
            assert_eq!(failure.operation, Operation::Save);
            assert!(
                failure
                    .message
                    .starts_with("Could not start the file operation:")
            );
            assert!(!e.can_start_project_operation());
            assert!(!e.can_switch_projects());
            assert!(!e.action_available(Action::InvertPixels));
        })
        .unwrap();
        cx.simulate_keystrokes(window, "ctrl-z ctrl-n ctrl-q alt-i b")
            .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 1);
            assert_eq!(e.errors.len(), 1);
            assert_eq!(e.session().document, preview);
        })
        .unwrap();
        if acknowledgement == "click" {
            cx.click(window, "error-ok").unwrap();
        } else {
            cx.simulate_keystrokes(window, acknowledgement).unwrap();
        }
        cx.read(view, |e| {
            assert!(e.errors.is_empty());
            assert!(e.adjustment_edit.is_some());
            assert_eq!(e.session().document, preview);
            assert_eq!(e.session().committed_document(), &doc);
            assert!(matches!(&e.modal, Some(Form::Edit { fields, .. }) if fields[0].1 == "90"));
        })
        .unwrap();
        cx.simulate_keystrokes(window, "escape").unwrap();
        cx.read(view, |e| {
            assert!(e.modal.is_none());
            assert_eq!(e.session().document, doc);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
    }
}

#[test]
fn crop_failure_keeps_the_document_and_editable_crop() {
    let mut editor = Editor::with_test_document();
    let original = editor.session().document.clone();
    editor.tools.tool = Tool::Crop;
    let mut frame = compositor::geometry::Transform::new(1, 1);
    frame.size = [0., 0.];
    editor.tools.pending_crop = Some(crop::CropPreview {
        frame,
        guides: [None; 2],
    });
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Crop failure").size(1500., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "crop-apply").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.errors.front().unwrap().operation, Operation::Crop);
        assert_eq!(e.session().document, original);
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
    cx.simulate_keystrokes(window, "escape").unwrap();
    cx.read(view, |e| {
        assert!(e.errors.is_empty());
        assert!(e.tools.pending_crop.is_some());
        assert_eq!(e.session().document, original);
    })
    .unwrap();
}

#[test]
fn queued_errors_are_acknowledged_individually_on_an_empty_workspace() {
    let mut editor = Editor::new(Vec::new()).unwrap();
    editor.show_error(Operation::Open, "The first project is missing.");
    editor.show_error(Operation::Import, "The second image is invalid.");
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Queued errors").size(1280., 850.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    cx.simulate_keystrokes(window, "escape").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.errors.len(), 1);
        assert_eq!(e.errors.front().unwrap().operation, Operation::Import);
        assert!(!e.has_document());
    })
    .unwrap();
    cx.click(window, "error-ok").unwrap();
    cx.read(view, |e| {
        assert!(e.errors.is_empty());
        assert!(!e.has_document());
        assert!(e.can_switch_projects());
    })
    .unwrap();
}
