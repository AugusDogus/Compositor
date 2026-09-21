use super::*;
use compositor::{document::Layer, selection::Selection};
use quickgui::{MouseButton, Point, PointerEvent, PointerPhase, Size, Vector};

fn editor(tool: Tool) -> Editor {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(20, 20).unwrap();
    compositor::edits::fill(&mut doc, [80, 140, 210, 255], false, false).unwrap();
    doc.selection = Some(Selection::rectangle(20, 20, [4., 4.], [12., 12.], false));
    e.tabs = vec![Session::new(doc, None).into()];
    e.session_mut().fit = false;
    e.tools.tool = tool;
    e.tools.show_transform_controls = false;
    e
}

fn pointer(e: &mut Editor, phase: PointerPhase, at: [f32; 2]) {
    let point = Point::new(at[0], at[1]);
    e.pointer(&PointerEvent {
        phase,
        position: point,
        origin: point,
        local_position: point,
        local_origin: point,
        delta: Vector::ZERO,
        button: MouseButton::Left,
        modifiers: Modifiers::CONTROL,
        size: Size::new(20., 20.),
    })
    .unwrap();
}

#[test]
fn control_drag_lifts_pixels_only_inside_a_selection_tool_outline() {
    for tool in [
        Tool::Rectangle,
        Tool::Ellipse,
        Tool::Lasso,
        Tool::Polygon,
        Tool::Wand,
    ] {
        for inside in [false, true] {
            let mut e = editor(tool);
            let before = e.session().document.clone();
            let at = if inside { [8., 8.] } else { [16., 16.] };
            pointer(&mut e, PointerPhase::Down, at);
            assert_eq!(
                matches!(e.gesture, Some(Gesture::Pixels { .. })),
                inside,
                "{tool:?}"
            );
            pointer(&mut e, PointerPhase::Move, [at[0] + 2., at[1] + 2.]);
            pointer(&mut e, PointerPhase::Up, [at[0] + 2., at[1] + 2.]);
            if inside {
                assert_ne!(e.session().document.layers, before.layers);
                assert_eq!(e.session().undo_label(), Some("Move Pixels"));
                e.session_mut().undo();
                assert_eq!(e.session().document, before);
            } else {
                assert_eq!(e.session().document.layers, before.layers);
            }
        }
    }
    let mut e = editor(Tool::Move);
    let before = e.session().document.clone();
    pointer(&mut e, PointerPhase::Down, [8., 8.]);
    pointer(&mut e, PointerPhase::Move, [11., 10.]);
    pointer(&mut e, PointerPhase::Up, [11., 10.]);
    assert_eq!(
        e.session().document.layers[0].content,
        before.layers[0].content
    );
    assert_eq!(e.session().document.layers[0].transform.origin, [3., 2.]);
    assert_eq!(e.session().document.selection, before.selection);
    e.session_mut().undo();
    assert_eq!(e.session().document, before);
}

#[test]
fn transform_command_uses_layer_transforms_when_selection_cannot_float() {
    use quickgui::{Application, WindowOptions};
    for multiple in [false, true] {
        let mut e = editor(Tool::Rectangle);
        if multiple {
            let doc = &mut e.session_mut().document;
            let active = doc.active.unwrap();
            let mut other = doc.layers[0].clone();
            other.id = uuid::Uuid::new_v4();
            other.name = "Second".into();
            doc.add(other).unwrap();
            doc.select(active, true);
        } else {
            e.session_mut().document.selection =
                Some(Selection::rectangle(20, 20, [2., 2.], [2., 2.], false));
        }
        let before = e.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Transform selection eligibility").size(1500., 900.),
                e,
            )
            .unwrap();
        cx.focus(view.window_handle(), "workspace").unwrap();
        cx.simulate_keystrokes(view.window_handle(), "ctrl-t")
            .unwrap();
        cx.update(view, |e, _| {
            assert!(e.pending_pixels.is_none());
            assert!(e.transform_edit.is_some());
            assert_eq!(e.session().document, before);
            e.finish_toolbar_transform(false).unwrap();
            assert_eq!(e.session().document, before);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
    }
}

#[test]
fn selected_pixel_drag_and_nudge_preserve_ineligible_targets() {
    for state in 0..3 {
        let mut e = editor(Tool::Rectangle);
        match state {
            0 => e.session_mut().document.layers[0].visible = false,
            1 => {
                let doc = &mut e.session_mut().document;
                let active = doc.active.unwrap();
                let mut group = Layer::blank("Hidden folder", 20, 20);
                group.content = compositor::document::LayerContent::Group;
                group.visible = false;
                doc.layers[0].parent = Some(group.id);
                doc.add(group).unwrap();
                doc.select(active, false);
            }
            _ => {
                let doc = &mut e.session_mut().document;
                let active = doc.active.unwrap();
                doc.add(Layer::blank("Second", 20, 20)).unwrap();
                doc.select(active, true);
            }
        }
        let before = e.session().document.clone();
        for phase in [PointerPhase::Down, PointerPhase::Move, PointerPhase::Up] {
            pointer(&mut e, phase, [8., 8.]);
            assert!(e.gesture.is_none(), "state {state}");
            assert!(!e.session().has_pending_edit());
        }
        e.nudge(&Key::ArrowRight, Modifiers::CONTROL).unwrap();
        assert_eq!(e.session().document, before, "state {state}");
        assert!(e.session().undo_label().is_none());
    }
}

#[test]
fn selected_pixel_drag_keeps_both_axes_with_shift_and_captures_copy_on_press() {
    for duplicate in [false, true] {
        let mut e = editor(Tool::Rectangle);
        let before = e.session().document.clone();
        for (phase, point, modifiers) in [
            (
                PointerPhase::Down,
                Point::new(8., 8.),
                if duplicate {
                    Modifiers::CONTROL | Modifiers::ALT
                } else {
                    Modifiers::CONTROL
                },
            ),
            (
                PointerPhase::Move,
                Point::new(11., 10.),
                Modifiers::CONTROL | Modifiers::SHIFT,
            ),
            (
                PointerPhase::Up,
                Point::new(11., 10.),
                Modifiers::CONTROL | Modifiers::SHIFT,
            ),
        ] {
            e.pointer(&PointerEvent {
                phase,
                position: point,
                origin: Point::new(8., 8.),
                local_position: point,
                local_origin: Point::new(8., 8.),
                delta: Vector::ZERO,
                button: MouseButton::Left,
                modifiers,
                size: Size::new(20., 20.),
            })
            .unwrap();
        }
        let doc = &e.session().document;
        assert_eq!(
            doc.selection.as_ref().unwrap().bounds(),
            Some([7., 6., 15., 14.])
        );
        assert_eq!(
            doc.layers[0].raster().unwrap()[(5, 5)][3],
            if duplicate { 255 } else { 0 }
        );
        assert_eq!(
            e.session().undo_label(),
            Some(if duplicate {
                "Duplicate Pixels"
            } else {
                "Move Pixels"
            })
        );
        e.session_mut().undo();
        assert_eq!(e.session().document, before);
    }
}
