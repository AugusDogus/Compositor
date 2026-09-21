use super::*;
use quickgui::{MouseButton, PointerEvent, PointerPhase, Size, Vector};

pub(super) fn pointer(
    editor: &mut Editor,
    phase: PointerPhase,
    point: Point,
    modifiers: Modifiers,
) {
    let (zoom, offset) = editor.viewport(500., 400.);
    let p = quickgui::Point::new(
        (offset[0] + point[0] * zoom) as f32,
        (offset[1] + point[1] * zoom) as f32,
    );
    editor
        .pointer(&PointerEvent {
            phase,
            position: p,
            origin: p,
            local_position: p,
            local_origin: p,
            delta: Vector::ZERO,
            size: Size::new(500., 400.),
            button: MouseButton::Left,
            modifiers,
        })
        .unwrap();
}

#[test]
fn layer_fields_and_handle_drags_share_cancel_and_one_undo() {
    for mask in [false, true] {
        for apply in [false, true] {
            let mut e = Editor::with_test_document();
            let mut doc = Document::new(100, 80).unwrap();
            compositor::edits::fill(&mut doc, [80, 140, 200, 255], false, false).unwrap();
            if mask {
                compositor::edits::add_mask(&mut doc, false).unwrap();
                let placement = doc.layers[0].transform;
                let target = doc.layers[0].mask.as_mut().unwrap();
                target.linked = false;
                target.placement = Some(placement);
            }
            let original = doc.clone();
            e.tabs = vec![Session::new(doc, None).into()];
            e.tools.mask_target = mask;
            e.header_transform_input(0, "10").unwrap();
            pointer(&mut e, PointerPhase::Down, [110., 80.], Modifiers::empty());
            pointer(&mut e, PointerPhase::Up, [130., 96.], Modifiers::empty());
            assert!(e.transform_edit.is_some());
            assert!(e.session().undo_label().is_none());
            assert_eq!(e.header_transform_bounds().unwrap().size, [120., 96.]);
            e.header_transform_input(1, "5").unwrap();
            e.finish_toolbar_transform(apply).unwrap();
            if apply {
                assert_eq!(
                    e.session().undo_label(),
                    Some(if mask {
                        "Transform Layer Mask"
                    } else {
                        "Transform Layer"
                    })
                );
                e.session_mut().undo();
            }
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        }
    }
}

fn patterned_document() -> Document {
    let mut doc = Document::new(100, 80).unwrap();
    let pixels = image::RgbaImage::from_fn(100, 80, |x, y| {
        image::Rgba([x as u8 * 2, y as u8 * 3, 160, 255])
    });
    doc.layers[0].content =
        compositor::document::LayerContent::Raster(Some(std::sync::Arc::new(pixels)));
    doc
}

#[test]
fn arrow_nudges_keep_layer_and_mask_transform_drafts_pending() {
    for mask in [false, true] {
        for perspective in [false, true] {
            for apply in [false, true] {
                let mut e = Editor::with_test_document();
                let mut doc = patterned_document();
                if mask {
                    compositor::edits::add_mask(&mut doc, false).unwrap();
                    doc.layers[0].mask.as_mut().unwrap().linked = false;
                }
                e.tabs = vec![Session::new(doc.clone(), None).into()];
                e.tools.mask_target = mask;
                e.start_toolbar_transform().unwrap();
                e.header_transform_input(0, "5").unwrap();
                if perspective {
                    pointer(&mut e, PointerPhase::Down, [5., 0.], Modifiers::CONTROL);
                    pointer(&mut e, PointerPhase::Move, [0., 5.], Modifiers::CONTROL);
                    pointer(&mut e, PointerPhase::Up, [0., 5.], Modifiers::CONTROL);
                }
                let before = e.session().document.clone();
                let corners = e.transform_placement().unwrap().corners();
                e.nudge(&Key::ArrowRight, Modifiers::empty()).unwrap();
                e.nudge(&Key::ArrowDown, Modifiers::SHIFT).unwrap();
                assert!(e.transform_edit.is_some());
                assert!(e.session().undo_label().is_none());
                assert_eq!(
                    e.transform_placement().unwrap().corners(),
                    corners.map(|p| [p[0] + 1., p[1] + 10.])
                );
                e.nudge(&Key::ArrowLeft, Modifiers::empty()).unwrap();
                e.nudge(&Key::ArrowUp, Modifiers::SHIFT).unwrap();
                assert_eq!(e.session().document, before);
                e.finish_toolbar_transform(apply).unwrap();
                if apply {
                    assert_eq!(
                        e.session().undo_label(),
                        Some(match (mask, perspective) {
                            (true, true) => "Distort Layer Mask",
                            (true, false) => "Transform Layer Mask",
                            (false, true) => "Distort",
                            (false, false) => "Transform Layer",
                        })
                    );
                    e.session_mut().undo();
                }
                assert_eq!(e.session().document, doc);
                assert!(e.session().undo_label().is_none());
            }
        }
    }
}

