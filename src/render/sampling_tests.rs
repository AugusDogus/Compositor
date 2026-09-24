use super::*;
use crate::{
    adjustment::{Adjustment, Kind},
    document::Mask,
};
use std::sync::Arc;

#[test]
fn cursor_sampling_matches_full_surfaces_with_chained_masked_blurs() {
    let mut doc = Document::new(40, 32).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(40, 32, |x, y| {
            Rgba([
                (x * 6) as u8,
                (y * 7) as u8,
                70,
                if x < 30 { 160 } else { 0 },
            ])
        }))));
    for kind in [Kind::GaussianBlur, Kind::MotionBlur] {
        let mut adjustment = Adjustment::new(kind);
        adjustment.blur_radius = Some(2.);
        adjustment.motion_distance = Some(5.);
        adjustment.motion_angle = Some(25.);
        let mut layer = Layer::blank("Blur", 40, 32);
        layer.content = LayerContent::Adjustment(Box::new(adjustment));
        layer.opacity = 0.7;
        layer.mask = Some(Mask {
            pixels: Arc::new(image::GrayImage::from_fn(40, 32, |_, y| {
                image::Luma([if y < 20 { 255 } else { 80 }])
            })),
            enabled: true,
            linked: true,
            placement: None,
        });
        doc.add(layer).unwrap();
    }
    let full = Sampler::new(&doc).unwrap();
    for point in [[0.5, 0.5], [15.5, 16.5], [29.2, 19.8], [39.5, 31.5]] {
        let actual = sample(&doc, point).unwrap();
        let expected = full.sample(point);
        for (a, b) in actual.into_iter().zip(expected) {
            assert!(
                (a - b).abs() <= 1. / 255.,
                "{point:?}: {actual:?} vs {expected:?}"
            );
        }
    }
}

#[test]
fn cursor_sampling_does_not_materialize_a_large_sparse_canvas() {
    let mut doc = Document::new(30_000, 30_000).unwrap();
    let mut blur = Layer::blank("Blur", doc.width, doc.height);
    blur.content = LayerContent::Adjustment(Box::new(Adjustment::new(Kind::GaussianBlur)));
    doc.add(blur).unwrap();
    let point = [15_000.5, 15_000.5];
    assert_eq!(sample(&doc, point).unwrap(), [0.; 4]);
    let sampler = Sampler::for_region(&doc, [2, 2], point.map(|v| (v - 0.5).floor())).unwrap();
    let surface = sampler.state.surfaces.values().next().unwrap();
    assert_eq!(surface.image.dimensions(), (66, 66));
}
