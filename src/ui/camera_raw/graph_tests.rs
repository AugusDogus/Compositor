use super::*;
use quickgui::{Application, MouseButton, PointerEvent, PointerPhase, WindowOptions};
fn editor(group: Group) -> Editor {
    let mut e = Editor::with_test_document();
    compositor::edits::fill(
        &mut e.session_mut().document,
        [90, 120, 140, 255],
        false,
        false,
    )
    .unwrap();
    e.open_camera_raw().unwrap();
    e.camera_change(|e| e.group = group);
    e
}
fn pointer(x: f32, y: f32, phase: PointerPhase) -> PointerEvent {
    let p = quickgui::Point::new(x, y);
    PointerEvent {
        phase,
        position: p,
        origin: p,
        local_position: p,
        local_origin: p,
        delta: quickgui::Vector::ZERO,
        button: MouseButton::Left,
        modifiers: Modifiers::empty(),
        size: quickgui::Size::new(200., 200.),
    }
}
#[test]
fn camera_curve_points_dividers_and_cancel_keep_valid_settings() {
    let mut e = editor(Group::Curve);
    e.camera_curve_pointer(&pointer(50., 100., PointerPhase::Down));
    e.camera_curve_pointer(&pointer(195., 100., PointerPhase::Move));
    assert_eq!(e.camera_raw.settings.curve.shadow_split, 48.);
    e.camera_curve_pointer(&pointer(195., 100., PointerPhase::Cancel));
    assert_eq!(e.camera_raw.settings.curve.shadow_split, 25.);
    e.camera_change(|e| e.curve_page = curve::Page::Point);
    e.camera_curve_pointer(&pointer(100., 60., PointerPhase::Down));
    assert_eq!(e.camera_raw.settings.curves.channels[0].len(), 3);
    e.camera_curve_pointer(&pointer(200., 20., PointerPhase::Move));
    assert_eq!(e.camera_raw.settings.curves.channels[0][1].x, 254.);
    e.camera_raw.settings.validate().unwrap();
    e.camera_curve_pointer(&pointer(200., 20., PointerPhase::Cancel));
    assert_eq!(e.camera_raw.settings.curves.channels[0].len(), 2);
}
#[test]
fn camera_wheels_set_hue_saturation_without_changing_other_ranges() {
    let mut e = editor(Group::Grading);
    e.camera_grading_pointer(1, &pointer(100., 6., PointerPhase::Down));
    let w = &e.camera_raw.settings.grading.wheels[1];
    assert!((w.hue - 90.).abs() < 1e-8);
    assert_eq!(w.saturation, 100.);
    assert_eq!(e.camera_raw.settings.grading.wheels[0].saturation, 0.);
    e.camera_grading_pointer(1, &pointer(100., 6., PointerPhase::Cancel));
    assert_eq!(e.camera_raw.settings.grading.wheels[1].saturation, 0.);
    e.camera_grading_pointer(2, &pointer(194., 100., PointerPhase::Down));
    e.camera_grading_pointer(2, &pointer(194., 100., PointerPhase::Up));
    assert_eq!(e.camera_raw.settings.grading.wheels[2].hue, 0.);
    assert_eq!(e.camera_raw.settings.grading.wheels[2].saturation, 100.);
    e.camera_luminance_pointer(2, &pointer(196., 100., PointerPhase::Down));
    assert_eq!(e.camera_raw.settings.grading.wheels[2].luminance, 100.);
    e.camera_luminance_pointer(2, &pointer(196., 100., PointerPhase::Cancel));
    assert_eq!(e.camera_raw.settings.grading.wheels[2].luminance, 0.);
    assert_eq!(e.camera_raw.settings.grading.wheels[2].saturation, 100.);
    e.camera_raw.settings.validate().unwrap();
}
#[test]
fn camera_graphs_render_and_presets_and_wheel_pages_are_interactive() {
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(
            WindowOptions::new("Camera graphs").size(1280., 900.),
            editor(Group::Curve),
        )
        .unwrap();
    let window = view.window_handle();
    let capture = |cx: &mut quickgui::TestAppContext, name: &str| {
        let frame = cx.capture_screenshot(window).unwrap();
        if let Some(dir) = std::env::var_os("COMPOSITOR_CAMERA_GRAPH_CAPTURE") {
            frame
                .write_png(std::path::Path::new(&dir).join(name))
                .unwrap();
        }
    };
    let graph = cx.element_bounds(window, "camera-curve-graph").unwrap();
    assert!(graph.width > 200. && graph.height == 150.);
    cx.click(window, "camera-curve-page-Point").unwrap();
    cx.click(window, "camera-curve-preset-Medium contrast")
        .unwrap();
    cx.read(view, |e| {
        assert_eq!(e.camera_raw.settings.curves.channels[0].len(), 4)
    })
    .unwrap();
    capture(&mut cx, "camera-curve.png");
    cx.click(window, "camera-group").unwrap();
    cx.simulate_keystrokes(window, "home down down down down down enter")
        .unwrap();
    for i in 0..3 {
        let wheel = cx
            .element_bounds(window, format!("camera-grading-wheel-{i}"))
            .unwrap();
        assert!(wheel.width >= 90. && wheel.x + wheel.width < 1280.);
    }
    let frame = cx.capture_screenshot(window).unwrap();
    let wheel = cx.element_bounds(window, "camera-grading-wheel-0").unwrap();
    let scale = frame.width() as f32 / 1280.;
    let sample = |x: f32| {
        frame
            .pixel(
                ((wheel.x + wheel.width * x) * scale) as u32,
                ((wheel.y + wheel.height * 0.5) * scale) as u32,
            )
            .unwrap()
    };
    let red = sample(0.8);
    let cyan = sample(0.2);
    assert!(red[0] > red[1].saturating_add(80) && red[0] > red[2].saturating_add(80));
    assert!(cyan[1] > cyan[0].saturating_add(80) && cyan[2] > cyan[0].saturating_add(80));
    capture(&mut cx, "camera-grading.png");
    cx.click(window, "camera-grade-3").unwrap();
    assert!(
        cx.element_bounds(window, "camera-grading-wheel-3")
            .unwrap()
            .width
            > 150.
    );
    cx.click(window, "camera-grading-reset-3").unwrap();
    cx.click(window, "camera-grading-three").unwrap();
    assert!(cx.element_bounds(window, "camera-grading-wheel-0").is_ok());
}
