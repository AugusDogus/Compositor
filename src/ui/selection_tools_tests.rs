use super::*;
use compositor::selection::{Selection, SelectionMode};
use quickgui::{
    Application, MouseButton, Point, PointerEvent, PointerPhase, Size, Vector, WindowOptions,
};

fn pointer(point: [f32; 2], phase: PointerPhase, modifiers: Modifiers) -> PointerEvent {
    let point = Point::new(point[0], point[1]);
    PointerEvent {
        phase,
        position: point,
        origin: point,
        local_position: point,
        local_origin: point,
        delta: Vector::ZERO,
        button: MouseButton::Left,
        modifiers,
        size: Size::new(100., 100.),
    }
}

fn editor(tool: Tool) -> Editor {
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(Document::new(100, 100).unwrap(), None).into()];
    editor.session_mut().fit = false;
    editor.session_mut().zoom = 1.;
    editor.tools.tool = tool;
    editor
}

fn draw(e: &mut Editor, points: &[[f32; 2]]) {
    if e.tools.tool == Tool::Polygon {
        for &point in points {
            e.pointer(&pointer(point, PointerPhase::Down, Modifiers::empty()))
                .unwrap();
        }
        e.commit_polygon().unwrap();
    } else {
        for (index, &point) in points.iter().enumerate() {
            let phase = if index == 0 {
                PointerPhase::Down
            } else if index == points.len() - 1 {
                PointerPhase::Up
            } else {
                PointerPhase::Move
            };
            e.pointer(&pointer(point, phase, Modifiers::empty()))
                .unwrap();
        }
    }
}

#[test]
fn selections_and_shapes_do_not_require_an_active_visible_pixel_target() {
    for target in [
        "empty document",
        "hidden layer",
        "folder",
        "adjustment",
        "multiple layers",
        "empty selection",
    ] {
        let mut e = editor(Tool::Rectangle);
        let doc = &mut e.session_mut().document;
        match target {
            "empty document" => {
                doc.layers.clear();
                doc.active = None;
                doc.selected.clear();
            }
            "hidden layer" => doc.layers[0].visible = false,
            "folder" => compositor::layer_ops::group(doc).unwrap(),
            "adjustment" => {
                doc.layers[0].content = compositor::document::LayerContent::Adjustment(Box::new(
                    compositor::adjustment::Adjustment::new(Kind::Exposure),
                ))
            }
            "multiple layers" => {
                let first = doc.active.unwrap();
                doc.add(compositor::document::Layer::blank("Second", 100, 100))
                    .unwrap();
                doc.select(first, true);
            }
            _ => {
                doc.selection = Some(Selection::rectangle(
                    100,
                    100,
                    [110., 110.],
                    [140., 140.],
                    false,
                ))
            }
        }
        assert!(!e.can_edit_pixels(), "{target}");
        let original = e.session().document.clone();
        draw(&mut e, &[[10., 10.], [70., 70.]]);
        assert_eq!(
            e.session().document.selection.as_ref().unwrap().bounds(),
            Some([10., 10., 70., 70.]),
            "{target}"
        );
        assert_eq!(e.session().document.layers, original.layers);
        let selected = e.session().document.clone();
        e.tools.tool = Tool::Shape;
        draw(&mut e, &[[20., 20.], [60., 60.]]);
        assert_eq!(
            e.session().document.layers.len(),
            selected.layers.len() + 1,
            "{target}"
        );
        assert_eq!(e.session().document.selection, selected.selection);
        for layer in &selected.layers {
            assert_eq!(e.session().document.layer(layer.id), Some(layer));
        }
        e.session_mut().undo();
        assert_eq!(e.session().document, selected);
        e.session_mut().undo();
        assert_eq!(e.session().document, original);
    }
}

