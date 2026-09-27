use crate::{
    document::{Document, LayerContent, Mask},
    filters::{self, Filter},
    selection::Selection,
    threshold::Threshold,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

#[test]
fn threshold_filter_preserves_selection_alpha_mask_and_placement() {
    let mut doc = Document::new(4, 1).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(4, 1, |x, _| {
            Rgba([128, 128, 128, if x == 0 { 0 } else { 73 }])
        }))));
    doc.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(4, 1, Luma([91]))),
        enabled: true,
        linked: false,
        placement: Some(crate::geometry::Transform::new(4, 1)),
    });
    doc.selection = Some(Selection::from_mask(GrayImage::from_fn(4, 1, |x, _| {
        Luma([match x {
            2 => 0,
            3 => 255,
            _ => 128,
        }])
    })));
    let original = doc.clone();
    filters::apply(&mut doc, Filter::Threshold(Threshold { level: 129 }), false).unwrap();
    let pixels = doc.layers[0].raster().unwrap();
    assert_eq!(pixels[(0, 0)], Rgba([128, 128, 128, 0]));
    assert_eq!(pixels[(1, 0)], Rgba([64, 64, 64, 73]));
    assert_eq!(pixels[(2, 0)], Rgba([128, 128, 128, 73]));
    assert_eq!(pixels[(3, 0)], Rgba([0, 0, 0, 73]));
    assert_eq!(doc.layers[0].mask, original.layers[0].mask);
    assert_eq!(doc.layers[0].transform, original.layers[0].transform);
    assert_eq!(doc.selection, original.selection);
}