#[test]
fn perspective_keeps_flipped_source_through_repeated_drags_and_one_undo() {
    for mask in [false, true] {
        for group in [false, true] {
            if mask && group {
                continue;
            }
            let mut e = Editor::with_test_document();
            let mut doc = patterned_document();
            if mask {
                compositor::edits::add_mask(&mut doc, false).unwrap();
                let layer = &mut doc.layers[0];
                let mask = layer.mask.as_mut().unwrap();
                mask.linked = false;
                mask.placement = Some(layer.transform);
            }
            if group {
                compositor::layer_ops::group(&mut doc).unwrap();
            }
            let original = doc.clone();
            e.tabs = vec![Session::new(doc, None).into()];
            e.tools.mask_target = mask;
            e.change_header_transform(|t| {
                t.flip_x = true;
                t.sampling = compositor::geometry::Sampling::Nearest;
            })
            .unwrap();
            e.header_transform_input(0, "10").unwrap();
            let affine = e.session().document.clone();
            let bounds = e.header_transform_bounds().unwrap();
            pointer(&mut e, PointerPhase::Down, [10., 0.], Modifiers::CONTROL);
            pointer(&mut e, PointerPhase::Up, [20., 10.], Modifiers::CONTROL);
            assert!(e.transform_edit.is_some());
            assert!(!e.can_edit_transform_numbers());
            assert!(e.session().undo_label().is_none());
            let corner = e.transform_placement().unwrap().corners()[2];
            pointer(&mut e, PointerPhase::Down, corner, Modifiers::empty());
            pointer(&mut e, PointerPhase::Up, [100., 70.], Modifiers::empty());
            let corners = e.transform_placement().unwrap().corners();
            let mut expected = affine;
            compositor::distort::apply(&mut expected, bounds, corners, mask).unwrap();
            assert_eq!(e.session().document, expected);
            e.finish_toolbar_transform(true).unwrap();
            assert_eq!(
                e.session().undo_label(),
                Some(if group {
                    "Distort Layers"
                } else if mask {
                    "Distort Layer Mask"
                } else {
                    "Distort"
                })
            );
            e.session_mut().undo();
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        }
    }
}

#[test]
fn cancelled_drag_restores_numeric_draft_and_invalid_quad_keeps_last_preview() {
    let mut e = Editor::with_test_document();
    let original = patterned_document();
    e.tabs = vec![Session::new(original.clone(), None).into()];
    e.header_transform_input(0, "10.25").unwrap();
    e.tools.show_transform_controls = false;
    let draft = e.session().document.clone();
    for modifiers in [Modifiers::empty(), Modifiers::CONTROL] {
        pointer(&mut e, PointerPhase::Down, [10.25, 0.], modifiers);
        pointer(&mut e, PointerPhase::Move, [20.25, 10.], modifiers);
        assert_ne!(e.session().document, draft);
        let preview = e.session().document.clone();
        if modifiers == Modifiers::CONTROL {
            pointer(&mut e, PointerPhase::Move, [200., 150.], modifiers);
            assert_eq!(e.session().document, preview);
        }
        pointer(&mut e, PointerPhase::Cancel, [20.25, 10.], modifiers);
        assert_eq!(e.session().document, draft);
        assert!(e.transform_edit.is_some());
        assert!(e.can_edit_transform_numbers());
        assert!(e.session().undo_label().is_none());
    }
    e.finish_toolbar_transform(false).unwrap();
    assert_eq!(e.session().document, original);
}

#[test]
fn first_perspective_drag_waits_for_apply_and_pan_preserves_draft() {
    let mut e = Editor::with_test_document();
    let original = patterned_document();
    e.tabs = vec![Session::new(original.clone(), None).into()];
    pointer(&mut e, PointerPhase::Down, [0., 0.], Modifiers::CONTROL);
    pointer(&mut e, PointerPhase::Up, [10., 10.], Modifiers::CONTROL);
    assert!(e.transform_edit.is_some());
    assert!(e.session().undo_label().is_none());
    let preview = e.session().document.clone();
    e.space_pan = true;
    pointer(&mut e, PointerPhase::Down, [40., 40.], Modifiers::empty());
    pointer(&mut e, PointerPhase::Cancel, [50., 50.], Modifiers::empty());
    assert_eq!(e.session().document, preview);
    assert!(e.transform_edit.is_some());
    e.finish_toolbar_transform(false).unwrap();
    assert_eq!(e.session().document, original);
}