#[test]
fn selection_drafts_preserve_the_prior_selection_until_mouse_release() {
    for tool in [Tool::Rectangle, Tool::Ellipse, Tool::Lasso] {
        for mode in [
            SelectionMode::Replace,
            SelectionMode::Add,
            SelectionMode::Subtract,
        ] {
            let mut e = editor(tool);
            e.tools.selection_mode = mode;
            e.session_mut().document.selection =
                Some(Selection::rectangle(100, 100, [2., 2.], [12., 12.], false));
            let original = e.session().document.clone();
            e.pointer(&pointer([70., 70.], PointerPhase::Down, Modifiers::empty()))
                .unwrap();
            for point in [[70., 0.], [0., 0.]] {
                e.pointer(&pointer(point, PointerPhase::Move, Modifiers::empty()))
                    .unwrap();
                assert!(
                    e.session().document == original,
                    "{tool:?}/{mode:?}: a draft must not replace the prior selection"
                );
                assert!(e.session().undo_label().is_none());
            }
            e.pointer(&pointer([0., 0.], PointerPhase::Up, Modifiers::empty()))
                .unwrap();
            assert_ne!(
                e.session().document.selection,
                original.selection,
                "{tool:?}/{mode:?}"
            );
            assert!(e.session().undo_label().is_some());
            e.session_mut().undo();
            assert_eq!(e.session().document, original);
        }
    }
}

#[test]
fn freehand_drafts_skip_points_within_a_quarter_document_pixel() {
    let mut e = editor(Tool::Lasso);
    e.pointer(&pointer([20., 20.], PointerPhase::Down, Modifiers::empty()))
        .unwrap();
    for point in [[20.1, 20.1], [20.3, 20.3], [20.4, 20.4]] {
        e.pointer(&pointer(point, PointerPhase::Move, Modifiers::empty()))
            .unwrap();
    }
    let Some(Gesture::Lasso { points, .. }) = &e.gesture else {
        panic!("Missing freehand draft");
    };
    assert_eq!(
        points.len(),
        2,
        "The source keeps the first point and the next point at least 0.25 pixels away"
    );
    e.pointer(&pointer(
        [20.4, 20.4],
        PointerPhase::Cancel,
        Modifiers::empty(),
    ))
    .unwrap();
    assert!(e.gesture.is_none());
    assert!(e.session().undo_label().is_none());
}

#[test]
fn shape_drags_preserve_the_starting_radius_and_constrain_from_the_anchor() {
    for ellipse in [false, true] {
        for (modifiers, origin, size) in [
            (Modifiers::empty(), [20., 20.], [50., 20.]),
            (Modifiers::SHIFT, [20., 20.], [50., 50.]),
            (Modifiers::ALT, [-30., 0.], [100., 40.]),
            (
                Modifiers::SHIFT | Modifiers::ALT,
                [-30., -30.],
                [100., 100.],
            ),
        ] {
            let mut e = editor(Tool::Shape);
            e.tools.shape_kind = if ellipse {
                compositor::document::ShapeKind::Ellipse
            } else {
                compositor::document::ShapeKind::Rectangle
            };
            e.tools.shape_radius = 12.;
            let original = e.session().document.clone();
            e.pointer(&pointer([20., 20.], PointerPhase::Down, modifiers))
                .unwrap();
            e.tools.shape_radius = 1000.;
            e.pointer(&pointer([70., 40.], PointerPhase::Up, modifiers))
                .unwrap();
            let layer = e.session().document.active_layer().unwrap();
            assert_eq!(layer.transform.origin, origin);
            assert_eq!(layer.transform.size, size);
            assert_eq!(
                layer.shape.as_ref().unwrap().corner_radius,
                if ellipse { 0. } else { 12. }
            );
            e.session_mut().undo();
            assert_eq!(e.session().document, original);
        }
    }
}

