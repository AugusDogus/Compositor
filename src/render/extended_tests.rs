use super::*;
use crate::{
    adjustment::{ExtendedAdjustment, PhotoFilter},
    document::{LayerContent, Mask},
};
use image::{GrayImage, Luma, Rgba};
use std::sync::Arc;

pub(super) fn scene() -> Document {
    let mut doc = Document::new(4, 2).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(4, 2, |x, _| {
            Rgba([200, 100, 60, if x == 3 { 0 } else { 128 }])
        }))));
    let mut adjustment = Layer::blank("Filter", 4, 2);
    adjustment.content =
        LayerContent::ExtendedAdjustment(Box::new(ExtendedAdjustment::PhotoFilter(PhotoFilter {
            color: [0.5, 0., 1.],
            density: 50.,
            preserve_luminosity: false,
        })));
    adjustment.opacity = 0.5;
    adjustment.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_fn(4, 2, |x, _| {
            Luma([if x == 0 { 0 } else { 255 }])
        })),
        enabled: true,
        linked: true,
        placement: None,
    });
    doc.add(adjustment).unwrap();
    doc
}
#[test]
fn photo_filter_layer_honors_mask_opacity_and_alpha() {
    let doc = scene();
    let image = render(&doc, 4, 2).unwrap();
    assert_eq!(image[(0, 0)], Rgba([200, 100, 60, 128]));
    assert_eq!(image[(1, 0)], Rgba([175, 75, 60, 128]));
    assert_eq!(image[(3, 0)][3], 0);
}
#[test]
fn photo_filter_clipping_preserves_background_outside_base() {
    let mut doc = scene();
    doc.layers[1].clip_source = Some(doc.layers[0].id);
    let mut background = Layer::blank("Background", 4, 2);
    background.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        4,
        2,
        Rgba([0, 0, 255, 255]),
    ))));
    doc.layers.insert(0, background);
    let image = render(&doc, 4, 2).unwrap();
    assert_eq!(image[(3, 0)], Rgba([0, 0, 255, 255]));
    assert_eq!(image[(1, 0)], Rgba([88, 38, 157, 255]));
}
#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn photo_filter_layers_gpu_matches_masks_groups_clipping_and_chains() {
    let mut engine = gpu::Engine::new().unwrap();
    let mut doc = scene();
    for clipping in [false, true] {
        doc.layers[1].clip_source = clipping.then_some(doc.layers[0].id);
        for grouped in [false, true] {
            let mut source = doc.clone();
            if grouped {
                let mut group = Layer::blank("Folder", 4, 2);
                group.content = LayerContent::Group;
                group.opacity = 0.7;
                for layer in &mut source.layers {
                    layer.parent = Some(group.id);
                }
                source.layers.insert(0, group);
            }
            let mut next = Layer::blank("Second filter", 4, 2);
            next.content = LayerContent::ExtendedAdjustment(Box::new(
                ExtendedAdjustment::PhotoFilter(PhotoFilter::default()),
            ));
            source.add(next).unwrap();
            let scene = gpu::scene::Scene::compile(&source, [0.; 2], [1.; 2]).unwrap();
            let actual = engine.render(&scene, [4, 2], [0.; 2], [1.; 2]).unwrap();
            let expected = render(&source, 4, 2).unwrap();
            for (a, b) in actual.as_raw().iter().zip(expected.as_raw()) {
                assert!(a.abs_diff(*b) <= 1, "{a} != {b}");
            }
        }
    }
}

#[test]
fn channel_mixer_layer_uses_backdrop_channels_and_preserves_coverage() {
    let mut doc = scene();
    doc.layers[1].content = LayerContent::ExtendedAdjustment(Box::new(
        ExtendedAdjustment::ChannelMixer(crate::adjustment::ChannelMixer {
            rows: [[0., 0., 100., 0.], [0., 100., 0., 0.], [100., 0., 0., 0.]],
            monochrome: false,
        }),
    ));
    let image = render(&doc, 4, 2).unwrap();
    assert_eq!(image[(0, 0)], Rgba([200, 100, 60, 128]));
    assert_eq!(image[(1, 0)], Rgba([130, 100, 130, 128]));
    assert_eq!(image[(3, 0)][3], 0);
}
#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn channel_mixer_layers_gpu_matches_signed_coefficients_constants_and_monochrome() {
    let mut engine = gpu::Engine::new().unwrap();
    let mut doc = scene();
    for monochrome in [false, true] {
        for clipping in [false, true] {
            doc.layers[1].clip_source = clipping.then_some(doc.layers[0].id);
            doc.layers[1].content = LayerContent::ExtendedAdjustment(Box::new(
                ExtendedAdjustment::ChannelMixer(crate::adjustment::ChannelMixer {
                    rows: [
                        [-200., 150., 180., 30.],
                        [25., 15., -80., 60.],
                        [50., 75., 20., -100.],
                    ],
                    monochrome,
                }),
            ));
            let scene = gpu::scene::Scene::compile(&doc, [0.; 2], [1.; 2]).unwrap();
            let actual = engine.render(&scene, [4, 2], [0.; 2], [1.; 2]).unwrap();
            let expected = render(&doc, 4, 2).unwrap();
            for (a, b) in actual.as_raw().iter().zip(expected.as_raw()) {
                assert!(
                    a.abs_diff(*b) <= 1,
                    "mono={monochrome} clipped={clipping}: GPU={:?} CPU={:?}",
                    actual,
                    expected
                );
            }
        }
    }
}
