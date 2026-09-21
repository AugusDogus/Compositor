use super::*;
use image::Luma;
fn settings(target: Target, mode: SelectionMode) -> Settings {
    Settings {
        target,
        sample_all: true,
        antialiased: true,
        mode,
    }
}
fn object_mask(left: bool) -> GrayImage {
    GrayImage::from_fn(16, 12, |x, y| {
        let inside = if left {
            (2..8).contains(&x)
        } else {
            (8..14).contains(&x)
        };
        Luma([if inside && (2..10).contains(&y) {
            255
        } else {
            0
        }])
    })
}
#[test]
fn supplied_object_mask_separates_touching_objects_and_preserves_detached_parts() {
    use crate::document::LayerContent;
    use image::{Rgba, RgbaImage};
    let mut doc = Document::new(16, 12).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(16, 12, |x, y| {
            if (x, y) == (3, 0) {
                Rgba([255, 0, 0, 200])
            } else if !(2..14).contains(&x) || !(2..10).contains(&y) {
                Rgba([0; 4])
            } else if x < 8 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 0, 255, 255])
            }
        }))));
    let mut mask = object_mask(true);
    mask[(3, 5)] = Luma([0]);
    mask[(3, 0)] = Luma([200]);
    let layers = doc.layers.clone();
    select_from_mask(
        &mut doc,
        &mask,
        settings(Target::Object([2.5, 4.5]), SelectionMode::Replace),
    )
    .unwrap();
    let selection = doc.selection.as_ref().unwrap();
    assert_eq!(selection.coverage([7.5, 4.5]), 1.);
    assert_eq!(selection.coverage([8.5, 4.5]), 0.);
    assert_eq!(selection.coverage([3.5, 5.5]), 0.);
    assert_eq!(selection.coverage([3.5, 0.5]), 200. / 255.);
    assert_eq!(doc.layers, layers);
}
#[test]
fn supplied_masks_support_add_subtract_and_empty_replace() {
    let mut doc = Document::new(16, 12).unwrap();
    let left = object_mask(true);
    let right = object_mask(false);
    let empty = GrayImage::new(16, 12);
    select_from_mask(
        &mut doc,
        &left,
        settings(Target::Object([3., 4.]), SelectionMode::Replace),
    )
    .unwrap();
    select_from_mask(
        &mut doc,
        &right,
        settings(Target::Object([11., 4.]), SelectionMode::Add),
    )
    .unwrap();
    for p in [[3.5, 4.5], [11.5, 4.5]] {
        assert_eq!(doc.selection.as_ref().unwrap().coverage(p), 1.);
    }
    select_from_mask(
        &mut doc,
        &left,
        settings(Target::Object([3., 4.]), SelectionMode::Subtract),
    )
    .unwrap();
    assert_eq!(doc.selection.as_ref().unwrap().coverage([3.5, 4.5]), 0.);
    assert_eq!(doc.selection.as_ref().unwrap().coverage([11.5, 4.5]), 1.);
    let before = doc.selection.clone();
    select_from_mask(
        &mut doc,
        &empty,
        settings(Target::Object([0., 0.]), SelectionMode::Add),
    )
    .unwrap();
    assert_eq!(doc.selection, before);
    select_from_mask(
        &mut doc,
        &empty,
        settings(Target::Object([0., 0.]), SelectionMode::Replace),
    )
    .unwrap();
    assert!(doc.selection.is_none());
}
#[test]
fn subject_selects_all_predicted_objects_and_invalid_inputs_preserve_document() {
    let mut doc = Document::new(16, 12).unwrap();
    let left = object_mask(true);
    let right = object_mask(false);
    let mask = GrayImage::from_fn(16, 12, |x, y| Luma([left[(x, y)][0].max(right[(x, y)][0])]));
    select_from_mask(
        &mut doc,
        &mask,
        settings(Target::Subject, SelectionMode::Replace),
    )
    .unwrap();
    for p in [[3.5, 4.5], [11.5, 4.5]] {
        assert_eq!(doc.selection.as_ref().unwrap().coverage(p), 1.);
    }
    let before = doc.clone();
    assert!(
        select_from_mask(
            &mut doc,
            &GrayImage::new(1, 1),
            settings(Target::Subject, SelectionMode::Replace)
        )
        .is_err()
    );
    assert_eq!(doc, before);
    for point in [
        [f64::NAN, 4.],
        [f64::INFINITY, 4.],
        [-1., 4.],
        [16., 4.],
        [4., 12.],
    ] {
        let settings = settings(Target::Object(point), SelectionMode::Replace);
        assert!(select_from_mask(&mut doc, &mask, settings).is_err());
        assert!(select(&mut doc, settings).is_err());
        assert_eq!(doc, before);
    }
}
#[test]
fn cancelling_selection_restores_previous_document_and_commit_is_undoable() {
    let doc = Document::new(16, 12).unwrap();
    let mut session = crate::session::Session::new(doc.clone(), None);
    session.begin("Object selection").unwrap();
    select_from_mask(
        &mut session.document,
        &object_mask(true),
        settings(Target::Object([3., 4.]), SelectionMode::Replace),
    )
    .unwrap();
    session.cancel();
    assert_eq!(session.document, doc);
    session.begin("Object selection").unwrap();
    select_from_mask(
        &mut session.document,
        &object_mask(false),
        settings(Target::Object([11., 4.]), SelectionMode::Replace),
    )
    .unwrap();
    session.commit().unwrap();
    session.undo();
    assert_eq!(session.document, doc);
}
#[test]
fn sampling_this_layer_keeps_transform_mask_and_excludes_other_layers() {
    use crate::document::{Layer, LayerContent};
    use image::{Rgba, RgbaImage};
    let mut doc = Document::new(4, 4).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        4,
        4,
        Rgba([255, 0, 0, 255]),
    ))));
    let mut top = Layer::blank("Blue", 2, 2);
    top.transform.origin = [1., 1.];
    top.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        2,
        2,
        Rgba([0, 0, 255, 255]),
    ))));
    doc.add(top).unwrap();
    let source = source_document(&doc, false).unwrap();
    let pixels = crate::render::render(&source, 4, 4).unwrap();
    assert_eq!(pixels[(0, 0)][3], 0);
    assert_eq!(pixels[(1, 1)], Rgba([0, 0, 255, 255]));
    let all = source_document(&doc, true).unwrap();
    assert_eq!(
        crate::render::render(&all, 4, 4).unwrap()[(0, 0)],
        Rgba([255, 0, 0, 255])
    );
}

