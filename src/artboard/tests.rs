use super::*;
use crate::{
    document::Mask,
    guides::{Axis, Guide},
    vector_path::{Anchor, BezierPath, Closure, SavedPath},
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

fn frame(x: f64, y: f64, w: u32, h: u32) -> Transform {
    Transform {
        origin: [x, y],
        ..Transform::new(w, h)
    }
}
fn child(document: &mut Document, board: Uuid, name: &str) -> Uuid {
    let mut layer = Layer::blank(name, 2, 2);
    layer.parent = Some(board);
    layer.transform.origin = [4., 5.];
    layer.content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(2, 2, Rgba([255; 4])))));
    let id = layer.id;
    document.add(layer).unwrap();
    id
}

#[test]
fn negative_frames_grow_canvas_and_shift_every_document_coordinate_together() {
    let mut doc = Document::new(20, 20).unwrap();
    doc.guides.push(Guide {
        id: Uuid::new_v4(),
        axis: Axis::Vertical,
        position: 3.,
    });
    doc.paths.push(
        SavedPath::new(
            "Path",
            BezierPath {
                anchors: vec![Anchor::corner([2., 3.])],
                closure: Closure::Open,
            },
        )
        .unwrap(),
    );
    let original_layer = doc.layers[0].id;
    let board = create(&mut doc, "Board", frame(-5., -7., 10, 10), [255; 4]).unwrap();
    assert_eq!((doc.width, doc.height), (25, 27));
    assert_eq!(doc.layer(board).unwrap().transform.origin, [0., 0.]);
    assert_eq!(
        doc.layer(original_layer).unwrap().transform.origin,
        [5., 7.]
    );
    assert_eq!(doc.guides[0].position, 8.);
    assert_eq!(doc.paths[0].geometry.anchors[0].point, [7., 10.]);
    assert_eq!(owner(&doc, board), Some(board));
    doc.validate().unwrap();
}

#[test]
fn moving_board_carries_hidden_blank_adjustment_and_mask_placements_once() {
    let mut doc = Document::new(40, 40).unwrap();
    let board = create(&mut doc, "Board", frame(2., 3., 20, 20), [0; 4]).unwrap();
    let painted = child(&mut doc, board, "Painted");
    let hidden = child(&mut doc, board, "Hidden");
    let mut blank = Layer::blank("Blank", 2, 2);
    blank.parent = Some(board);
    let blank_id = blank.id;
    doc.add(blank).unwrap();
    let mut adjust = Layer::blank("Adjustment", 2, 2);
    adjust.parent = Some(board);
    adjust.content = LayerContent::Adjustment(Box::new(crate::adjustment::Adjustment::new(
        crate::adjustment::Kind::Invert,
    )));
    let adjust_id = adjust.id;
    doc.add(adjust).unwrap();
    let layer = doc.layers.iter_mut().find(|l| l.id == hidden).unwrap();
    layer.visible = false;
    layer.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([128]))),
        enabled: true,
        linked: false,
        placement: Some(frame(7., 8., 2, 2)),
    });
    let original = doc.clone();
    translate(&mut doc, board, [5., 6.]).unwrap();
    for id in [board, painted, hidden, blank_id, adjust_id] {
        let old = original.layer(id).unwrap();
        let new = doc.layer(id).unwrap();
        assert_eq!(
            new.transform.origin,
            [old.transform.origin[0] + 5., old.transform.origin[1] + 6.]
        );
        assert_eq!(owner(&doc, id), Some(board));
    }
    assert_eq!(
        doc.layer(hidden)
            .unwrap()
            .mask
            .as_ref()
            .unwrap()
            .placement
            .unwrap()
            .origin,
        [12., 14.]
    );
    assert_eq!(doc.layers[0], original.layers[0]);
}

