use super::*;
use crate::{
    adjustment::{ChannelMixer, ExtendedAdjustment},
    document::{LayerContent, Mask},
    posterize::Posterize,
};
use image::{GrayImage, Luma, Rgba};
use std::sync::Arc;

#[test]
fn posterize_layer_preserves_mask_opacity_and_alpha() {
    let mut doc = super::extended_tests::scene();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        4,
        2,
        Rgba([128, 128, 128, 128]),
    ))));
    doc.layers[1].content = LayerContent::ExtendedAdjustment(Box::new(
        ExtendedAdjustment::Posterize(Posterize::default()),
    ));
    let image = render(&doc, 4, 2).unwrap();
    assert_eq!(image[(0, 0)], Rgba([128, 128, 128, 128]));
    assert_eq!(image[(1, 0)], Rgba([149, 149, 149, 128]));
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn posterize_gpu_layers_match_all_levels_bytes_masks_clipping_and_groups() {
    let mut engine = gpu::Engine::new().unwrap();
    let mut base = Document::new(256, 4).unwrap();
    base.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(256, 4, |x, y| {
            Rgba([
                x as u8,
                255 - x as u8,
                (x as u8).wrapping_mul(17),
                (y * 85) as u8,
            ])
        }))));
    let mut adjustment = Layer::blank("Posterize", 256, 4);
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
    for levels in 2..=256 {
        for clipping in [false, true] {
            for grouped in [false, true] {
                let mut doc = base.clone();
                doc.layers[1].content = LayerContent::ExtendedAdjustment(Box::new(
                    ExtendedAdjustment::Posterize(Posterize::new(levels).unwrap()),
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
                            "levels={levels} clipping={clipping} grouped={grouped}: {a:?}/{b:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn posterize_gpu_preserves_continuous_backdrops_and_full_level_identity() {
    let mut engine = gpu::Engine::new().unwrap();
    for levels in [2, 4, 8, 16, 128, 255, 256] {
        let boundary = 256. / f64::from(levels);
        let rgb = [
            (boundary - 0.001) / 255.,
            boundary / 255.,
            (boundary + 0.001) / 255.,
        ];
        let mut doc = Document::new(256, 1).unwrap();
        doc.layers[0].content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(256, 1, |x, _| {
                Rgba([255, 255, 255, x as u8])
            }))));
        let mut mixer = Layer::blank("Continuous source", 256, 1);
        mixer.content = LayerContent::ExtendedAdjustment(Box::new(
            ExtendedAdjustment::ChannelMixer(ChannelMixer {
                rows: rgb.map(|c| [0., 0., 0., c * 100.]),
                monochrome: false,
            }),
        ));
        doc.add(mixer).unwrap();
        let before = render(&doc, 256, 1).unwrap();
        let mut adjustment = Layer::blank("Posterize", 256, 1);
        adjustment.content = LayerContent::ExtendedAdjustment(Box::new(
            ExtendedAdjustment::Posterize(Posterize::new(levels).unwrap()),
        ));
        doc.add(adjustment).unwrap();
        let scene = gpu::scene::Scene::compile(&doc, [0.; 2], [1.; 2]).unwrap();
        let actual = engine.render(&scene, [256, 1], [0.; 2], [1.; 2]).unwrap();
        let expected = render(&doc, 256, 1).unwrap();
        for (a, b) in actual.pixels().zip(expected.pixels()) {
            for channel in 0..4 {
                assert!(
                    a[channel].abs_diff(b[channel]) <= 1,
                    "levels={levels}: {a:?}/{b:?}"
                );
            }
        }
        if levels == 256 {
            assert_eq!(expected, before);
        } else {
            assert_eq!(actual[(255, 0)][0], 0);
            assert!(actual[(255, 0)][1] > 0);
        }
    }
}
