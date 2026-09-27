use super::*;
use crate::{
    adjustment::ExtendedAdjustment,
    document::{LayerContent, Mask},
    vibrance::Vibrance,
};
use image::{GrayImage, Luma, Rgba};
use std::sync::Arc;

#[test]
fn vibrance_layer_preserves_mask_opacity_and_alpha() {
    let mut doc = super::extended_tests::scene();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        4,
        2,
        Rgba([255, 0, 0, 128]),
    ))));
    doc.layers[1].content = LayerContent::ExtendedAdjustment(Box::new(
        ExtendedAdjustment::Vibrance(Vibrance::new(0., -100.).unwrap()),
    ));
    let image = render(&doc, 4, 2).unwrap();
    assert_eq!(image[(0, 0)], Rgba([255, 0, 0, 128]));
    assert_eq!(image[(1, 0)], Rgba([155, 27, 27, 128]));
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn vibrance_gpu_layers_match_signed_values_masks_clipping_and_groups() {
    let mut engine = gpu::Engine::new().unwrap();
    let mut base = Document::new(256, 4).unwrap();
    base.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(256, 4, |x, y| {
            Rgba([x as u8, 120, (x as u8).wrapping_mul(17), (y * 85) as u8])
        }))));
    let mut adjustment = Layer::blank("Vibrance", 256, 4);
    adjustment.opacity = 0.75;
    adjustment.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_fn(256, 4, |x, _| {
            Luma([if x % 7 == 0 { 0 } else { 255 }])
        })),
        enabled: true,
        linked: true,
        placement: None,
    });
    base.add(adjustment).unwrap();
    for (vibrance, saturation) in [
        (0., 0.),
        (100., 0.),
        (-100., 0.),
        (0., -100.),
        (100., 100.),
        (47.25, -23.75),
    ] {
        for clipping in [false, true] {
            for grouped in [false, true] {
                let mut doc = base.clone();
                doc.layers[1].content = LayerContent::ExtendedAdjustment(Box::new(
                    ExtendedAdjustment::Vibrance(Vibrance::new(vibrance, saturation).unwrap()),
                ));
                doc.layers[1].clip_source = clipping.then_some(doc.layers[0].id);
                if grouped {
                    let mut folder = Layer::blank("Folder", 256, 4);
                    folder.content = LayerContent::Group;
                    folder.opacity = 0.7;
                    for layer in &mut doc.layers {
                        layer.parent = Some(folder.id);
                    }
                    doc.layers.insert(0, folder);
                }
                let scene = gpu::scene::Scene::compile(&doc, [0.; 2], [1.; 2]).unwrap();
                let actual = engine.render(&scene, [256, 4], [0.; 2], [1.; 2]).unwrap();
                let expected = render(&doc, 256, 4).unwrap();
                for (a, b) in actual.pixels().zip(expected.pixels()) {
                    for channel in 0..4 {
                        assert!(
                            a[channel].abs_diff(b[channel]) <= 1,
                            "vibrance={vibrance} saturation={saturation} clipping={clipping} grouped={grouped}: {a:?}/{b:?}"
                        );
                    }
                }
            }
        }
    }
}