#[test]
fn frame_resize_and_rename_preserve_descendant_pixels_and_positions() {
    let mut doc = Document::new(32, 32).unwrap();
    let board = create(&mut doc, "Board", frame(2., 3., 10, 10), [0; 4]).unwrap();
    let id = child(&mut doc, board, "Content");
    let original = doc.layer(id).unwrap().clone();
    update(
        &mut doc,
        board,
        "Renamed",
        frame(2., 3., 20, 15),
        [10, 20, 30, 128],
    )
    .unwrap();
    assert_eq!(doc.layer(id).unwrap(), &original);
    assert_eq!(doc.layer(board).unwrap().name, "Renamed");
    assert_eq!(
        doc.layer(board).unwrap().content,
        LayerContent::Artboard(Artboard {
            background: [10, 20, 30, 128]
        })
    );
    doc.select(board, false);
    let old = doc.layer(board).unwrap().transform;
    let new = frame(4., 5., 8, 9);
    assert_eq!(crate::transform::selection_bounds(&doc, false), Some(old));
    crate::transform::apply(&mut doc, old, new, false).unwrap();
    assert_eq!(doc.layer(board).unwrap().transform, new);
    assert_eq!(
        doc.layer(id).unwrap().transform.origin,
        original.transform.origin
    );
    assert_eq!(
        doc.layer(id).unwrap().transform.size,
        original.transform.size
    );
}

#[test]
fn invalid_frames_growth_and_cross_boundary_clips_fail_atomically() {
    let mut doc = Document::new(20, 20).unwrap();
    let board = create(&mut doc, "Board", frame(0., 0., 10, 10), [255; 4]).unwrap();
    let original = doc.clone();
    for invalid in [
        frame(29_999., 0., 2, 2),
        Transform {
            rotation: 30.,
            ..frame(0., 0., 2, 2)
        },
        Transform {
            flip_x: true,
            ..frame(0., 0., 2, 2)
        },
        frame(f64::NAN, 0., 2, 2),
    ] {
        assert!(update(&mut doc, board, "Changed", invalid, [0; 4]).is_err());
        assert_eq!(doc, original);
    }
    let outside = doc.layers[0].id;
    let inside = child(&mut doc, board, "Inside");
    doc.layers
        .iter_mut()
        .find(|layer| layer.id == inside)
        .unwrap()
        .clip_source = Some(outside);
    assert!(doc.validate().is_err());
}

#[test]
fn from_selected_layers_keeps_complete_clipping_stack_and_rejects_partial_transfer() {
    let mut doc = Document::new(32, 32).unwrap();
    crate::edits::fill(&mut doc, [255; 4], false, false).unwrap();
    let base = doc.layers[0].id;
    let mut clip = Layer::blank("Clip", 2, 2);
    clip.content = LayerContent::Raster(Some(Arc::new(RgbaImage::new(2, 2))));
    clip.clip_source = Some(base);
    let clip_id = clip.id;
    doc.add(clip).unwrap();
    doc.select(base, false);
    let original = doc.clone();
    assert!(from_selection(&mut doc, "Partial", [0; 4]).is_err());
    assert_eq!(doc, original);
    doc.select(clip_id, true);
    let board = from_selection(&mut doc, "Complete", [0; 4]).unwrap();
    assert_eq!(owner(&doc, base), Some(board));
    assert_eq!(owner(&doc, clip_id), Some(board));
    assert_eq!(doc.layer(clip_id).unwrap().clip_source, Some(base));
    doc.validate().unwrap();
}

#[test]
fn nesting_and_ungrouping_reject_without_losing_frame_or_background() {
    let mut doc = Document::new(32, 32).unwrap();
    let board = create(&mut doc, "Board", frame(2., 3., 10, 10), [255; 4]).unwrap();
    let mut group = Layer::blank("Group", 32, 32);
    group.content = LayerContent::Group;
    let group_id = group.id;
    doc.add(group).unwrap();
    doc.select(board, false);
    let original = doc.clone();
    assert!(
        crate::layer_ops::place(
            &mut doc,
            board,
            Some(group_id),
            crate::layer_ops::Position::Top
        )
        .is_err()
    );
    assert_eq!(doc, original);
    assert!(crate::layer_ops::group(&mut doc).is_err());
    assert_eq!(doc, original);
    let mut session = crate::session::Session::new(doc, None);
    assert!(session.ungroup().is_err());
    assert_eq!(session.document, original);
}

