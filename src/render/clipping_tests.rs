use super::*;
use crate::{
    adjustment::{Adjustment, Kind},
    edits, layer_ops,
};
use std::sync::Arc;

#[test]
fn copied_pixels_land_outside_existing_clipping_stacks() {
    for below in [false, true] {
        let mut doc = Document::new(1, 1).unwrap();
        edits::fill(&mut doc, [255, 0, 0, 128], false, false).unwrap();
        let base = doc.active.unwrap();
        let mut invert = Layer::blank("Invert", 1, 1);
        invert.content = LayerContent::Adjustment(Box::new(Adjustment::new(Kind::Invert)));
        invert.clip_source = Some(base);
        let clipped = invert.id;
        doc.add(invert).unwrap();
        doc.add(Layer::blank("Copy source", 1, 1)).unwrap();
        edits::fill(&mut doc, [0, 255, 0, 128], false, false).unwrap();
        let source = doc.active.unwrap();
        let position = if below {
            layer_ops::Position::Below(clipped)
        } else {
            layer_ops::Position::Above(base)
        };
        layer_ops::duplicate_to(&mut doc, source, None, position).unwrap();
        doc.layers
            .iter_mut()
            .find(|l| l.id == source)
            .unwrap()
            .visible = false;
        let expected = if below {
            Rgba([0, 255, 170, 192])
        } else {
            Rgba([0, 255, 85, 192])
        };
        assert_eq!(render(&doc, 1, 1).unwrap()[(0, 0)], expected);
    }
}

#[test]
fn duplicating_adjustments_from_different_bases_keeps_both_effective() {
    let mut doc = Document::new(2, 1).unwrap();
    for x in 0..2 {
        let mut base = Layer::blank("Base", 2, 1);
        base.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(2, 1, |i, _| {
            Rgba([255, 0, 0, if i == x { 128 } else { 0 }])
        }))));
        let mut invert = Layer::blank("Invert", 2, 1);
        invert.content = LayerContent::Adjustment(Box::new(Adjustment::new(Kind::Invert)));
        invert.clip_source = Some(base.id);
        doc.add(base).unwrap();
        doc.add(invert).unwrap();
    }
    doc.selected = doc
        .layers
        .iter()
        .filter(|l| matches!(l.content, LayerContent::Adjustment(_)))
        .map(|l| l.id)
        .collect();
    assert!(
        render(&doc, 2, 1)
            .unwrap()
            .pixels()
            .all(|p| *p == Rgba([0, 255, 255, 128]))
    );
    layer_ops::duplicate_selected(&mut doc).unwrap();
    assert!(
        render(&doc, 2, 1)
            .unwrap()
            .pixels()
            .all(|p| *p == Rgba([255, 0, 0, 128]))
    );
}

#[test]
fn detached_raster_masks_keep_their_compositing_order() {
    let mut doc = Document::new(1, 1).unwrap();
    edits::fill(&mut doc, [255, 0, 0, 128], false, false).unwrap();
    let base = doc.active.unwrap();
    doc.add(Layer::blank("Middle", 1, 1)).unwrap();
    edits::fill(&mut doc, [0, 255, 0, 255], false, false).unwrap();
    doc.add(Layer::blank("Clipped", 1, 1)).unwrap();
    edits::fill(&mut doc, [0, 0, 255, 255], false, false).unwrap();
    doc.active_layer_mut().unwrap().clip_source = Some(base);
    assert_eq!(
        render(&doc, 1, 1).unwrap()[(0, 0)],
        Rgba([0, 127, 128, 255])
    );
}

fn blur_documents() -> (Document, Document) {
    let mut doc = Document::new(24, 16).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(24, 16, |x, y| {
            Rgba([if x < 12 { 255 } else { 0 }, (y * 12) as u8, 80, 180])
        }))));
    let base = doc.active.unwrap();
    for (kind, clip_source) in [(Kind::GaussianBlur, Some(base)), (Kind::MotionBlur, None)] {
        let mut layer = Layer::blank("Blur", 24, 16);
        let mut a = Adjustment::new(kind);
        a.blur_radius = Some(2.);
        a.motion_distance = Some(5.);
        layer.content = LayerContent::Adjustment(Box::new(a));
        layer.clip_source = clip_source;
        doc.add(layer).unwrap();
    }
    let mut detached = doc.clone();
    detached.layers.swap(1, 2);
    (doc, detached)
}

#[test]
fn detached_spatial_adjustments_are_prepared_in_compositing_order() {
    let (doc, detached) = blur_documents();
    assert_eq!(
        render(&detached, 24, 16).unwrap(),
        render(&doc, 24, 16).unwrap()
    );
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn gpu_detached_spatial_adjustments_match_contiguous_stack() {
    initialize_gpu().unwrap();
    let (doc, detached) = blur_documents();
    let mut cache = DownsampleCache::default();
    let expected = region_accelerated(&doc, 24, 16, [0., 0.], [1., 1.], &mut cache).unwrap();
    let actual = region_accelerated(&detached, 24, 16, [0., 0.], [1., 1.], &mut cache).unwrap();
    assert_eq!(actual, expected);
    let cpu = render(&detached, 24, 16).unwrap();
    assert!(
        actual
            .as_raw()
            .iter()
            .zip(cpu.as_raw())
            .all(|(a, b)| a.abs_diff(*b) <= 3)
    );
}
