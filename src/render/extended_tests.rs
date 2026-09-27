use super::*;
use crate::{
    adjustment::{ExtendedAdjustment, PhotoFilter},
    document::{LayerContent, Mask},
};
use image::{GrayImage, Luma, Rgba};
use std::sync::Arc;

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn threshold_gpu_layers_preserve_colored_exact_and_continuous_boundaries() {
    let mut engine = gpu::Engine::new().unwrap();
    for (rgb, level) in [
        ([0_u8, 6, 127], 18),
        ([0, 122, 249], 100),
        ([0, 208, 236], 149),
    ] {
        let mut doc = Document::new(256, 1).unwrap();
        doc.layers[0].content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(256, 1, |x, _| {
                Rgba([rgb[0], rgb[1], rgb[2], x as u8])
            }))));
        let mut threshold = Layer::blank("Threshold", 256, 1);
        threshold.content = LayerContent::ExtendedAdjustment(Box::new(
            ExtendedAdjustment::Threshold(crate::threshold::Threshold { level }),
        ));
        doc.add(threshold).unwrap();
        let scene = gpu::scene::Scene::compile(&doc, [0.; 2], [1.; 2]).unwrap();
        let actual = engine.render(&scene, [256, 1], [0.; 2], [1.; 2]).unwrap();
        for alpha in 1..=255_u32 {
            assert_eq!(
                actual[(alpha, 0)],
                Rgba([255, 255, 255, alpha as u8]),
                "{rgb:?} cutoff{level} alpha{alpha}"
            );
        }
        for offset in [-0.001, 0., 0.001] {
            let mut mixer = Layer::blank("Continuous source", 256, 1);
            mixer.content = LayerContent::ExtendedAdjustment(Box::new(
                ExtendedAdjustment::ChannelMixer(crate::adjustment::ChannelMixer {
                    rows: [
                        [0., 0., 0., f64::from(rgb[0]) * 100. / 255.],
                        [0., 0., 0., (f64::from(rgb[1]) + offset) * 100. / 255.],
                        [0., 0., 0., f64::from(rgb[2]) * 100. / 255.],
                    ],
                    monochrome: false,
                }),
            ));
            doc.layers.insert(1, mixer);
            let scene = gpu::scene::Scene::compile(&doc, [0.; 2], [1.; 2]).unwrap();
            let actual = engine.render(&scene, [256, 1], [0.; 2], [1.; 2]).unwrap();
            let expected = render(&doc, 256, 1).unwrap();
            assert_eq!(actual, expected, "{rgb:?} cutoff{level} offset{offset}");
            let value = if offset < 0. { 0 } else { 255 };
            assert_eq!(actual[(255, 0)], Rgba([value, value, value, 255]));
            doc.layers.remove(1);
        }
    }
}