#[test]
fn duplication_and_clipboard_paste_keep_boards_at_root_with_remapped_children() {
    let mut doc = Document::new(32, 32).unwrap();
    let board = create(&mut doc, "Board", frame(2., 3., 10, 10), [80, 90, 100, 255]).unwrap();
    child(&mut doc, board, "Content");
    doc.select(board, false);
    crate::layer_ops::duplicate_active(&mut doc).unwrap();
    let duplicate = doc.active.unwrap();
    assert_ne!(duplicate, board);
    assert_eq!(doc.descendants(duplicate).len(), 2);
    assert!(doc.layer(duplicate).unwrap().parent.is_none());
    let clipboard = crate::layer_clipboard::Layers::capture(&doc).unwrap();
    let mut target = Document::new(40, 40).unwrap();
    let target_board = create(&mut target, "Target", frame(0., 0., 20, 20), [0; 4]).unwrap();
    child(&mut target, target_board, "Active child");
    clipboard.paste(&mut target).unwrap();
    let pasted = target.active.unwrap();
    assert!(target.layer(pasted).unwrap().is_artboard());
    assert!(target.layer(pasted).unwrap().parent.is_none());
    assert_eq!(target.descendants(pasted).len(), 2);
    target.validate().unwrap();
}

#[test]
fn canvas_rotations_mirrors_and_image_resize_keep_axis_aligned_frames_and_mask_coverage() {
    let mut doc = Document::new(40, 30).unwrap();
    let board = create(&mut doc, "Board", frame(2., 3., 10, 8), [255; 4]).unwrap();
    doc.layers
        .iter_mut()
        .find(|layer| layer.id == board)
        .unwrap()
        .mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_fn(2, 1, |x, _| {
            Luma([if x == 0 { 0 } else { 255 }])
        })),
        enabled: true,
        linked: true,
        placement: None,
    });
    let before = crate::render::render(&doc, 40, 30).unwrap();
    crate::canvas_rotation::rotate(&mut doc, crate::canvas_rotation::QuarterTurn::Clockwise)
        .unwrap();
    assert_eq!(doc.layer(board).unwrap().transform, frame(19., 2., 8, 10));
    assert_eq!(
        crate::render::render(&doc, 30, 40).unwrap(),
        image::imageops::rotate90(&before)
    );
    crate::canvas_rotation::rotate(
        &mut doc,
        crate::canvas_rotation::QuarterTurn::CounterClockwise,
    )
    .unwrap();
    crate::edits::flip_canvas(&mut doc, true).unwrap();
    assert_eq!(doc.layer(board).unwrap().transform, frame(28., 3., 10, 8));
    assert_eq!(
        crate::render::render(&doc, 40, 30).unwrap(),
        image::imageops::flip_horizontal(&before)
    );
    crate::image_resize::resize(&mut doc, 80, 90, 72., crate::geometry::Sampling::Nearest).unwrap();
    assert_eq!(doc.layer(board).unwrap().transform.origin, [56., 9.]);
    assert_eq!(doc.layer(board).unwrap().transform.size, [20., 24.]);
    doc.validate().unwrap();
}

#[test]
fn background_only_artboard_merges_without_losing_its_picture() {
    let mut doc = Document::new(30_000, 30_000).unwrap();
    let board = create(&mut doc, "Board", frame(20., 30., 3, 2), [20, 40, 60, 255]).unwrap();
    doc.select(board, false);
    crate::layer_ops::merge(&mut doc, false).unwrap();
    let layer = doc.active_layer().unwrap();
    assert_eq!(layer.transform.origin, [20., 30.]);
    assert_eq!(layer.raster().unwrap().dimensions(), (3, 2));
    assert!(
        layer
            .raster()
            .unwrap()
            .pixels()
            .all(|pixel| *pixel == Rgba([20, 40, 60, 255]))
    );
}

