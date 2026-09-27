use super::*;
use image::Rgba;

#[test]
fn photo_filter_equation_and_luminosity_preservation() {
    let mut settings = PhotoFilter {
        color: [1., 0.5, 0.],
        density: 50.,
        preserve_luminosity: false,
    };
    assert_eq!(settings.pixel([100, 100, 100, 127]), [100, 75, 50, 127]);
    settings.preserve_luminosity = true;
    assert_eq!(settings.pixel([100, 100, 100, 127]), [126, 94, 63, 127]);
    let result = settings.pixel([100, 100, 100, 127]);
    let luminosity =
        f64::from(result[0]) * 0.299 + f64::from(result[1]) * 0.587 + f64::from(result[2]) * 0.114;
    assert!((luminosity - 100.).abs() < 0.5);
    assert_eq!(settings.pixel([11, 22, 33, 0]), [11, 22, 33, 0]);
    assert_eq!(settings.pixel([0, 0, 0, 255]), [0, 0, 0, 255]);
    settings.color = [0.; 3];
    settings.density = 100.;
    assert_eq!(settings.pixel([255, 255, 255, 255]), [0, 0, 0, 255]);
}

#[test]
fn photo_filter_zero_density_white_color_and_invalid_values_are_atomic() {
    use crate::{
        document::{Document, LayerContent},
        filters::{self, Filter},
    };
    let mut doc = Document::new(3, 2).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(std::sync::Arc::new(RgbaImage::from_pixel(
        3,
        2,
        Rgba([79, 132, 207, 127]),
    ))));
    let original = doc.clone();
    for settings in [
        PhotoFilter {
            density: 0.,
            ..Default::default()
        },
        PhotoFilter {
            color: [1.; 3],
            ..Default::default()
        },
    ] {
        filters::apply(&mut doc, Filter::PhotoFilter(settings), false).unwrap();
        assert_eq!(doc, original);
    }
    for settings in [
        PhotoFilter {
            density: f64::NAN,
            ..Default::default()
        },
        PhotoFilter {
            density: 100.1,
            ..Default::default()
        },
        PhotoFilter {
            color: [f64::INFINITY, 0., 0.],
            ..Default::default()
        },
        PhotoFilter {
            color: [-0.1, 0., 0.],
            ..Default::default()
        },
    ] {
        assert!(filters::apply(&mut doc, Filter::PhotoFilter(settings), false).is_err());
        assert_eq!(doc, original);
    }
}

#[test]
fn photo_filter_selection_preserves_hidden_pixels_masks_and_placement() {
    use crate::{
        document::{Document, LayerContent, Mask},
        filters::{self, Filter},
        selection::Selection,
    };
    use image::{GrayImage, Luma};
    use std::sync::Arc;
    let mut doc = Document::new(4, 1).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(4, 1, |x, _| {
            Rgba([100, 100, 100, if x == 0 { 0 } else { 128 }])
        }))));
    doc.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(4, 1, Luma([71]))),
        enabled: true,
        linked: false,
        placement: Some(crate::geometry::Transform::new(4, 1)),
    });
    doc.selection = Some(Selection::from_mask(GrayImage::from_fn(4, 1, |x, _| {
        Luma([if x == 3 { 0 } else { 128 }])
    })));
    let original = doc.clone();
    filters::apply(
        &mut doc,
        Filter::PhotoFilter(PhotoFilter {
            color: [1., 0.5, 0.],
            density: 50.,
            preserve_luminosity: false,
        }),
        false,
    )
    .unwrap();
    let output = doc.layers[0].raster().unwrap();
    assert_eq!(output[(0, 0)], Rgba([100, 100, 100, 0]));
    assert_eq!(output[(1, 0)], Rgba([100, 87, 75, 128]));
    assert_eq!(output[(3, 0)], Rgba([100, 100, 100, 128]));
    assert_eq!(doc.layers[0].mask, original.layers[0].mask);
    assert_eq!(doc.layers[0].transform, original.layers[0].transform);
}
