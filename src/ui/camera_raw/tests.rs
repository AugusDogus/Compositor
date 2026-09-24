use super::*;
#[test]
fn switching_camera_groups_preserves_drafts_and_cancel_preserves_document() {
    let mut editor = Editor::with_test_document();
    compositor::edits::fill(
        &mut editor.session_mut().document,
        [80, 100, 120, 255],
        false,
        false,
    )
    .unwrap();
    let before = editor.session().document.clone();
    editor.open_camera_raw().unwrap();
    editor.update_form_field(0, "1.5");
    editor.camera_change(|edit| edit.group = Group::Color);
    assert_eq!(editor.camera_raw.settings.light.exposure, 1.5);
    editor.update_form_field(0, "25");
    editor.camera_change(|edit| edit.group = Group::Light);
    assert_eq!(editor.camera_raw.settings.color.temperature, 25.);
    assert_eq!(editor.camera_raw.fields()[0].1, "1.5");
    editor.cancel_filter();
    assert_eq!(editor.session().document, before);
    assert!(editor.session().undo_label().is_none());
}

#[test]
fn all_camera_groups_render_and_remain_switchable() {
    use quickgui::{Application, WindowOptions};
    let mut editor = Editor::with_test_document();
    compositor::edits::fill(
        &mut editor.session_mut().document,
        [80, 100, 120, 255],
        false,
        false,
    )
    .unwrap();
    editor.open_camera_raw().unwrap();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(
            WindowOptions::new("Camera Raw controls").size(1280., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    for group in Group::ALL {
        cx.click(window, format!("camera-group-{}", group as usize))
            .unwrap();
        cx.read(view, |e| assert_eq!(e.camera_raw.group, group))
            .unwrap();
        let control = cx.element_bounds(window, "camera-param-0").unwrap();
        assert!(control.width > 40. && control.height > 0.);
    }
    cx.click(window, "form-cancel").unwrap();
    cx.read(view, |e| assert!(e.filter_edit.is_none())).unwrap();
}