#[test]
fn completing_a_shape_targets_its_pixels_and_preserves_the_existing_selection() {
    for ellipse in [false, true] {
        let mut e = editor(Tool::Shape);
        e.tools.shape_kind = if ellipse {
            compositor::document::ShapeKind::Ellipse
        } else {
            compositor::document::ShapeKind::Rectangle
        };
        compositor::edits::add_mask(&mut e.session_mut().document, false).unwrap();
        e.tools.mask_target = true;
        e.session_mut().document.selection =
            Some(Selection::rectangle(100, 100, [1., 1.], [5., 5.], false));
        let original = e.session().document.clone();
        draw(&mut e, &[[20., 20.], [20., 20.]]);
        assert!(
            e.tools.mask_target,
            "A click without a shape retains its editing target"
        );
        assert_eq!(e.session().document, original);
        assert!(e.session().undo_label().is_none());
        draw(&mut e, &[[20., 20.], [60., 70.]]);
        assert_eq!(e.session().document.selection, original.selection);
        assert_eq!(e.session().document.layers[0], original.layers[0]);
        assert_eq!(e.session().document.layers.len(), 2);
        assert!(
            !e.tools.mask_target,
            "The new shape has no mask; its pixels must be the editing target"
        );
        assert!(e.can_edit_pixels());
        e.session_mut().undo();
        assert_eq!(e.session().document, original);
    }
}

#[test]
fn canvas_selection_and_shape_commands_use_source_history_names() {
    for (tool, ellipse, label) in [
        (Tool::Rectangle, false, "Rectangular Marquee"),
        (Tool::Ellipse, false, "Elliptical Marquee"),
        (Tool::Lasso, false, "Lasso"),
        (Tool::Polygon, false, "Polygonal Lasso"),
        (Tool::Shape, false, "Rectangle"),
        (Tool::Shape, true, "Ellipse"),
    ] {
        let mut e = editor(tool);
        e.tools.shape_kind = if ellipse {
            compositor::document::ShapeKind::Ellipse
        } else {
            compositor::document::ShapeKind::Rectangle
        };
        let original = e.session().document.clone();
        draw(&mut e, &[[10., 10.], [70., 10.], [70., 70.]]);
        assert_eq!(e.session().undo_label(), Some(label), "{tool:?}");
        e.session_mut().undo();
        assert_eq!(e.session().document, original);
        assert_eq!(e.session().undo_label(), None);
    }
    for tool in [Tool::Rectangle, Tool::Ellipse, Tool::Lasso, Tool::Polygon] {
        let mut e = editor(tool);
        e.session_mut().document.selection =
            Some(Selection::rectangle(100, 100, [0., 0.], [5., 5.], false));
        let original = e.session().document.clone();
        draw(&mut e, &[[20., 20.], [20., 20.]]);
        assert!(e.session().document.selection.is_none());
        assert_eq!(e.session().undo_label(), Some("Deselect"), "{tool:?}");
        e.session_mut().undo();
        assert_eq!(e.session().document, original);
    }
}

#[test]
fn off_canvas_lassos_keep_an_explicit_empty_selection_instead_of_deselecting() {
    for tool in [Tool::Lasso, Tool::Polygon] {
        for mode in [
            SelectionMode::Replace,
            SelectionMode::Add,
            SelectionMode::Subtract,
        ] {
            let mut e = editor(tool);
            e.tools.selection_mode = mode;
            e.session_mut().document.selection =
                Some(Selection::rectangle(100, 100, [0., 0.], [5., 5.], false));
            assert!(e.can_edit_pixels());
            let before = e.session().document.clone();
            draw(
                &mut e,
                &[[110., 110.], [140., 110.], [140., 140.], [110., 140.]],
            );
            let selection = e
                .session()
                .document
                .selection
                .as_ref()
                .expect("A valid outline remains a selection even outside the canvas");
            assert_eq!(
                selection.bounds(),
                if mode == SelectionMode::Replace {
                    None
                } else {
                    Some([0., 0., 5., 5.])
                }
            );
            assert_eq!(e.session().document.layers, before.layers);
            if mode == SelectionMode::Replace {
                assert!(!e.can_edit_pixels());
                e.session_mut().undo();
                assert_eq!(e.session().document, before);
            }
        }
    }
}

