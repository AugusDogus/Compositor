use super::*;
use crate::{
    document::{Document, LayerContent, Mask},
    effects::LayerEffects,
    geometry::Sampling,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

fn effect(style: Style) -> LayerEffects {
    LayerEffects {
        bevel: Some(Box::new(Settings {
            style,
            size: 3.,
            angle: 0.,
            altitude: 0.,
            ..Default::default()
        })),
        ..Default::default()
    }
}

#[test]
fn inner_bevel_preserves_alpha_and_outer_only_shades_uncovered_pixels() {
    let image = RgbaImage::from_fn(21, 21, |x, y| {
        if (6..15).contains(&x) && (6..15).contains(&y) {
            Rgba([100, 120, 150, 255])
        } else {
            Rgba([0; 4])
        }
    });
    for style in [Style::Inner, Style::Outer, Style::Emboss] {
        let rendered = crate::effects::cpu::render(&image, &effect(style)).unwrap();
        assert_eq!(rendered[(10, 10)], image[(10, 10)]);
        if style == Style::Inner {
            assert!(
                rendered
                    .pixels()
                    .zip(image.pixels())
                    .all(|(a, b)| a[3] == b[3])
            );
            assert!(rendered[(14, 10)][0] > image[(14, 10)][0]);
            assert!(rendered[(6, 10)][0] < image[(6, 10)][0]);
        } else {
            assert!(rendered[(5, 10)][3] > 0);
            assert!(rendered[(15, 10)][3] > 0);
            if style == Style::Outer {
                assert!(
                    image
                        .enumerate_pixels()
                        .filter(|(_, _, p)| p[3] == 255)
                        .all(|(x, y, p)| rendered[(x, y)] == *p)
                );
            }
        }
    }
}

#[test]
fn fractional_coverage_outer_bevel_is_applied_once() {
    let mut settings = effect(Style::Outer);
    let size = settings.margin() * 2 + 7;
    let image = RgbaImage::from_fn(size, size, |x, y| {
        Rgba([
            80,
            90,
            100,
            if x == size / 2 && y == size / 2 {
                128
            } else {
                0
            },
        ])
    });
    // A one-pixel source has no centered gradient at its own center. Use an
    // adjacent partially covered pixel to exercise the exterior gate.
    let mut image = image;
    let (x, y) = (size / 2 + 1, size / 2);
    image[(x, y)][3] = 96;
    let shape: Vec<_> = image.pixels().map(|p| f32::from(p[3]) / 255.).collect();
    let surface = surface::Surface::new(
        &shape,
        size as usize,
        size as usize,
        settings.bevel.as_ref().unwrap(),
    )
    .unwrap();
    let shade = surface.sample(x as usize, y as usize);
    assert!(shade.highlight > 0.);
    let alpha = 96. / 255.;
    let added = shade.highlight * (1. - alpha);
    let expected_alpha = ((added + alpha * (1. - added)) * 255.).round() as u8;
    let rendered = crate::effects::cpu::render(&image, &settings).unwrap();
    assert_eq!(rendered[(x, y)][3], expected_alpha);
    settings.bevel.as_mut().unwrap().size = 0.;
    let unchanged = crate::effects::cpu::render(&image, &settings).unwrap();
    assert_eq!(unchanged[(x, y)], image[(x, y)]);
}

#[test]
fn bevel_follows_mask_and_transform_without_changing_source() {
    let mut doc = Document::new(32, 32).unwrap();
    let layer = &mut doc.layers[0];
    let original = Arc::new(RgbaImage::from_pixel(7, 7, Rgba([100, 130, 180, 255])));
    layer.content = LayerContent::Raster(Some(original.clone()));
    layer.transform.origin = [8., 9.];
    layer.transform.size = [14., 7.];
    layer.transform.rotation = 90.;
    layer.transform.sampling = Sampling::Nearest;
    let mask = Arc::new(GrayImage::from_fn(7, 7, |x, y| {
        Luma([if x < 4 && y < 5 { 255 } else { 0 }])
    }));
    layer.mask = Some(Mask {
        pixels: mask.clone(),
        enabled: true,
        linked: true,
        placement: None,
    });
    layer.effects = Some(effect(Style::Emboss));
    let mut baked_mask = doc.clone();
    baked_mask.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(7, 7, |x, y| {
            Rgba([100, 130, 180, mask[(x, y)][0]])
        }))));
    baked_mask.layers[0].mask = None;
    let image = crate::render::render(&doc, 32, 32).unwrap();
    assert_eq!(image, crate::render::render(&baked_mask, 32, 32).unwrap());
    assert!(Arc::ptr_eq(&original, doc.layers[0].raster().unwrap()));
    assert!(Arc::ptr_eq(
        &mask,
        &doc.layers[0].mask.as_ref().unwrap().pixels
    ));
    doc.layers[0]
        .effects
        .as_mut()
        .unwrap()
        .bevel
        .as_mut()
        .unwrap()
        .enabled = false;
    let without_effect = crate::render::render(&doc, 32, 32).unwrap();
    assert_ne!(image, without_effect);
    doc.layers[0].effects = None;
    assert_eq!(without_effect, crate::render::render(&doc, 32, 32).unwrap());
}

#[test]
fn invalid_bevel_settings_fail_document_validation_even_when_disabled() {
    let mut doc = Document::new(2, 2).unwrap();
    doc.layers[0].effects = Some(effect(Style::Inner));
    let settings = doc.layers[0]
        .effects
        .as_mut()
        .unwrap()
        .bevel
        .as_mut()
        .unwrap();
    settings.enabled = false;
    settings.altitude = -1.;
    assert!(doc.validate().is_err());
}
