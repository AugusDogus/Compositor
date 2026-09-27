use crate::{
    document::{Document, LayerContent, Mask},
    filters::{self, Filter},
    selection::Selection,
    selective_color::{Mode, Range, SelectiveColor},
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

#[test]
fn selective_color_filter_preserves_selection_alpha_mask_and_placement() {
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
    filters::apply(&mut doc, Filter::SelectiveColor(Default::default()), false).unwrap();
    assert_eq!(doc, original);
    for value in [100.1, f32::NAN, f32::INFINITY] {
        let mut settings = SelectiveColor::default();
        settings.adjustments[8][3] = value;
        assert!(filters::apply(&mut doc, Filter::SelectiveColor(settings), false).is_err());
        assert_eq!(doc, original);
    }
    let mut settings = SelectiveColor {
        mode: Mode::Absolute,
        ..Default::default()
    };
    settings.adjustments[Range::Neutrals.index()] = [50., 0., 0., 0.];
    filters::apply(&mut doc, Filter::SelectiveColor(settings), false).unwrap();
    let pixels = doc.layers[0].raster().unwrap();
    assert_eq!(pixels[(0, 0)], Rgba([128, 128, 128, 0]));
    assert_eq!(pixels[(1, 0)], Rgba([64, 128, 128, 73]));
    assert_eq!(pixels[(2, 0)], Rgba([128, 128, 128, 73]));
    assert_eq!(pixels[(3, 0)], Rgba([1, 128, 128, 73]));
    assert_eq!(doc.layers[0].mask, original.layers[0].mask);
    assert_eq!(doc.layers[0].transform, original.layers[0].transform);
    assert_eq!(doc.selection, original.selection);
}