#[test]
fn changing_shape_kind_cancels_the_drag_before_switching_the_header() {
    for keyboard in [false, true] {
        let e = editor(Tool::Shape);
        let before = e.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Shape switch").size(1280., 900.), e)
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, "workspace").unwrap();
        cx.update(view, |e, _| {
            e.pointer(&pointer([10., 10.], PointerPhase::Down, Modifiers::empty()))
                .unwrap();
            e.pointer(&pointer([60., 60.], PointerPhase::Move, Modifiers::empty()))
                .unwrap();
            assert!(e.gesture.is_some());
        })
        .unwrap();
        if keyboard {
            cx.simulate_keystrokes(window, "shift-u").unwrap();
        } else {
            cx.click(window, "shape-Ellipse").unwrap();
        }
        cx.update(view, |e, _| {
            assert_eq!(e.tools.shape_kind, compositor::document::ShapeKind::Ellipse);
            assert!(
                e.gesture.is_none(),
                "Changing shape kind must cancel the old draft"
            );
            e.pointer(&pointer([60., 60.], PointerPhase::Up, Modifiers::empty()))
                .unwrap();
            assert_eq!(e.session().document, before);
            assert_eq!(e.session().undo_label(), None);
            draw(e, &[[10., 10.], [60., 60.]]);
            assert_eq!(e.session().undo_label(), Some("Ellipse"));
        })
        .unwrap();
    }
}

#[test]
fn expand_and_contract_controls_validate_apply_and_undo() {
    let mut e = editor(Tool::Rectangle);
    let original = Selection::marquee(100, 100, [40., 40.], [60., 60.], false, true).unwrap();
    e.session_mut().document.selection = Some(original.clone());
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Resize Selection").size(1280., 900.), e)
        .unwrap();
    let window = view.window_handle();
    cx.update(view, |e, _| {
        assert!(e.resize_selection(true, 501).is_err());
        assert_eq!(e.session().document.selection, Some(original.clone()));
    })
    .unwrap();
    cx.focus(window, "selection-expand-amount").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "501").unwrap();
    assert_eq!(
        cx.read(view, |e| e.tools.selection_expand_amount).unwrap(),
        500
    );
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "5").unwrap();
    cx.click(window, 413_u64).unwrap();
    assert_eq!(
        cx.read(view, |e| e
            .session()
            .document
            .selection
            .as_ref()
            .unwrap()
            .bounds())
            .unwrap(),
        Some([35., 35., 65., 65.])
    );
    cx.focus(window, "selection-contract-amount").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "8").unwrap();
    cx.click(window, 414_u64).unwrap();
    assert_eq!(
        cx.read(view, |e| e
            .session()
            .document
            .selection
            .as_ref()
            .unwrap()
            .bounds())
            .unwrap(),
        Some([43., 43., 57., 57.])
    );
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("z".into()), Modifiers::CONTROL),
    )
    .unwrap();
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("z".into()), Modifiers::CONTROL),
    )
    .unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.selection.clone())
            .unwrap(),
        Some(original)
    );
    assert_eq!(
        cx.read(view, |e| e.tools.selection_expand_amount).unwrap(),
        5
    );
    assert_eq!(
        cx.read(view, |e| e.tools.selection_contract_amount)
            .unwrap(),
        8
    );
    assert!(cx.read(view, |e| e.modal.is_none()).unwrap());
}

#[test]
fn polygon_preserves_starting_mode_and_closes_inside_the_original_selection() {
    let mut e = editor(Tool::Polygon);
    let base = Selection::rectangle(100, 100, [0., 0.], [90., 90.], false);
    e.session_mut().document.selection = Some(base.clone());
    for (point, modifiers) in [
        ([10., 10.], Modifiers::ALT),
        ([70., 10.], Modifiers::empty()),
        ([10., 70.], Modifiers::SHIFT),
        ([10., 10.], Modifiers::empty()),
    ] {
        e.pointer(&pointer(point, PointerPhase::Down, modifiers))
            .unwrap();
    }
    assert!(e.tools.polygon.is_none());
    assert!(e.gesture.is_none());
    let result = e.session().document.selection.as_ref().unwrap();
    assert_eq!(result.coverage([20., 20.]), 0.);
    assert_eq!(result.coverage([80., 80.]), 1.);
    e.session_mut().undo();
    assert_eq!(e.session().document.selection, Some(base));
}

