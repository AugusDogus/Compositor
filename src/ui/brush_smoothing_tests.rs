use super::*;
use crate::ui::{Editor, Gesture, Modifiers};
use compositor::{document::Document, session::Session};
use quickgui::{MouseButton, PointerEvent, PointerPhase, Size, Vector};

fn editor(tool: Tool, mask: bool, zoom: f64) -> Editor {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(160, 100).unwrap();
    if tool != Tool::Brush || mask {
        compositor::edits::fill(&mut doc, [200, 80, 40, 255], false, false).unwrap();
    }
    if mask {
        compositor::edits::add_mask(&mut doc, false).unwrap();
    }
    e.tabs = vec![Session::new(doc, None).into()];
    e.session_mut().fit = false;
    e.session_mut().zoom = zoom;
    e.tools.tool = tool;
    e.tools.mask_target = mask;
    e.tools.brush.diameter = 4.;
    e.tools.brush_smoothing = 20.;
    e
}

fn pointer(e: &mut Editor, phase: PointerPhase, point: Point, modifiers: Modifiers) {
    let (zoom, offset) = e.viewport(320., 200.);
    let p = quickgui::Point::new(
        (point[0] * zoom + offset[0]) as f32,
        (point[1] * zoom + offset[1]) as f32,
    );
    e.pointer(&PointerEvent {
        phase,
        position: p,
        origin: p,
        local_position: p,
        local_origin: p,
        delta: Vector::ZERO,
        button: MouseButton::Left,
        modifiers,
        size: Size::new(320., 200.),
    })
    .unwrap();
}

#[test]
fn rope_filters_jitter_and_retains_a_constant_screen_length_at_every_zoom() {
    for zoom in [0.25, 1., 4.] {
        let mut rope = Rope::start(Tool::Brush, 20., [0., 0.]).unwrap();
        assert_eq!(rope.pull([10. / zoom, 10. / zoom], zoom), None);
        let point = rope.pull([30. / zoom, 40. / zoom], zoom).unwrap();
        assert!((point[0] * zoom - 18.).abs() < 1e-9);
        assert!((point[1] * zoom - 24.).abs() < 1e-9);
        assert_eq!(rope.pull([20. / zoom, 25. / zoom], zoom), None);
        let point = rope.pull([60. / zoom, 24. / zoom], zoom).unwrap();
        assert!((point[0] * zoom - 40.).abs() < 1e-9);
        assert!((point[1] * zoom - 24.).abs() < 1e-9);
    }
    for (tool, _) in Tool::ALL {
        assert_eq!(
            Rope::start(tool, 50., [0., 0.]).is_some(),
            matches!(tool, Tool::Brush | Tool::Erase)
        );
    }
    assert!(Rope::start(Tool::Brush, 0., [0., 0.]).is_none());
    assert_eq!(Editor::with_test_document().tools.brush_smoothing, 0.);
}

#[test]
fn slack_paint_and_erase_including_masks_catch_up_on_release_in_one_undo() {
    for tool in [Tool::Brush, Tool::Erase] {
        for mask in [false, true] {
            for zoom in [0.5, 1., 2.] {
                let mut e = editor(tool, mask, zoom);
                let original = e.session().document.clone();
                pointer(&mut e, PointerPhase::Down, [40., 50.], Modifiers::empty());
                let dab = e.session().document.clone();
                let end = [40. + 15. / zoom, 50.];
                pointer(&mut e, PointerPhase::Move, end, Modifiers::empty());
                assert_eq!(e.session().document, dab, "Slack rope changed pixels");
                pointer(&mut e, PointerPhase::Up, end, Modifiers::empty());
                let painted = e.session().document.clone();
                assert_ne!(painted, dab, "Release must reach the pointer");
                let alpha = compositor::render::sample(&painted, end).unwrap()[3];
                let expected = if tool == Tool::Brush && !mask { 1. } else { 0. };
                assert!(
                    (alpha - expected).abs() < 0.01,
                    "Release endpoint alpha {alpha}, expected {expected}"
                );
                assert_eq!(e.tools.last_brush.unwrap().2, end);
                assert!(e.gesture.is_none());
                assert!(!e.session().has_pending_edit());
                e.session_mut().undo();
                assert_eq!(e.session().document, original);
                assert!(e.session().undo_label().is_none());
                e.session_mut().redo();
                assert_eq!(e.session().document, painted);
            }
        }
    }
}

