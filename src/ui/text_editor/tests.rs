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

#[test]
fn clicked_point_text_starts_at_its_baseline_and_dragged_text_keeps_its_frame() {
    let mut editor = Editor::with_test_document();
    editor.session_mut().zoom = 1.;
    let pointer = [220., 180.];
    editor.begin_text(pointer, pointer, true).unwrap();
    let Form::Text(draft) = editor.modal.as_ref().unwrap() else {
        panic!("text draft missing")
    };
    let baseline = TextRenderer::default()
        .first_baseline(&draft.style)
        .unwrap();
    assert_eq!(draft.origin, [pointer[0] - 12., pointer[1] - baseline]);
    editor.modal = None;
    editor.begin_text([20., 30.], [220., 180.], true).unwrap();
    let Form::Text(draft) = editor.modal.as_ref().unwrap() else {
        panic!("text draft missing")
    };
    assert_eq!(draft.origin, [20., 30.]);
    assert_eq!(draft.style.box_size, Some([200., 150.]));
}

#[test]
fn move_tool_text_open_uses_topmost_visible_transformed_text_and_preserves_document() {
    let mut editor = Editor::with_test_document();
    editor.begin_text([120., 100.], [120., 100.], true).unwrap();
    if let Some(Form::Text(draft)) = &mut editor.modal {
        draft.style.content = "Clickable text".into();
    }
    editor.apply_text().unwrap();
    let layer = editor.session_mut().document.active_layer_mut().unwrap();
    layer.transform.rotation = 25.;
    let point = layer.transform.point([0.5, 0.5]);
    let id = layer.id;
    editor.tools.tool = Tool::Move;
    let original = editor.session().document.clone();
    assert!(editor.edit_text_at(point).unwrap());
    assert!(matches!(&editor.modal,Some(Form::Text(draft)) if draft.layer==Some(id)));
    assert_eq!(editor.session().document, original);
    editor.modal = None;
    editor
        .session_mut()
        .document
        .active_layer_mut()
        .unwrap()
        .visible = false;
    assert!(!editor.edit_text_at(point).unwrap());
}

#[test]
fn selected_text_color_survives_picker_typing_undo_and_apply() {
    let mut editor = Editor::with_test_document();
    editor.begin_text([20., 30.], [20., 30.], false).unwrap();
    if let Some(Form::Text(draft)) = &mut editor.modal {
        draft.replace_input("AB中", Some(5..5));
    }
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Type colors").size(1400., 1000.), editor)
        .unwrap();
    let window = view.window_handle();
    cx.focus(window, "text-content").unwrap();
    cx.simulate_keystrokes(window, "ctrl-home right shift-right")
        .unwrap();
    cx.click(window, "text-color-picker").unwrap();
    cx.focus(window, 51_003_u64).unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "FF0000").unwrap();
    cx.update(view, |editor, cx| {
        editor.finish_color(true).unwrap();
        cx.invalidate();
    })
    .unwrap();
    cx.update(view, |editor, _| {
        let Some(Form::Text(draft)) = &mut editor.modal else {
            panic!("draft missing")
        };
        assert_eq!(draft.selection, 1..2);
        assert_eq!(draft.style.color_runs.as_ref().unwrap()[0].location, 1);
        draft.set_selection(2..2);
        draft.replace_input("AB!中", Some(3..3));
        assert_eq!(draft.style.color_runs.as_ref().unwrap()[0].length, 2);
        draft.undo_text(false);
        assert_eq!(draft.style.content, "AB中");
        assert_eq!(draft.style.color_runs.as_ref().unwrap()[0].length, 1);
        draft.undo_text(true);
        assert_eq!(draft.style.content, "AB!中");
        editor.apply_text().unwrap();
        let text = editor
            .session()
            .document
            .active_layer()
            .unwrap()
            .text
            .as_ref()
            .unwrap();
        assert_eq!(text.color_runs.as_ref().unwrap()[0].length, 2);
    })
    .unwrap();
}