#[test]
fn subtraction_without_selection_and_empty_lasso_do_not_select_pixels() {
    for tool in [Tool::Rectangle, Tool::Lasso, Tool::Polygon] {
        let mut e = editor(tool);
        e.tools.selection_mode = SelectionMode::Subtract;
        e.pointer(&pointer([10., 10.], PointerPhase::Down, Modifiers::empty()))
            .unwrap();
        if tool == Tool::Polygon {
            for point in [[70., 10.], [10., 70.], [10., 10.]] {
                e.pointer(&pointer(point, PointerPhase::Down, Modifiers::empty()))
                    .unwrap();
            }
        } else {
            e.pointer(&pointer([70., 10.], PointerPhase::Move, Modifiers::empty()))
                .unwrap();
            e.pointer(&pointer([10., 70.], PointerPhase::Up, Modifiers::empty()))
                .unwrap();
        }
        assert!(e.session().document.selection.is_none());
        assert!(e.session().undo_label().is_none());
    }
    let mut e = editor(Tool::Lasso);
    e.session_mut().document.selection =
        Some(Selection::rectangle(100, 100, [0., 0.], [5., 5.], false));
    e.pointer(&pointer([20., 20.], PointerPhase::Down, Modifiers::empty()))
        .unwrap();
    e.pointer(&pointer([20., 20.], PointerPhase::Up, Modifiers::empty()))
        .unwrap();
    assert!(e.session().document.selection.is_none());
}

#[test]
fn marquee_shift_at_press_adds_and_alt_subtracts_without_centering() {
    let mut e = editor(Tool::Rectangle);
    e.pointer(&pointer([10., 10.], PointerPhase::Down, Modifiers::SHIFT))
        .unwrap();
    e.pointer(&pointer([70., 30.], PointerPhase::Up, Modifiers::SHIFT))
        .unwrap();
    assert_eq!(
        e.session().document.selection.as_ref().unwrap().bounds(),
        Some([10., 10., 70., 30.])
    );
    e.pointer(&pointer(
        [10., 10.],
        PointerPhase::Down,
        Modifiers::ALT | Modifiers::SHIFT,
    ))
    .unwrap();
    e.pointer(&pointer(
        [40., 30.],
        PointerPhase::Up,
        Modifiers::ALT | Modifiers::SHIFT,
    ))
    .unwrap();
    assert_eq!(
        e.session().document.selection.as_ref().unwrap().bounds(),
        Some([40., 10., 70., 30.])
    );
    e.session_mut().document.selection = None;
    e.pointer(&pointer([10., 10.], PointerPhase::Down, Modifiers::empty()))
        .unwrap();
    e.pointer(&pointer([70., 30.], PointerPhase::Up, Modifiers::SHIFT))
        .unwrap();
    assert_eq!(
        e.session().document.selection.as_ref().unwrap().bounds(),
        Some([10., 10., 70., 70.])
    );
}

#[test]
fn polygon_toolbar_mode_vertex_deletion_cancel_and_double_click() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Polygon").size(1280., 900.),
            editor(Tool::Polygon),
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "selection-mode-Add").unwrap();
    cx.update(view, |e, _| {
        for point in [[10., 10.], [70., 10.], [10., 70.]] {
            e.pointer(&pointer(point, PointerPhase::Down, Modifiers::empty()))
                .unwrap();
        }
        assert_eq!(e.tools.polygon.as_ref().unwrap().mode, SelectionMode::Add);
    })
    .unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Backspace, Modifiers::empty()),
    )
    .unwrap();
    assert_eq!(
        cx.read(view, |e| e.tools.polygon.as_ref().unwrap().points.len())
            .unwrap(),
        2
    );
    cx.update(view, |e, _| {
        e.polygon_click([10., 70.], 1., SelectionMode::Replace)
            .unwrap()
    })
    .unwrap();
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    cx.simulate_mouse_down(
        window,
        "canvas",
        quickgui::MouseDownEvent {
            position: Point::new(bounds.x + 20., bounds.y + 20.),
            button: MouseButton::Left,
            modifiers: Modifiers::empty(),
            click_count: 2,
            first_mouse: false,
        },
    )
    .unwrap();
    cx.read(view, |e| {
        assert!(e.tools.polygon.is_none());
        assert_eq!(
            e.session()
                .document
                .selection
                .as_ref()
                .unwrap()
                .coverage([20., 20.]),
            1.
        );
    })
    .unwrap();
    cx.update(view, |e, _| {
        e.polygon_click([90., 90.], 1., SelectionMode::Add).unwrap()
    })
    .unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Escape, Modifiers::empty()),
    )
    .unwrap();
    assert!(cx.read(view, |e| e.tools.polygon.is_none()).unwrap());
    assert!(
        cx.read(view, |e| e.session().document.selection.is_some())
            .unwrap()
    );
}