#[test]
fn resize_handle_keeps_contents_and_implicit_mask_fixed_with_selected_descendant() {
    let mut doc = Document::new(40, 40).unwrap();
    let board = create(&mut doc, "Board", frame(5., 6., 10, 10), [255; 4]).unwrap();
    let child = child(&mut doc, board, "Content");
    let original_child = doc.layer(child).unwrap().clone();
    doc.layers
        .iter_mut()
        .find(|layer| layer.id == board)
        .unwrap()
        .mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(2, 2, Luma([128]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    doc.select(board, false);
    doc.select(child, true);
    let old = frame(5., 6., 10, 10);
    assert_eq!(crate::transform::selection_bounds(&doc, false), Some(old));
    crate::transform::apply(&mut doc, old, frame(3., 4., 12, 12), false).unwrap();
    assert_eq!(doc.layer(child).unwrap(), &original_child);
    assert_eq!(
        doc.layer(board).unwrap().mask.as_ref().unwrap().placement,
        Some(old)
    );
}

#[test]
fn merging_selected_board_and_descendant_keeps_result_at_root() {
    let mut doc = Document::new(40, 40).unwrap();
    let board = create(&mut doc, "Board", frame(0., 0., 10, 10), [255; 4]).unwrap();
    let child = child(&mut doc, board, "Content");
    doc.select(board, false);
    doc.select(child, true);
    let expected = crate::render::render(&doc, 40, 40).unwrap();
    crate::layer_ops::merge(&mut doc, false).unwrap();
    doc.validate().unwrap();
    assert!(doc.active_layer().unwrap().parent.is_none());
    assert_eq!(crate::render::render(&doc, 40, 40).unwrap(), expected);
}

#[test]
fn project_transfer_grows_around_board_and_rejects_oversized_growth_atomically() {
    let mut source = Document::new(40, 40).unwrap();
    let board = create(&mut source, "Board", frame(5., 6., 30, 20), [255; 4]).unwrap();
    child(&mut source, board, "Content");
    source.select(board, false);
    let mut target = Document::new(10, 10).unwrap();
    let old = target.layers[0].id;
    crate::layer_ops::copy_to_project(&source, &mut target, board, [5., 5.]).unwrap();
    assert_eq!((target.width, target.height), (30, 20));
    assert_eq!(
        target.active_layer().unwrap().transform,
        frame(0., 0., 30, 20)
    );
    assert_eq!(target.layer(old).unwrap().transform.origin, [10., 5.]);
    target.validate().unwrap();
    let before = target.clone();
    assert!(
        crate::layer_ops::copy_to_project(&source, &mut target, board, [30_000., 30_000.]).is_err()
    );
    assert_eq!(target, before);
}

#[test]
fn dialog_resize_freezes_mask_then_translates_it_with_requested_move() {
    let mut doc = Document::new(40, 40).unwrap();
    let board = create(&mut doc, "Board", frame(2., 3., 10, 10), [255; 4]).unwrap();
    doc.active_layer_mut().unwrap().mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(2, 2, Luma([128]))),
        enabled: true,
        linked: false,
        placement: None,
    });
    update(&mut doc, board, "Board", frame(5., 7., 20, 20), [255; 4]).unwrap();
    let placement = frame(5., 7., 10, 10);
    assert_eq!(
        doc.active_layer().unwrap().mask.as_ref().unwrap().placement,
        Some(placement)
    );
    assert_eq!(
        crate::transform::selection_bounds(&doc, true),
        Some(placement)
    );
    let rotated = Transform {
        rotation: 45.,
        ..placement
    };
    crate::transform::apply(&mut doc, placement, rotated, true).unwrap();
    assert_eq!(
        doc.active_layer().unwrap().mask.as_ref().unwrap().placement,
        Some(rotated)
    );
    assert_eq!(doc.active_layer().unwrap().transform, frame(5., 7., 20, 20));
    doc.validate().unwrap();
}

#[test]
fn copying_artboard_into_folder_rejects_without_leaving_an_extra_copy() {
    let mut doc = Document::new(40, 40).unwrap();
    let board = create(&mut doc, "Board", frame(2., 3., 10, 10), [255; 4]).unwrap();
    let mut group = Layer::blank("Folder", 10, 10);
    group.content = LayerContent::Group;
    let parent = group.id;
    doc.add(group).unwrap();
    doc.select(board, false);
    let before = doc.clone();
    assert!(
        crate::layer_ops::duplicate_to(
            &mut doc,
            board,
            Some(parent),
            crate::layer_ops::Position::Top
        )
        .is_err()
    );
    assert_eq!(doc, before);
}
