use super::*;

#[test]
fn graph_uses_source_transparency_with_single_opacity_at_grid_crossings() {
    use quickgui::{Application, WindowOptions};

    let mut editor = Editor::with_test_document();
    editor.open_adjustment(Some(Kind::Curves)).unwrap();
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Curve material").size(1500., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    let bounds = cx.element_bounds(window, "curve-graph").unwrap();
    let shot = cx.capture_screenshot(window).unwrap();
    let scale = shot.width() as f32 / 1500.;
    let background = shot
        .pixel(
            ((bounds.x + 20.) * scale) as u32,
            ((bounds.y + 20.) * scale) as u32,
        )
        .unwrap()[0];
    assert!(
        (i16::from(background) - 29).abs() <= 1,
        "35% black over the panel should produce 29, got {background}"
    );
    // The upper-left quarter crossing is clear of the identity curve. Integrate
    // one-point line coverage, then apply white at 12% exactly once.
    let center_x = (bounds.x + bounds.width / 4.) * scale;
    let center_y = (bounds.y + bounds.height / 4.) * scale;
    for y in center_y.floor() as u32 - 2..=center_y.ceil() as u32 + 2 {
        for x in center_x.floor() as u32 - 2..=center_x.ceil() as u32 + 2 {
            let coverage = |at: u32, center: f32| {
                ((at as f32 + 1.).min(center + scale / 2.) - (at as f32).max(center - scale / 2.))
                    .clamp(0., 1.)
            };
            let union = 1. - (1. - coverage(x, center_x)) * (1. - coverage(y, center_y));
            let expected = f32::from(background) + (255. - f32::from(background)) * 0.12 * union;
            let actual = shot.pixel(x, y).unwrap()[0];
            assert!(
                (f32::from(actual) - expected).abs() <= 1.5,
                "Grid pixel ({x}, {y}): expected {expected}, got {actual}"
            );
        }
    }
}

#[test]
fn graph_grid_is_centered_on_the_curve_coordinate_quarters() {
    use quickgui::{Application, WindowOptions};

    let mut editor = Editor::with_test_document();
    editor.open_adjustment(Some(Kind::Curves)).unwrap();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Curve grid").size(1500., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    let bounds = cx.element_bounds(window, "curve-graph").unwrap();
    let shot = cx.capture_screenshot(window).unwrap();
    let scale = shot.width() as f32 / 1500.;
    // Sample away from the diagonal curve and crossing grid lines. A stroked
    // grid is centered on the same coordinates used to plot the curve points.
    for vertical in [false, true] {
        for quarter in 1..=3 {
            let (start, length, cross) = if vertical {
                (bounds.x, bounds.width, bounds.y + 20.)
            } else {
                (bounds.y, bounds.height, bounds.x + 20.)
            };
            let center = (start + length * quarter as f32 / 4.) * scale;
            let cross = (cross * scale).floor() as u32;
            let sample = |at| {
                if vertical {
                    shot.pixel(at, cross).unwrap()[0]
                } else {
                    shot.pixel(cross, at).unwrap()[0]
                }
            };
            let background = sample(center.floor() as u32 - 5);
            let mut weight = 0.;
            let mut moment = 0.;
            for at in center.floor() as u32 - 3..=center.ceil() as u32 + 3 {
                let coverage = sample(at).saturating_sub(background) as f32;
                weight += coverage;
                moment += (at as f32 + 0.5) * coverage;
            }
            assert!(weight > 0., "Missing grid line");
            let actual = moment / weight;
            assert!(
                (actual - center).abs() < 0.15 * scale,
                "Grid quarter {quarter}, vertical={vertical}: center {actual}, expected {center}"
            );
        }
    }
}

fn pointer(x: f32, y: f32, phase: PointerPhase, width: f32) -> PointerEvent {
    let point = quickgui::Point::new(x / 255. * width, (1. - y / 255.) * 260.);
    PointerEvent {
        phase,
        position: point,
        origin: point,
        local_position: point,
        local_origin: point,
        delta: quickgui::Vector::ZERO,
        button: MouseButton::Left,
        modifiers: Modifiers::empty(),
        size: quickgui::Size::new(width, 260.),
    }
}

#[test]
fn curves_use_nearest_tone_space_hit_testing_and_update_on_press() {
    for width in [330., 660.] {
        let mut e = Editor::with_test_document();
        e.open_adjustment(Some(Kind::Curves)).unwrap();
        e.adjustment_edit.as_mut().unwrap().settings.curves.channels[0] = vec![
            CurvePoint { x: 0., y: 0. },
            CurvePoint { x: 100., y: 100. },
            CurvePoint { x: 110., y: 100. },
            CurvePoint { x: 255., y: 255. },
        ];
        e.show_adjustment_fields();
        e.curve_pointer(&pointer(109., 108., PointerPhase::Down, width))
            .unwrap();
        let edit = e.adjustment_edit.as_ref().unwrap();
        assert_eq!(edit.curve.selected(), Some(2));
        assert!((edit.settings.curves.channels[0][2].y - 108.).abs() < 0.001);
        assert_eq!(edit.settings.curves.channels[0].len(), 4);
        e.curve_pointer(&pointer(95., 130., PointerPhase::Move, width))
            .unwrap();
        assert_eq!(
            e.adjustment_edit.as_ref().unwrap().settings.curves.channels[0][2].x,
            101.
        );
    }
}

#[test]
fn new_curve_points_keep_one_tone_clearance_from_neighbors_and_endpoints() {
    let mut e = Editor::with_test_document();
    e.open_adjustment(Some(Kind::Curves)).unwrap();
    e.adjustment_edit.as_mut().unwrap().settings.curves.channels[0] = vec![
        CurvePoint { x: 0., y: 0. },
        CurvePoint { x: 100., y: 100. },
        CurvePoint { x: 255., y: 255. },
    ];
    e.show_adjustment_fields();
    for x in [0.5, 100.5, 254.5] {
        e.curve_pointer(&pointer(x, 200., PointerPhase::Down, 330.))
            .unwrap();
        assert_eq!(
            e.adjustment_edit.as_ref().unwrap().settings.curves.channels[0].len(),
            3
        );
        e.curve_pointer(&pointer(x, 200., PointerPhase::Up, 330.))
            .unwrap();
    }
    e.curve_pointer(&pointer(102., 200., PointerPhase::Down, 330.))
        .unwrap();
    assert_eq!(
        e.adjustment_edit.as_ref().unwrap().settings.curves.channels[0].len(),
        4
    );
}
