use super::*;
use crate::{
    adjustment::{Adjustment, Kind},
    blend::Blend,
    document::{Layer, LayerContent, Mask},
    geometry::Transform,
};
use image::{GrayImage, Luma, Rgba};
use std::sync::Arc;
fn check(engine: &mut Engine, doc: &Document, size: [u32; 2], origin: Point, step: Point) {
    let scene = Scene::compile(doc, origin, step).unwrap();
    let actual = engine.render(&scene, size, origin, step).unwrap();
    let expected = super::super::region(doc, size[0], size[1], origin, step).unwrap();
    for (i, (a, b)) in actual.as_raw().iter().zip(expected.as_raw()).enumerate() {
        assert!(a.abs_diff(*b) <= 2, "byte {i}: GPU {a}, CPU {b}");
    }
}
#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn compositing_matches_masks_groups_clipping_blends_and_adjustments() {
    let mut engine = Engine::new().unwrap();
    let mut doc = Document::new(80, 70).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(80, 70, |x, y| {
            Rgba([(x * 3) as u8, (y * 3) as u8, 70, 180])
        }))));
    doc.layers[0].transform.sampling = crate::geometry::Sampling::Smooth;
    let mut layer = Layer::blank("Top", 40, 50);
    layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(40, 50, |x, y| {
        Rgba([200, (x * 5) as u8, (y * 4) as u8, ((x + y) * 2) as u8])
    }))));
    layer.transform.origin = [15., 10.];
    layer.transform.rotation = 23.;
    layer.transform.flip_x = true;
    layer.opacity = 0.63;
    layer.transform.sampling = crate::geometry::Sampling::Smooth;
    layer.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_fn(10, 10, |x, y| {
            Luma([((x + y) * 12) as u8])
        })),
        enabled: true,
        linked: false,
        placement: Some(Transform {
            origin: [7., 8.],
            size: [60., 50.],
            rotation: -15.,
            ..Transform::new(10, 10)
        }),
    });
    let top = layer.id;
    doc.add(layer).unwrap();
    for mode in Blend::ALL {
        doc.layers[1].blend = mode;
        check(&mut engine, &doc, [97, 85], [-6.3, -4.7], [0.87, 0.91]);
    }
    doc.layers[1].blend = Blend::Normal;
    for kind in [
        Kind::HueSaturation,
        Kind::Levels,
        Kind::Curves,
        Kind::Exposure,
        Kind::GradientMap,
        Kind::Grain,
    ] {
        let mut adjustment = Layer::blank("Adjustment", 80, 70);
        let mut a = Adjustment::new(kind);
        a.hue = 35.;
        a.saturation = 40.;
        a.lightness = -15.;
        a.levels.ranges[0].gamma = 0.7;
        a.levels.ranges[2].black = 15.;
        a.curves.channels[1].insert(1, crate::adjustment::CurvePoint { x: 120., y: 70. });
        adjustment.content = LayerContent::Adjustment(Box::new(a));
        adjustment.opacity = 0.7;
        doc.add(adjustment).unwrap();
        check(&mut engine, &doc, [97, 85], [-6.3, -4.7], [0.87, 0.91]);
        doc.layers.last_mut().unwrap().clip_source = Some(top);
        check(&mut engine, &doc, [97, 85], [-6.3, -4.7], [0.87, 0.91]);
        doc.layers.pop();
    }
    doc.layers[1].clip_source = Some(doc.layers[0].id);
    let mut group = Layer::blank("Folder", 80, 70);
    group.content = LayerContent::Group;
    group.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([170]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    for l in &mut doc.layers {
        l.parent = Some(group.id);
    }
    doc.add(group).unwrap();
    check(&mut engine, &doc, [97, 85], [-6.3, -4.7], [0.87, 0.91]);
    doc.layers.last_mut().unwrap().opacity = 0.45;
    check(&mut engine, &doc, [97, 85], [-6.3, -4.7], [0.87, 0.91]);
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn lanczos_preview_resize_preserves_premultiplied_color_and_mask_edges() {
    let mut engine = Engine::new().unwrap();
    for (from, to) in [
        ([137, 119], [51, 37]),
        ([17, 19], [1, 1]),
        ([31, 19], [31, 7]),
        ([31, 19], [13, 19]),
    ] {
        let original = RgbaImage::from_fn(from[0], from[1], |x, y| {
            Rgba([
                (x * 17) as u8,
                (y * 11) as u8,
                ((x + y) * 7) as u8,
                ((x * 3 + y * 19) % 256) as u8,
            ])
        });
        let bytes = engine
            .resize(original.as_raw(), from, to, false)
            .unwrap()
            .unwrap();
        let actual = RgbaImage::from_raw(to[0], to[1], bytes).unwrap();
        let expected = crate::native_pixels::unpremultiply(image::imageops::resize(
            &crate::native_pixels::premultiply(&original),
            to[0],
            to[1],
            image::imageops::FilterType::Lanczos3,
        ));
        for (a, b) in actual.pixels().zip(expected.pixels()) {
            assert!(a[3].abs_diff(b[3]) <= 1, "alpha {a:?} vs {b:?}");
            for i in 0..3 {
                assert!(
                    (i32::from(a[i]) * i32::from(a[3]) - i32::from(b[i]) * i32::from(b[3])).abs()
                        <= 510,
                    "color {a:?} vs {b:?}"
                );
            }
        }
        let gray = GrayImage::from_fn(from[0], from[1], |x, y| {
            Luma([((x * 3 + y * 19) % 256) as u8])
        });
        let source: Vec<u32> = gray.as_raw().iter().map(|p| u32::from(*p)).collect();
        let bytes = engine
            .resize(bytemuck::cast_slice(&source), from, to, true)
            .unwrap()
            .unwrap();
        let expected =
            image::imageops::resize(&gray, to[0], to[1], image::imageops::FilterType::Lanczos3);
        for (a, b) in bytes.chunks_exact(4).zip(expected.pixels()) {
            assert!(a[0].abs_diff(b[0]) <= 1);
        }
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn error_scopes_capture_validation_and_release_correctly_on_early_exit() {
    let engine = Engine::new().unwrap();
    let scopes = crate::gpu::ErrorScopes::new(&engine.device);
    let _ = engine
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl("invalid wgsl".into()),
        });
    assert!(scopes.finish().is_some());
    {
        let _scopes = crate::gpu::ErrorScopes::new(&engine.device);
    }
    assert!(
        crate::gpu::ErrorScopes::new(&engine.device)
            .finish()
            .is_none()
    );
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn viewport_batches_high_quality_reductions_and_sparse_coordinates_match() {
    super::initialize().unwrap();
    let mut doc = Document::new(30000, 30000).unwrap();
    let pixels = Arc::new(RgbaImage::from_fn(1700, 1200, |x, y| {
        Rgba([(x * 7) as u8, (y * 5) as u8, 90, 160 + (x % 96) as u8])
    }));
    doc.layers[0].content = LayerContent::Raster(Some(pixels.clone()));
    doc.layers[0].transform = Transform {
        origin: [24500., 25600.],
        size: [1700., 1200.],
        flip_y: true,
        ..Transform::new(1700, 1200)
    };
    let mut cache = super::super::DownsampleCache::default();
    for (size, origin, step) in [
        ([1200, 1000], [24370., 25560.], [1., 1.]),
        ([600, 450], [24370., 25560.], [2.13, 2.17]),
    ] {
        let actual =
            super::super::region_accelerated(&doc, size[0], size[1], origin, step, &mut cache)
                .unwrap();
        let expected = super::super::region(&doc, size[0], size[1], origin, step).unwrap();
        for (i, (a, b)) in actual.as_raw().iter().zip(expected.as_raw()).enumerate() {
            assert!(
                a.abs_diff(*b) <= 2,
                "viewport {size:?} byte {i}: GPU {a}, CPU {b}"
            );
        }
        assert!(Arc::ptr_eq(doc.layers[0].raster().unwrap(), &pixels));
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn adjustment_ranges_extremes_and_seed_bits_match_cpu() {
    use crate::adjustment::{
        Color, ColorRange, Exposure, GradientMap, Grain, HueBand, HueSaturation, RangeAdjustment,
    };
    let mut engine = Engine::new().unwrap();
    let mut doc = Document::new(96, 72).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(96, 72, |x, y| {
            Rgba([
                (x * 41 + y * 3) as u8,
                (y * 17) as u8,
                (x * 7 + y * 19) as u8,
                64 + (x % 192) as u8,
            ])
        }))));
    doc.layers[0].transform.origin = [-35., -27.];
    let mut variants = Vec::new();
    for colorize in [false, true] {
        let mut a = Adjustment::new(Kind::HueSaturation);
        a.hsv_settings = Some(HueSaturation {
            colorize,
            invert_range: true,
            range: ColorRange::Reds,
            adjustments: vec![
                (
                    ColorRange::Master,
                    RangeAdjustment {
                        hue: -35.,
                        saturation: -40.,
                        lightness: 12.,
                    },
                ),
                (
                    ColorRange::Reds,
                    RangeAdjustment {
                        hue: 167.,
                        saturation: 65.,
                        lightness: -35.,
                    },
                ),
                (
                    ColorRange::Magentas,
                    RangeAdjustment {
                        hue: 45.,
                        saturation: 75.,
                        lightness: 20.,
                    },
                ),
            ],
            bands: vec![(
                ColorRange::Reds,
                HueBand {
                    falloff_start: 280.,
                    range_start: 325.,
                    range_end: 25.,
                    falloff_end: 75.,
                },
            )],
        });
        variants.push(a);
    }
    for seed in [0, 0x7fc0_1234, u32::MAX] {
        let mut a = Adjustment::new(Kind::Grain);
        a.grain_settings = Some(Grain {
            amount: 95.,
            size: 4.7,
            roughness: 75.,
            seed,
        });
        variants.push(a);
    }
    let mut exposure = Adjustment::new(Kind::Exposure);
    exposure.exposure_settings = Some(Exposure {
        exposure: 2.7,
        offset: -0.11,
        gamma: 0.73,
    });
    variants.push(exposure);
    let mut map = Adjustment::new(Kind::GradientMap);
    map.gradient_map_settings = Some(GradientMap {
        shadows: Color {
            red: 0.2,
            green: 0.7,
            blue: 0.8,
        },
        highlights: Color {
            red: 0.9,
            green: 0.1,
            blue: 0.3,
        },
        reversed: true,
    });
    variants.push(map);
    for a in variants {
        let mut layer = Layer::blank(format!("{:?}", a.kind), 96, 72);
        layer.content = LayerContent::Adjustment(Box::new(a));
        layer.opacity = 0.87;
        layer.blend = Blend::Color;
        doc.add(layer).unwrap();
        check(&mut engine, &doc, [120, 100], [-40.2, -31.4], [0.81, 0.79]);
        doc.layers.pop();
    }
}
