use super::*;
use crate::{
    document::LayerContent,
    vector_path::{Anchor, Closure, SavedPath},
};
use image::Rgba;

fn document(points: &[[f64; 2]], closure: Closure) -> (Document, Uuid) {
    let mut document = Document::new(32, 32).unwrap();
    let path = SavedPath::new(
        "Working path",
        BezierPath {
            anchors: points.iter().copied().map(Anchor::corner).collect(),
            closure,
        },
    )
    .unwrap();
    let id = path.id;
    document.paths.push(path);
    (document, id)
}
fn square() -> (Document, Uuid) {
    document(
        &[[4., 4.], [20., 4.], [20., 20.], [4., 20.]],
        Closure::Closed,
    )
}
fn brush() -> Brush {
    Brush {
        diameter: 2.,
        opacity: 0.5,
        color: [255, 0, 0, 255],
        ..Brush::default()
    }
}

#[test]
fn selection_supports_replace_add_subtract_intersect_without_changing_path() {
    let (mut doc, id) = square();
    let paths = doc.paths.clone();
    select(&mut doc, id, SelectionMode::Subtract, false).unwrap();
    assert!(doc.selection.is_none());
    select(&mut doc, id, SelectionMode::Replace, false).unwrap();
    assert_eq!(doc.selection.as_ref().unwrap().coverage([10.5, 10.5]), 1.);
    assert_eq!(doc.selection.as_ref().unwrap().coverage([2.5, 2.5]), 0.);
    let left = Selection::rectangle(32, 32, [0., 0.], [10., 32.], false);
    doc.selection = Some(left.clone());
    select(&mut doc, id, SelectionMode::Add, false).unwrap();
    assert_eq!(doc.selection.as_ref().unwrap().coverage([2.5, 2.5]), 1.);
    assert_eq!(doc.selection.as_ref().unwrap().coverage([15.5, 10.5]), 1.);
    doc.selection = Some(left.clone());
    select(&mut doc, id, SelectionMode::Subtract, false).unwrap();
    assert_eq!(doc.selection.as_ref().unwrap().coverage([2.5, 2.5]), 1.);
    assert_eq!(doc.selection.as_ref().unwrap().coverage([5.5, 10.5]), 0.);
    doc.selection = Some(left);
    select(&mut doc, id, SelectionMode::Intersect, false).unwrap();
    assert_eq!(doc.selection.as_ref().unwrap().coverage([2.5, 2.5]), 0.);
    assert_eq!(doc.selection.as_ref().unwrap().coverage([5.5, 10.5]), 1.);
    assert_eq!(doc.selection.as_ref().unwrap().coverage([15.5, 10.5]), 0.);
    assert_eq!(doc.paths, paths);
}

#[test]
fn fill_preserves_selection_and_obeys_its_intersection_with_the_path() {
    let (mut doc, id) = square();
    doc.selection = Some(Selection::rectangle(32, 32, [0., 0.], [10., 32.], false));
    let selection = doc.selection.clone();
    let paths = doc.paths.clone();
    Operation::Fill {
        color: [255, 0, 0, 128],
        mask: false,
        antialiased: false,
    }
    .apply(&mut doc, id)
    .unwrap();
    let pixels = doc.layers[0].raster().unwrap();
    assert_eq!(pixels[(5, 10)], Rgba([255, 0, 0, 128]));
    assert_eq!(pixels[(15, 10)][3], 0);
    assert_eq!(pixels[(2, 2)][3], 0);
    assert_eq!(doc.selection, selection);
    assert_eq!(doc.paths, paths);
    doc.selection = Some(Selection::rectangle(32, 32, [25., 25.], [30., 30.], false));
    let original = doc.clone();
    fill(&mut doc, id, [0, 255, 0, 255], false, false).unwrap();
    assert_eq!(doc, original);
}

#[test]
fn fill_and_stroke_masks_without_changing_layer_pixels() {
    let (mut doc, id) = square();
    crate::edits::fill(&mut doc, [20, 40, 60, 255], false, false).unwrap();
    crate::edits::add_mask(&mut doc, true).unwrap();
    let content = doc.layers[0].content.clone();
    fill(&mut doc, id, [128; 4], true, false).unwrap();
    assert_eq!(
        doc.layers[0].mask.as_ref().unwrap().pixels[(10, 10)][0],
        128
    );
    assert_eq!(doc.layers[0].mask.as_ref().unwrap().pixels[(1, 1)][0], 0);
    stroke(
        &mut doc,
        id,
        Brush {
            diameter: 2.,
            color: [255; 4],
            ..Brush::default()
        },
        sampled::Shape::Round,
        true,
    )
    .unwrap();
    assert!(doc.layers[0].mask.as_ref().unwrap().pixels[(4, 10)][0] > 128);
    assert_eq!(doc.layers[0].content, content);
}

