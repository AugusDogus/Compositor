use compositor::{
    document::{Document, LayerContent},
    filters::{self, Filter},
    selection::Selection,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

#[test]
fn selected_high_pass_preserves_untouched_hidden_rgb() {
    let mut doc = Document::new(4, 1).unwrap();
    let source = RgbaImage::from_fn(4, 1, |x, _| {
        Rgba([71, 83, 111, if x == 0 { 0 } else { 128 }])
    });
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(source.clone())));
    doc.selection = Some(Selection::from_mask(GrayImage::from_pixel(
        4,
        1,
        Luma([128]),
    )));
    filters::apply(&mut doc, Filter::HighPass { radius: 1. }, false).unwrap();
    let result = doc.layers[0].raster().unwrap();
    assert_ne!(result[(2, 0)], source[(2, 0)]);
    assert_eq!(result[(0, 0)], source[(0, 0)]);
}