#[test]
fn zero_smoothing_follows_pointer_and_does_not_apply_to_other_brush_tools() {
    let mut e = editor(Tool::Brush, false, 1.);
    e.tools.brush_smoothing = 0.;
    pointer(&mut e, PointerPhase::Down, [40., 50.], Modifiers::empty());
    let dab = e.session().document.clone();
    pointer(&mut e, PointerPhase::Move, [45., 50.], Modifiers::empty());
    assert_ne!(e.session().document, dab);
    assert!(matches!(
        e.gesture,
        Some(Gesture::Paint {
            smoothing: None,
            ..
        })
    ));
    pointer(&mut e, PointerPhase::Up, [45., 50.], Modifiers::empty());
    for tool in [Tool::Clone, Tool::Blur, Tool::Heal] {
        let mut e = editor(tool, false, 1.);
        e.tools.clone_source = Some([0., 0.]);
        pointer(&mut e, PointerPhase::Down, [40., 50.], Modifiers::empty());
        assert!(matches!(
            e.gesture,
            Some(Gesture::Paint {
                smoothing: None,
                ..
            })
        ));
        pointer(&mut e, PointerPhase::Cancel, [40., 50.], Modifiers::empty());
    }
}

#[test]
fn cancellation_preserves_the_previous_shift_line_origin_and_discards_the_rope() {
    let mut e = editor(Tool::Brush, false, 1.);
    pointer(&mut e, PointerPhase::Down, [20., 50.], Modifiers::empty());
    pointer(&mut e, PointerPhase::Up, [20., 50.], Modifiers::empty());
    let original = e.session().document.clone();
    let last = e.tools.last_brush;
    pointer(&mut e, PointerPhase::Down, [70., 50.], Modifiers::empty());
    pointer(&mut e, PointerPhase::Move, [100., 50.], Modifiers::empty());
    pointer(
        &mut e,
        PointerPhase::Cancel,
        [100., 50.],
        Modifiers::empty(),
    );
    assert!(e.gesture.is_none());
    assert_eq!(e.session().document, original);
    assert_eq!(e.tools.last_brush, last);
    pointer(&mut e, PointerPhase::Down, [60., 50.], Modifiers::SHIFT);
    // The explicit Shift line reaches its destination immediately, without rope lag.
    let line = e.session().document.clone();
    assert_ne!(line, original);
    let Some(Gesture::Paint {
        smoothing: Some(ref rope),
        ..
    }) = e.gesture
    else {
        panic!("Expected a smoothed stroke");
    };
    assert_eq!(rope.anchor, [60., 50.]);
    pointer(&mut e, PointerPhase::Move, [65., 50.], Modifiers::SHIFT);
    assert_eq!(e.session().document, line);
    pointer(&mut e, PointerPhase::Up, [65., 50.], Modifiers::SHIFT);
    assert_eq!(e.tools.last_brush.unwrap().2, [65., 50.]);
    e.session_mut().undo();
    assert_eq!(e.session().document, original);
}

#[test]
fn smoothing_control_accepts_typed_values_and_is_only_shown_for_paint_and_erase() {
    use quickgui::{Application, WindowOptions};
    let e = editor(Tool::Brush, false, 1.);
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Smoothing").size(1500., 900.), e)
        .unwrap();
    let window = view.window_handle();
    cx.focus(window, "brush-smoothing").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "5").unwrap();
    cx.simulate_input(window, "0").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.brush_smoothing).unwrap(), 50.);
    cx.simulate_keystrokes(window, "up").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.brush_smoothing).unwrap(), 51.);
    cx.focus(window, "brush-smoothing-slider").unwrap();
    cx.simulate_keystrokes(window, "end").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.brush_smoothing).unwrap(), 100.);
    cx.simulate_keystrokes(window, "home").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.brush_smoothing).unwrap(), 0.);
    assert!(
        cx.read(view, |e| e.session().undo_label().is_none())
            .unwrap()
    );
    for tool in [Tool::Erase, Tool::Clone, Tool::Heal, Tool::Blur] {
        cx.update(view, |e, cx| {
            e.tools.tool = tool;
            cx.invalidate();
        })
        .unwrap();
        assert_eq!(
            cx.element_bounds(window, "brush-smoothing").is_ok(),
            tool == Tool::Erase
        );
    }
}
