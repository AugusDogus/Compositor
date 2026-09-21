use super::*;
use quickgui::{Application, WindowOptions};

#[test]
fn inverse_edits_keep_the_tab_modified_and_prompt_before_closing() {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(8, 6).unwrap();
    compositor::edits::fill(&mut doc, [40, 80, 160, 255], false, false).unwrap();
    e.tabs = vec![Session::new(doc.clone(), Some("Saved.comp".into())).into()];
    let id = e.tabs[0].id;
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Saved revision").size(1500., 900.), e)
        .unwrap();
    let window = view.window_handle();
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystrokes(window, "ctrl-i ctrl-i").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().document, doc);
        assert!(e.tabs[0].dirty());
    })
    .unwrap();
    assert!(
        cx.accessibility_update(window)
            .unwrap()
            .nodes
            .iter()
            .any(|(_, node)| node.label() == Some("Unsaved changes"))
    );
    cx.simulate_keystrokes(window, "ctrl-w").unwrap();
    cx.read(view, |e| {
        assert!(matches!(e.close_intent, Some(CloseProgress::Tab { target, .. }) if target == id));
        assert!(matches!(e.modal, Some(Form::Close)));
    })
    .unwrap();
    cx.click(window, "form-cancel").unwrap();
    cx.simulate_keystrokes(window, "ctrl-z ctrl-z").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().document, doc);
        assert!(!e.tabs[0].dirty());
    })
    .unwrap();
    assert!(
        !cx.accessibility_update(window)
            .unwrap()
            .nodes
            .iter()
            .any(|(_, node)| node.label() == Some("Unsaved changes"))
    );
}
