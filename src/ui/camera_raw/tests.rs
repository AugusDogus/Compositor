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
        cx.click(window, "camera-group").unwrap();
        cx.simulate_keystrokes(window, "home").unwrap();
        for _ in 0..group as usize {
            cx.simulate_keystrokes(window, "down").unwrap();
        }
        cx.simulate_keystrokes(window, "enter").unwrap();
        cx.read(view, |e| assert_eq!(e.camera_raw.group, group))
            .unwrap();
        let control = cx
            .element_bounds(
                window,
                if group == Group::Grading {
                    "camera-param-3"
                } else {
                    "camera-param-0"
                },
            )
            .unwrap();
        assert!(control.width > 40. && control.height > 0.);
    }
    cx.click(window, "form-cancel").unwrap();
    cx.read(view, |e| assert!(e.filter_edit.is_none())).unwrap();
}

fn canvas_pointer(editor: &mut Editor, phase: quickgui::PointerPhase, unit: [f64; 2]) {
    use quickgui::{MouseButton, Point, PointerEvent, Size, Vector};
    let (zoom, offset) = editor.viewport(800., 600.);
    let layer = editor.session().document.active_layer().unwrap();
    let doc = layer.transform.point(unit);
    let point = Point::new(
        (offset[0] + doc[0] * zoom) as f32,
        (offset[1] + doc[1] * zoom) as f32,
    );
    editor
        .camera_sample_pointer(&PointerEvent {
            phase,
            position: point,
            origin: point,
            local_position: point,
            local_origin: point,
            delta: Vector::ZERO,
            button: MouseButton::Left,
            modifiers: Modifiers::empty(),
            size: Size::new(800., 600.),
        })
        .unwrap();
}
#[test]
fn camera_canvas_pick_guide_and_cancel_target_drag_preserve_source() {
    use quickgui::PointerPhase;
    let mut editor = Editor::with_test_document();
    compositor::edits::fill(
        &mut editor.session_mut().document,
        [180, 110, 60, 255],
        false,
        false,
    )
    .unwrap();
    let source = editor.session().document.clone();
    editor.open_camera_raw().unwrap();
    editor.camera_raw.group = Group::Mixer;
    editor.camera_raw.mixer_page = MixerPage::Points;
    editor.camera_raw.tool = super::pointer::Tool::PointColor;
    editor.sync_camera_fields();
    canvas_pointer(&mut editor, PointerPhase::Down, [0.5, 0.5]);
    assert_eq!(editor.camera_raw.settings.mixer.points.len(), 1);
    assert!(editor.camera_raw.settings.mixer.points[0].saturation > 0.4);
    editor.camera_raw.group = Group::Geometry;
    editor.camera_raw.tool = super::pointer::Tool::Guide;
    editor.sync_camera_fields();
    canvas_pointer(&mut editor, PointerPhase::Down, [0.2, 0.3]);
    canvas_pointer(&mut editor, PointerPhase::Up, [0.8, 0.35]);
    assert_eq!(editor.camera_raw.settings.guides.len(), 1);
    let guide = &editor.camera_raw.settings.guides[0];
    assert!((guide.start[1] - 0.7).abs() < 0.001);
    editor.camera_raw.group = Group::Curve;
    editor.camera_raw.tool = super::pointer::Tool::Curve;
    editor.sync_camera_fields();
    let before = editor.camera_raw.settings.clone();
    canvas_pointer(&mut editor, PointerPhase::Down, [0.5, 0.5]);
    canvas_pointer(&mut editor, PointerPhase::Move, [0.5, 0.4]);
    assert_ne!(editor.camera_raw.settings.curve, before.curve);
    canvas_pointer(&mut editor, PointerPhase::Cancel, [0.5, 0.4]);
    assert_eq!(editor.camera_raw.settings, before);
    editor.cancel_filter();
    assert_eq!(editor.session().document, source);
    assert!(editor.session().undo_label().is_none());
}

#[test]
fn camera_remembers_only_successfully_applied_grade() {
    let mut editor = Editor::with_test_document();
    compositor::edits::fill(
        &mut editor.session_mut().document,
        [80, 100, 120, 255],
        false,
        false,
    )
    .unwrap();
    editor.open_camera_raw().unwrap();
    editor.update_form_field(0, "1");
    let settings = editor.camera_raw.settings.clone();
    let (source, _) = editor.filter_source().unwrap();
    let initial = *source.clone();
    let id = editor.session().id;
    editor.begin_filter_commit();
    let result = jobs::Job::CameraRaw {
        settings: Box::new(settings),
        source,
    }
    .run(initial.clone());
    editor.camera_raw.settings.light.exposure = 4.; // A late field change must not replace the submitted grade.
    editor
        .complete_job(id, initial, result, jobs::Completion::CameraRaw)
        .unwrap();
    editor.open_camera_raw().unwrap();
    assert_eq!(editor.camera_raw.settings.light.exposure, 1.);
    editor.update_form_field(0, "2");
    editor.cancel_filter();
    editor.open_camera_raw().unwrap();
    assert_eq!(editor.camera_raw.settings.light.exposure, 1.);
    editor.update_form_field(0, "3");
    let original = editor.session().committed_document().clone();
    editor.begin_filter_commit();
    assert!(
        editor
            .complete_job(
                id,
                original,
                Err(compositor::invalid("worker failed")),
                jobs::Completion::CameraRaw
            )
            .is_err()
    );
    editor.open_camera_raw().unwrap();
    assert_eq!(editor.camera_raw.settings.light.exposure, 1.);
}