#[test]
fn authored_corner_stroke_has_no_spline_overshoot_or_opacity_accumulation() {
    let (mut doc, id) = document(&[[4.5, 20.5], [20.5, 20.5], [20.5, 4.5]], Closure::Open);
    let paths = doc.paths.clone();
    Operation::Stroke {
        brush: brush(),
        shape: sampled::Shape::Round,
        mask: false,
    }
    .apply(&mut doc, id)
    .unwrap();
    let pixels = doc.layers[0].raster().unwrap();
    assert_eq!(pixels[(10, 20)], Rgba([255, 0, 0, 128]));
    assert_eq!(pixels[(20, 10)], Rgba([255, 0, 0, 128]));
    assert_eq!(pixels[(20, 20)], Rgba([255, 0, 0, 128]));
    assert_eq!(pixels[(14, 22)][3], 0);
    assert_eq!(pixels[(22, 14)][3], 0);
    assert_eq!(doc.paths, paths);
}

#[test]
fn stroke_closes_closed_paths_and_respects_selection() {
    let (mut doc, id) = square();
    doc.selection = Some(Selection::rectangle(32, 32, [0., 0.], [10., 32.], false));
    let selection = doc.selection.clone();
    stroke(&mut doc, id, brush(), sampled::Shape::Round, false).unwrap();
    let pixels = doc.layers[0].raster().unwrap();
    assert!(pixels[(4, 10)][3] > 0);
    assert_eq!(pixels[(19, 10)][3], 0);
    assert_eq!(doc.selection, selection);
}

#[test]
fn sampled_brush_is_used_for_path_replay() {
    let tip = sampled::read(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gbr/pixel.gbr"),
    )
    .unwrap();
    let shape = sampled::Shape::Sampled(sampled::Sampled::new(std::sync::Arc::new(tip)));
    let (mut doc, id) = document(&[[4.5, 10.5], [20.5, 10.5]], Closure::Open);
    stroke(&mut doc, id, brush(), shape, false).unwrap();
    assert!(
        doc.layers[0]
            .raster()
            .unwrap()
            .pixels()
            .any(|pixel| pixel[3] > 0)
    );
    assert!(
        doc.layers[0]
            .raster()
            .unwrap()
            .pixels()
            .all(|pixel| pixel[3] <= 128)
    );
}

#[test]
fn failures_leave_selection_paths_and_pixels_unchanged() {
    let (mut doc, id) = square();
    doc.layers[0].content = LayerContent::Group;
    let original = doc.clone();
    assert!(fill(&mut doc, id, [255; 4], false, true).is_err());
    assert_eq!(doc, original);
    assert!(stroke(&mut doc, id, brush(), sampled::Shape::Round, false).is_err());
    assert_eq!(doc, original);
    assert!(select(&mut doc, Uuid::new_v4(), SelectionMode::Replace, true).is_err());
    assert_eq!(doc, original);
    let (mut doc, _) = square();
    let original = doc.clone();
    assert!(
        brush::paint_polyline(
            &mut doc,
            &[[1., 1.], [f64::NAN, 3.]],
            brush(),
            sampled::Shape::Round,
            false
        )
        .is_err()
    );
    assert_eq!(doc, original);
}

#[test]
fn oversized_stroke_failure_rolls_back_the_first_successful_dab() {
    let mut doc = Document::new(30_000, 30_000).unwrap();
    let path = SavedPath::new(
        "Diagonal",
        BezierPath {
            anchors: vec![Anchor::corner([1., 1.]), Anchor::corner([29_999., 29_999.])],
            closure: Closure::Open,
        },
    )
    .unwrap();
    let id = path.id;
    doc.paths.push(path);
    let original = doc.clone();
    assert!(stroke(&mut doc, id, brush(), sampled::Shape::Round, false).is_err());
    assert_eq!(doc, original);
}
