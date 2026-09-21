use super::*;
use quickgui::{Application, WindowOptions};

#[test]
fn point_text_edit_cancel_apply_and_undo_are_transactional() {
    let mut editor = Editor::with_test_document();
    let original = editor.session().document.clone();
    editor.begin_text([20., 30.], [20., 30.], false).unwrap();
    assert!(matches!(&editor.modal,Some(Form::Text(draft)) if draft.style.box_size.is_none()));
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Type").size(1400., 1000.), editor)
        .unwrap();
    let window = view.window_handle();
    for id in [
        "text-content",
        "text-number-0",
        "text-number-1",
        "text-number-2",
        "text-font",
        "text-color",
        "text-color-picker",
        "text-box",
        "text-preview",
    ] {
        assert!(
            cx.element_bounds(window, id).is_ok(),
            "Missing text control {id}"
        );
    }

    cx.update(view, |editor, cx| {
        if let Some(Form::Text(draft)) = &mut editor.modal {
            draft.style.content = "Cancel me".into();
        }
        editor.cancel_form(cx);
        assert!(editor.session().document == original);
        editor.begin_text([20., 30.], [20., 30.], false).unwrap();
        if let Some(Form::Text(draft)) = &mut editor.modal {
            draft.style.content = "Editable".into();
        }
        editor.apply_text().unwrap();
        assert_eq!(
            editor
                .session()
                .document
                .active_layer()
                .unwrap()
                .text
                .as_ref()
                .unwrap()
                .content,
            "Editable"
        );
        let committed = editor.session().document.clone();
        editor.edit_active_text().unwrap();
        editor.apply_text().unwrap();
        assert!(editor.session().document == committed);
        editor.session_mut().undo();
        assert!(editor.session().document == original);
        editor.session_mut().redo();
        assert!(editor.session().document == committed);
    })
    .unwrap();
}

#[test]
fn paragraph_draft_preserves_reverse_drag_bounds_and_invalid_edits() {
    let mut editor = Editor::with_test_document();
    editor.session_mut().zoom = 1.;
    editor.begin_text([320., 180.], [20., 30.], false).unwrap();
    if let Some(Form::Text(draft)) = &mut editor.modal {
        assert_eq!(draft.origin, [20., 30.]);
        assert_eq!(draft.style.box_size, Some([300., 150.]));
        draft.style.content = "A paragraph".into();
        draft.numbers[0] = "NaN".into();
    } else {
        panic!("Type draft missing");
    }
    let before = editor.session().document.clone();
    assert!(editor.apply_text().is_err());
    assert!(editor.modal.is_some());
    assert!(editor.session().document == before);
    if let Some(Form::Text(draft)) = &mut editor.modal {
        draft.numbers[0] = "48".into();
    }
    editor.apply_text().unwrap();
    assert_eq!(
        editor
            .session()
            .document
            .active_layer()
            .unwrap()
            .text
            .as_ref()
            .unwrap()
            .box_size,
        Some([300., 150.])
    );
}

#[test]
fn text_color_picker_returns_to_draft_and_preserves_precise_imported_colors_on_noop() {
    let mut editor = Editor::with_test_document();
    editor.begin_text([0., 0.], [0., 0.], true).unwrap();
    if let Some(Form::Text(draft)) = &mut editor.modal {
        draft.style.red = 0.123456789;
        draft.color = "#1F0000".into();
        assert_eq!(draft.parsed().unwrap().red, 0.123456789);
    }
    let original = editor.session().document.clone();
    editor.open_text_color_picker();
    assert!(matches!(editor.modal, Some(Form::Color(_))));
    editor.finish_color(false).unwrap();
    assert!(matches!(editor.modal, Some(Form::Text(_))));
    assert!(editor.session().document == original);
}
