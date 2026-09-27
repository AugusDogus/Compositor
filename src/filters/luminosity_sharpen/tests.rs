use super::*;
use image::Rgba;

#[test]
fn luminosity_edges_preserve_chroma_offsets_alpha_and_hidden_pixels() {
    let source = RgbaImage::from_fn(11, 7, |x, _| {
        if x == 0 {
            Rgba([250, 30, 180, 0])
        } else if x < 5 {
            Rgba([70, 90, 110, 37])
        } else {
            Rgba([130, 150, 170, 191])
        }
    });
    let result = cpu(
        &source,
        Settings {
            radius: 1.5,
            ..Default::default()
        },
    );
    assert!(result[(4, 3)][0] < source[(4, 3)][0]);
    assert!(result[(5, 3)][0] > source[(5, 3)][0]);
    for (a, b) in result.pixels().zip(source.pixels()) {
        assert_eq!(a[3], b[3]);
        if b[3] == 0 {
            assert_eq!(a, b);
        } else {
            assert_eq!(i16::from(a[1]) - i16::from(a[0]), 20);
            assert_eq!(i16::from(a[2]) - i16::from(a[1]), 20);
        }
    }
    let flat = RgbaImage::from_fn(11, 7, |x, _| {
        if x == 0 {
            Rgba([255, 0, 255, 0])
        } else {
            Rgba([40, 60, 80, 37])
        }
    });
    assert_eq!(
        cpu(
            &flat,
            Settings {
                amount: 500.,
                ..Default::default()
            }
        ),
        flat
    );
}
#[test]
fn reduce_noise_suppresses_small_differences_without_removing_strong_edges() {
    let source = RgbaImage::from_fn(15, 5, |x, _| {
        Rgba([if x < 7 { 100 + x as u8 % 2 * 2 } else { 170 }; 4])
    });
    let sharp = cpu(
        &source,
        Settings {
            amount: 300.,
            noise: 0.,
            ..Default::default()
        },
    );
    let quiet = cpu(
        &source,
        Settings {
            amount: 300.,
            noise: 100.,
            ..Default::default()
        },
    );
    let deviation =
        |image: &RgbaImage| (i16::from(image[(3, 2)][0]) - i16::from(source[(3, 2)][0])).abs();
    assert!(deviation(&quiet) < deviation(&sharp));
    assert_eq!(quiet[(7, 2)], sharp[(7, 2)]);
}
#[test]
fn validation_selection_and_mask_rejection_preserve_the_document() {
    use crate::{
        document::{Document, LayerContent},
        filters::{self, Filter},
        selection::Selection,
    };
    use std::sync::Arc;
    let mut doc = Document::new(12, 8).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(12, 8, |x, _| {
            Rgba([if x < 6 { 80 } else { 160 }, 100, 120, 37])
        }))));
    doc.selection = Some(Selection::rectangle(12, 8, [0., 0.], [6., 8.], false));
    let original = doc.clone();
    for settings in [
        Settings {
            amount: f64::NAN,
            ..Default::default()
        },
        Settings {
            radius: 0.,
            ..Default::default()
        },
        Settings {
            noise: 101.,
            ..Default::default()
        },
    ] {
        assert!(filters::apply(&mut doc, Filter::LuminositySharpen(settings), false).is_err());
        assert_eq!(doc, original);
    }
    assert!(
        filters::apply(
            &mut doc,
            Filter::LuminositySharpen(Default::default()),
            true
        )
        .is_err()
    );
    assert_eq!(doc, original);
    filters::apply(
        &mut doc,
        Filter::LuminositySharpen(Default::default()),
        false,
    )
    .unwrap();
    assert_ne!(
        doc.layers[0].raster().unwrap()[(5, 4)],
        original.layers[0].raster().unwrap()[(5, 4)]
    );
    assert_eq!(
        doc.layers[0].raster().unwrap()[(6, 4)],
        original.layers[0].raster().unwrap()[(6, 4)]
    );
    assert_eq!(doc.layers[0].transform, original.layers[0].transform);
}
