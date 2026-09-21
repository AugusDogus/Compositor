use super::*;
use quickgui::{
    Application, MouseButton, Point as UiPoint, PointerEvent, PointerPhase, Size, Vector,
    WindowOptions,
};

fn pointer(x: f32, phase: PointerPhase, modifiers: Modifiers) -> PointerEvent {
    PointerEvent {
        phase,
        position: UiPoint::new(x, 50.),
        origin: UiPoint::new(50., 50.),
        local_position: UiPoint::new(x, 50.),
        local_origin: UiPoint::new(50., 50.),
        delta: Vector::ZERO,
        button: MouseButton::Right,
        modifiers,
        size: Size::new(100., 100.),
    }
}

#[test]
fn right_drag_adjusts_size_or_hardness_from_the_press_without_editing_pixels() {
    let mut e = Editor::with_test_document();
    e.tools.tool = Tool::Brush;
    e.session_mut().fit = false;
    e.session_mut().zoom = 2.;
    e.tools.brush.diameter = 40.;
    e.tools.brush.hardness = 0.25;
    let original = e.session().document.clone();
    e.pointer(&pointer(50., PointerPhase::Down, Modifiers::empty()))
        .unwrap();
    e.pointer(&pointer(70., PointerPhase::Move, Modifiers::empty()))
        .unwrap();
    assert_eq!(e.tools.brush.diameter, 60.);
    e.pointer(&pointer(100., PointerPhase::Move, Modifiers::SHIFT))
        .unwrap();
    assert_eq!(
        e.tools.brush.diameter, 40.,
        "Switching to hardness restores the original diameter"
    );
    assert_eq!(e.tools.brush.hardness, 0.5);
    e.pointer(&pointer(90., PointerPhase::Move, Modifiers::empty()))
        .unwrap();
    assert_eq!(e.tools.brush.diameter, 80.);
    assert_eq!(e.tools.brush.hardness, 0.25);
    e.pointer(&pointer(90., PointerPhase::Up, Modifiers::empty()))
        .unwrap();
    assert!(e.gesture.is_none());
    assert_eq!(e.session().document, original);
    assert!(e.session().undo_label().is_none());
}

struct CursorView(Editor);
impl View for CursorView {
    fn render(&mut self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .bg(Color::rgb8(100, 100, 100))
            .child(self.0.brush_cursor(1., [0., 0.]).unwrap())
    }
}

#[test]
fn brush_circle_and_clone_marker_use_black_centers_with_white_outer_strokes() {
    let mut e = Editor::with_test_document();
    e.tools.tool = Tool::Clone;
    e.tools.brush.diameter = 40.;
    e.tools.clone_source = Some([20.5, 20.5]);
    e.canvas_pointer = Some([60.5, 60.5]);
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Brush cursor")
                .size(120., 120.)
                .minimum_size(1., 1.),
            CursorView(e),
        )
        .unwrap();
    let frame = cx.capture_screenshot(view.window_handle()).unwrap();
    let scale = frame.width() as f32 / 120.;
    let sample = |x: f32, y: f32| frame.pixel((x * scale) as u32, (y * scale) as u32).unwrap()[0];
    assert!(
        sample(80.25, 60.5) < 70,
        "The circle center stroke must be black"
    );
    assert!(
        sample(81., 60.5) > 160,
        "The circle needs a white outer stroke"
    );
    assert!(
        sample(24., 20.5) < 70,
        "The clone marker center must be black"
    );
    assert!(
        sample(24., 21.5) > 160,
        "The clone marker needs a white outer stroke"
    );
}

#[test]
fn hardness_drag_draws_a_dashed_inner_ring_at_the_press_point() {
    let mut e = Editor::with_test_document();
    e.tools.tool = Tool::Brush;
    e.session_mut().fit = false;
    e.tools.brush.diameter = 40.;
    e.tools.brush.hardness = 0.25;
    e.canvas_pointer = Some([100., 50.]);
    e.pointer(&pointer(50., PointerPhase::Down, Modifiers::SHIFT))
        .unwrap();
    e.pointer(&pointer(100., PointerPhase::Move, Modifiers::SHIFT))
        .unwrap();
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Hardness ring")
                .size(140., 120.)
                .minimum_size(1., 1.),
            CursorView(e),
        )
        .unwrap();
    let window = view.window_handle();
    let during = cx.capture_screenshot(window).unwrap();
    cx.update(view, |v, cx| {
        v.0.pointer(&pointer(100., PointerPhase::Up, Modifiers::SHIFT))
            .unwrap();
        cx.invalidate();
    })
    .unwrap();
    let after = cx.capture_screenshot(window).unwrap();
    let scale = during.width() as f32 / 140.;
    let mut changed = 0;
    for y in 38..63 {
        for x in 38..63 {
            let p = during
                .pixel((x as f32 * scale) as u32, (y as f32 * scale) as u32)
                .unwrap();
            if p[0].abs_diff(100) > 30 {
                changed += 1;
            }
            assert_eq!(
                after
                    .pixel((x as f32 * scale) as u32, (y as f32 * scale) as u32)
                    .unwrap(),
                [100, 100, 100, 255]
            );
        }
    }
    assert!(
        changed > 20,
        "The inner hardness ring should be visible while dragging"
    );
}

#[test]
fn secondary_events_preserve_an_active_stroke_and_tip_ranges_are_clamped() {
    let mut e = Editor::with_test_document();
    e.tools.tool = Tool::Brush;
    e.session_mut().fit = false;
    let mut paint = pointer(50., PointerPhase::Down, Modifiers::empty());
    paint.button = MouseButton::Left;
    e.pointer(&paint).unwrap();
    assert!(matches!(e.gesture, Some(Gesture::Paint { .. })));
    let painted = e.session().document.clone();
    for phase in [PointerPhase::Down, PointerPhase::Move, PointerPhase::Up] {
        e.pointer(&pointer(80., phase, Modifiers::empty())).unwrap();
        assert!(matches!(e.gesture, Some(Gesture::Paint { .. })));
        assert_eq!(e.session().document, painted);
    }
    paint.phase = PointerPhase::Cancel;
    e.pointer(&paint).unwrap();
    e.pointer(&pointer(50., PointerPhase::Down, Modifiers::empty()))
        .unwrap();
    for (x, shift, diameter, hardness) in [
        (5000., false, 2000., 1.),
        (-5000., false, 1., 1.),
        (5000., true, 40., 1.),
        (-5000., true, 40., 0.),
    ] {
        e.pointer(&pointer(
            x,
            PointerPhase::Move,
            if shift {
                Modifiers::SHIFT
            } else {
                Modifiers::empty()
            },
        ))
        .unwrap();
        assert_eq!(e.tools.brush.diameter, diameter);
        assert_eq!(e.tools.brush.hardness, hardness);
    }
    e.pointer(&pointer(50., PointerPhase::Cancel, Modifiers::empty()))
        .unwrap();
    assert!(e.gesture.is_none());
    assert!(e.session().undo_label().is_none());
}