#[test]
fn inferred_probabilities_and_source_alpha_are_not_thresholded_or_multiplied_again() {
    let mask = GrayImage::from_raw(9, 1, vec![0, 64, 200, 255, 32, 0, 100, 0, 0]).unwrap();
    let mut doc = Document::new(9, 1).unwrap();
    let mut settings = settings(Target::Object([3., 0.]), SelectionMode::Replace);
    select_from_mask(&mut doc, &mask, settings).unwrap();
    for (x, value) in mask.as_raw().iter().enumerate() {
        assert_eq!(
            doc.selection
                .as_ref()
                .unwrap()
                .coverage([x as f64 + 0.5, 0.5]),
            f64::from(*value) / 255.
        );
    }
    settings.antialiased = false;
    select_from_mask(&mut doc, &mask, settings).unwrap();
    assert_eq!(doc.selection.as_ref().unwrap().coverage([1.5, 0.5]), 0.);
    assert_eq!(doc.selection.as_ref().unwrap().coverage([2.5, 0.5]), 1.);
    settings.antialiased = true;
    select_from_mask(&mut doc, &GrayImage::from_pixel(9, 1, Luma([64])), settings).unwrap();
    assert_eq!(
        doc.selection.as_ref().unwrap().coverage([2.5, 0.5]),
        64. / 255.
    );
}

#[test]
fn oversized_sparse_canvas_is_rejected_before_inference_or_materialization() {
    let mut doc = Document::new(30_000, 30_000).unwrap();
    let before = doc.clone();
    let mut settings = settings(Target::Subject, SelectionMode::Replace);
    settings.sample_all = true;
    let error = select(&mut doc, settings).unwrap_err().to_string();
    assert!(error.contains("100 million pixels"));
    assert_eq!(doc, before);
}
#[test]
fn empty_and_disjoint_intersections_remove_selection() {
    for mask in [GrayImage::new(16, 12), object_mask(false)] {
        let mut doc = Document::new(16, 12).unwrap();
        select_from_mask(
            &mut doc,
            &object_mask(true),
            settings(Target::Object([3., 4.]), SelectionMode::Replace),
        )
        .unwrap();
        select_from_mask(
            &mut doc,
            &mask,
            settings(Target::Object([11., 4.]), SelectionMode::Intersect),
        )
        .unwrap();
        assert!(doc.selection.as_ref().is_none_or(|s| s.bounds().is_none()));
    }
}

#[test]
#[ignore = "Requires bundled GPU inference and COMPOSITOR_TEST_PHOTO"]
fn real_gpu_subject_selection_reuses_cached_foreground_mask() {
    let path = std::env::var_os("COMPOSITOR_TEST_PHOTO").expect("Set COMPOSITOR_TEST_PHOTO");
    let layer = crate::image_io::import(std::path::Path::new(&path)).unwrap();
    let pixels = layer.raster().unwrap();
    let mut doc = Document::new(pixels.width(), pixels.height()).unwrap();
    doc.layers.clear();
    doc.add(layer).unwrap();
    let settings = settings(Target::Subject, SelectionMode::Replace);
    select(&mut doc, settings).unwrap();
    assert!(doc.selection.as_ref().unwrap().bounds().is_some());
    let first = doc.selection.clone();
    select(&mut doc, settings).unwrap();
    assert_eq!(doc.selection, first);
}

#[test]
fn object_boxes_allow_reversed_canvas_edges_and_reject_invalid_bounds() {
    let mut doc = Document::new(16, 12).unwrap();
    let mask = object_mask(true);
    select_from_mask(
        &mut doc,
        &mask,
        settings(
            Target::ObjectBox {
                start: [16., 12.],
                end: [0., 0.],
            },
            SelectionMode::Replace,
        ),
    )
    .unwrap();
    assert_eq!(doc.selection.as_ref().unwrap().coverage([3.5, 4.5]), 1.);
    let before = doc.clone();
    for (start, end) in [
        ([0., 0.], [0., 12.]),
        ([0., 0.], [16., 0.]),
        ([-1., 0.], [10., 10.]),
        ([0., 0.], [17., 12.]),
        ([0., f64::NAN], [16., 12.]),
    ] {
        let settings = settings(Target::ObjectBox { start, end }, SelectionMode::Replace);
        assert!(select_from_mask(&mut doc, &mask, settings).is_err());
        assert!(select(&mut doc, settings).is_err());
        assert_eq!(doc, before);
    }
}