#[test]
fn feather_controls_validate_cancel_apply_and_undo() {
    let mut e = editor(Tool::Rectangle);
    let original = Selection::rectangle(100, 100, [30., 30.], [70., 70.], false);
    e.session_mut().document.selection = Some(original.clone());
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Feather Selection").size(1280., 900.), e)
        .unwrap();
    let window = view.window_handle();
    cx.update(view, |e, cx| e.action(Action::FeatherSelection, cx))
        .unwrap();
    cx.click(window, "form-cancel").unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.selection.clone())
            .unwrap(),
        Some(original.clone())
    );
    cx.update(view, |e, cx| {
        assert!(
            e.apply_form(Action::FeatherSelection, vec!["251".into()])
                .is_err()
        );
        assert!(
            e.apply_form(Action::FeatherSelection, vec!["1.5".into()])
                .is_err()
        );
        e.action(Action::FeatherSelection, cx);
        e.update_form_field(0, "8");
    })
    .unwrap();
    cx.click(window, "form-apply").unwrap();
    let coverage = cx
        .read(view, |e| {
            assert!(e.modal.is_none());
            assert_eq!(e.session().undo_label(), Some("Feather Selection"));
            e.session()
                .document
                .selection
                .as_ref()
                .unwrap()
                .coverage([29.5, 50.5])
        })
        .unwrap();
    assert!(coverage > 0. && coverage < 1.);
    cx.click(window, "selection-feather").unwrap();
    cx.update(view, |e, _| {
        e.session_mut().undo();
        assert_eq!(
            e.session()
                .document
                .selection
                .as_ref()
                .unwrap()
                .coverage([29.5, 50.5]),
            coverage
        );
        e.session_mut().undo();
        assert_eq!(e.session().document.selection, Some(original));
    })
    .unwrap();
}

#[test]
fn line_header_cycles_shapes_and_constrained_drag_keeps_starting_width() {
    use compositor::document::{ShapeGeometry, ShapeKind};
    let e = editor(Tool::Shape);
    let before = e.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Line").size(1280., 900.), e)
        .unwrap();
    let window = view.window_handle();
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystrokes(window, "shift-u shift-u").unwrap();
    assert_eq!(
        cx.read(view, |e| e.tools.shape_kind).unwrap(),
        ShapeKind::Line
    );
    cx.focus(window, "shape-line-width").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "6").unwrap();
    cx.update(view, |e, _| {
        e.session_mut().zoom = e.session().backing_scale;
        e.pointer(&pointer([40., 40.], PointerPhase::Down, Modifiers::empty()))
            .unwrap();
        e.tools.shape_line_width = 20.;
        e.pointer(&pointer(
            [60., 48.],
            PointerPhase::Up,
            Modifiers::SHIFT | Modifiers::ALT,
        ))
        .unwrap();
        assert_eq!(e.session().undo_label(), Some("Line"));
        let layer = e.session().document.active_layer().unwrap();
        let ShapeGeometry::Line {
            line_width,
            start,
            end,
        } = layer.shape.unwrap().geometry
        else {
            panic!("expected line");
        };
        assert_eq!(line_width, 6.);
        assert_eq!(start[1], end[1]);
        assert!((layer.transform.geometry_point([0.5, 0.5])[0] - 40.).abs() < 0.01);
        e.session_mut().undo();
        assert_eq!(e.session().document, before);
    })
    .unwrap();
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystrokes(window, "shift-u").unwrap();
    assert_eq!(
        cx.read(view, |e| e.tools.shape_kind).unwrap(),
        ShapeKind::Rectangle
    );
}