#[test]
fn threshold_layer_preserves_continuous_boundary_mask_opacity_and_alpha() {
    let mut doc = scene();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        4,
        2,
        Rgba([128, 128, 128, 128]),
    ))));
    doc.layers[1].content = LayerContent::ExtendedAdjustment(Box::new(
        ExtendedAdjustment::Threshold(crate::threshold::Threshold::default()),
    ));
    let image = render(&doc, 4, 2).unwrap();
    assert_eq!(image[(0, 0)], Rgba([128, 128, 128, 128]));
    assert_eq!(image[(1, 0)], Rgba([192, 192, 192, 128]));
    let settings = ExtendedAdjustment::Threshold(crate::threshold::Threshold::default());
    assert_eq!(
        settings.apply_rgba([127.75 / 255., 127.75 / 255., 127.75 / 255., 0.5]),
        [0., 0., 0., 0.5]
    );
    assert_eq!(
        settings.apply_rgba([0.7, 0.4, 0.1, 0.]),
        [0.7, 0.4, 0.1, 0.]
    );
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn threshold_gpu_layers_match_gray_boundaries_masks_clipping_and_groups() {
    let mut engine = gpu::Engine::new().unwrap();
    for level in 0..=255_u8 {
        for clipping in [false, true] {
            for grouped in [false, true] {
                let mut doc = scene();
                doc.layers[0].content =
                    LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(4, 2, |x, y| {
                        let gray = match x {
                            0 => level.saturating_sub(1),
                            1 => level,
                            _ => level.saturating_add(1),
                        };
                        Rgba([
                            gray,
                            gray,
                            gray,
                            if y == 0 {
                                255
                            } else {
                                [0, 73, 128, 200][x as usize]
                            },
                        ])
                    }))));
                doc.layers[1].content = LayerContent::ExtendedAdjustment(Box::new(
                    ExtendedAdjustment::Threshold(crate::threshold::Threshold { level }),
                ));
                doc.layers[1].clip_source = clipping.then_some(doc.layers[0].id);
                if grouped {
                    let mut folder = Layer::blank("Folder", 4, 2);
                    folder.content = LayerContent::Group;
                    folder.opacity = 0.7;
                    for layer in &mut doc.layers {
                        layer.parent = Some(folder.id);
                    }
                    doc.layers.insert(0, folder);
                }
                let scene = gpu::scene::Scene::compile(&doc, [0.; 2], [1.; 2]).unwrap();
                let actual = engine.render(&scene, [4, 2], [0.; 2], [1.; 2]).unwrap();
                let expected = render(&doc, 4, 2).unwrap();
                for (a, b) in actual.pixels().zip(expected.pixels()) {
                    for channel in 0..4 {
                        assert!(
                            a[channel].abs_diff(b[channel]) <= 1,
                            "level={level} clipped={clipping} grouped={grouped}: GPU={actual:?} CPU={expected:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn selective_color_layer_preserves_mask_opacity_and_alpha() {
    use crate::selective_color::{Mode, Range, SelectiveColor};
    let mut doc = scene();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(4, 2, |x, _| {
            Rgba([128, 128, 128, if x == 3 { 0 } else { 128 }])
        }))));
    let mut settings = SelectiveColor {
        mode: Mode::Absolute,
        ..Default::default()
    };
    settings.adjustments[Range::Neutrals.index()] = [50., 0., 0., 0.];
    doc.layers[1].content =
        LayerContent::ExtendedAdjustment(Box::new(ExtendedAdjustment::SelectiveColor(settings)));
    let image = render(&doc, 4, 2).unwrap();
    assert_eq!(image[(0, 0)], Rgba([128, 128, 128, 128]));
    assert_eq!(image[(1, 0)], Rgba([65, 128, 128, 128]));
    assert_eq!(image[(3, 0)][3], 0);
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn selective_color_gpu_layers_match_modes_masks_clipping_groups_and_chains() {
    use crate::selective_color::{Mode, SelectiveColor};
    let mut engine = gpu::Engine::new().unwrap();
    for mode in [Mode::Relative, Mode::Absolute] {
        for clipping in [false, true] {
            for grouped in [false, true] {
                let mut doc = scene();
                doc.layers[0].content =
                    LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(4, 2, |x, y| {
                        Rgba([
                            [0, 255, 128, 200][x as usize],
                            (y * 180) as u8,
                            (x * 70) as u8,
                            if x == 3 { 0 } else { 128 },
                        ])
                    }))));
                let settings = SelectiveColor {
                    mode,
                    adjustments: std::array::from_fn(|i| {
                        [i as f32 * 20. - 80., 32.5, -42.75, 17.25]
                    }),
                };
                doc.layers[1].content = LayerContent::ExtendedAdjustment(Box::new(
                    ExtendedAdjustment::SelectiveColor(settings),
                ));
                doc.layers[1].clip_source = clipping.then_some(doc.layers[0].id);
                if grouped {
                    let mut folder = Layer::blank("Folder", 4, 2);
                    folder.content = LayerContent::Group;
                    folder.opacity = 0.7;
                    for layer in &mut doc.layers {
                        layer.parent = Some(folder.id);
                    }
                    doc.layers.insert(0, folder);
                }
                let mut next = Layer::blank("Second adjustment", 4, 2);
                next.content = LayerContent::ExtendedAdjustment(Box::new(
                    ExtendedAdjustment::SelectiveColor(SelectiveColor {
                        mode,
                        adjustments: [[-20., 50., -30., 20.]; 9],
                    }),
                ));
                doc.add(next).unwrap();
                let scene = gpu::scene::Scene::compile(&doc, [0.; 2], [1.; 2]).unwrap();
                let actual = engine.render(&scene, [4, 2], [0.; 2], [1.; 2]).unwrap();
                let expected = render(&doc, 4, 2).unwrap();
                for (a, b) in actual.pixels().zip(expected.pixels()) {
                    assert_eq!(a[3], b[3]);
                    for channel in 0..3 {
                        assert!(
                            a[channel].abs_diff(b[channel]) <= 1,
                            "{mode:?} clipped={clipping} grouped={grouped}: GPU={actual:?} CPU={expected:?}"
                        );
                    }
                }
            }
        }
    }
}

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
