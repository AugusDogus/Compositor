use super::*;
use crate::{
    blend_if::{Range, Settings},
    document::Mask,
    effects::{ColorOverlayEffect, LayerEffects},
};
use image::{GrayImage, Luma};
use std::sync::Arc;

fn layer(color: [u8; 4]) -> Layer {
    let mut layer = Layer::blank("Pixels", 1, 1);
    layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(1, 1, Rgba(color)))));
    layer.transform.sampling = Sampling::Nearest;
    layer
}
fn source_range(points: [u8; 4]) -> Settings {
    Settings {
        source: Range::from_endpoints(points).unwrap(),
        ..Default::default()
    }
}
fn under_range(points: [u8; 4]) -> Settings {
    Settings {
        underlying: Range::from_endpoints(points).unwrap(),
        ..Default::default()
    }
}
fn document(layers: Vec<Layer>) -> Document {
    let mut doc = Document::new(1, 1).unwrap();
    doc.layers.clear();
    for layer in layers {
        doc.add(layer).unwrap();
    }
    doc.validate().unwrap();
    doc
}
fn alpha(doc: &Document) -> f64 {
    sample(doc, [0.5, 0.5]).unwrap()[3]
}

#[test]
fn blend_if_multiplies_opacity_and_masks_once_without_mutating_pixels() {
    let mut top = layer([100, 100, 100, 128]);
    let pixels = top.raster().unwrap().clone();
    top.opacity = 0.5;
    top.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([128]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    top.blend_if = Some(source_range([0, 200, 255, 255]));
    let doc = document(vec![top]);
    let expected = (128. / 255.) * 0.5 * (128. / 255.) * 0.5;
    assert!((alpha(&doc) - expected).abs() < 1e-12);
    assert!(Arc::ptr_eq(doc.layers[0].raster().unwrap(), &pixels));
}

#[test]
fn blend_if_backdrop_changes_are_live_and_effects_supply_source_tones() {
    let bottom = layer([0, 0, 0, 255]);
    let mut top = layer([255, 0, 0, 255]);
    top.blend_if = Some(under_range([100, 100, 255, 255]));
    let mut doc = document(vec![bottom, top]);
    let mut cache = DownsampleCache::default();
    let draw = |doc: &Document, cache: &mut DownsampleCache| {
        region_cached(doc, 1, 1, [0., 0.], [1., 1.], cache).unwrap()[(0, 0)]
    };
    assert_eq!(draw(&doc, &mut cache), Rgba([0, 0, 0, 255]));
    doc.layers[0].content = layer([255; 4]).content;
    assert_eq!(draw(&doc, &mut cache), Rgba([255, 0, 0, 255]));
    doc.layers[1].blend_if = Some(source_range([200, 200, 255, 255]));
    doc.layers[1].effects = Some(LayerEffects {
        color_overlay: Some(ColorOverlayEffect {
            red: 1.,
            green: 1.,
            blue: 1.,
            ..Default::default()
        }),
        ..Default::default()
    });
    doc.layers[0].visible = false;
    assert_eq!(draw(&doc, &mut cache), Rgba([255; 4]));
}

#[test]
fn blend_if_contiguous_clip_base_gates_original_source_once_before_children() {
    let mut base = layer([100, 100, 100, 128]);
    base.blend_if = Some(source_range([0, 200, 255, 255]));
    let mut top = layer([255, 255, 255, 255]);
    top.clip_source = Some(base.id);
    let doc = document(vec![base, top]);
    let pixel = sample(&doc, [0.5, 0.5]).unwrap();
    assert_eq!(pixel[..3], [1.; 3]);
    assert!((pixel[3] - 128. / 255. * 0.5).abs() < 1e-12);
}

#[test]
fn blend_if_clipped_child_uses_real_base_alpha_for_underlying_range() {
    let base = layer([0, 0, 0, 128]);
    let mut top = layer([255, 255, 255, 255]);
    top.clip_source = Some(base.id);
    top.blend_if = Some(under_range([100, 100, 255, 255]));
    let doc = document(vec![base, top]);
    let pixel = sample(&doc, [0.5, 0.5]).unwrap();
    assert!((pixel[0] - (1. - 128. / 255.)).abs() < 1e-12);
    assert_eq!(pixel[3], 128. / 255.);
}

#[test]
fn blend_if_baking_contiguous_base_uses_backdrop_before_the_stack() {
    let background = layer([255; 4]);
    let mut base = layer([0, 0, 0, 128]);
    base.blend_if = Some(under_range([100, 100, 255, 255]));
    let mut top = layer([255, 0, 0, 255]);
    top.clip_source = Some(base.id);
    let doc = document(vec![background, base, top]);
    assert_eq!(
        clipped_pixels(&doc, &doc.layers[2]).unwrap().unwrap()[(0, 0)][3],
        128
    );
}

#[test]
fn blend_if_detached_and_nested_masks_use_live_child_backdrop() {
    let mut base = layer([100, 100, 100, 255]);
    base.visible = false;
    base.blend_if = Some(under_range([100, 100, 255, 255]));
    let mut nested = layer([128, 128, 128, 128]);
    nested.clip_source = Some(base.id);
    nested.visible = false;
    nested.blend_if = Some(source_range([0, 128, 255, 255]));
    let backdrop = layer([0, 0, 0, 255]);
    let mut top = layer([255, 0, 0, 255]);
    top.clip_source = Some(nested.id);
    let mut doc = document(vec![base, nested, backdrop, top]);
    assert_eq!(render(&doc, 1, 1).unwrap()[(0, 0)], Rgba([0, 0, 0, 255]));
    doc.layers[2].content = layer([255; 4]).content;
    assert_eq!(
        render(&doc, 1, 1).unwrap()[(0, 0)],
        Rgba([255, 127, 127, 255])
    );
    let baked = clipped_pixels(&doc, &doc.layers[3]).unwrap().unwrap();
    assert_eq!(baked[(0, 0)][3], 128);
    doc.layers[2].content = layer([0, 0, 0, 255]).content;
    assert_eq!(
        clipped_pixels(&doc, &doc.layers[3]).unwrap().unwrap()[(0, 0)][3],
        0
    );
}

#[test]
fn blend_if_unsupported_layer_types_fail_validation_and_rendering() {
    for content in [
        LayerContent::Group,
        LayerContent::Adjustment(Box::new(crate::adjustment::Adjustment::new(
            crate::adjustment::Kind::Levels,
        ))),
    ] {
        let mut doc = Document::new(1, 1).unwrap();
        doc.layers[0].content = content;
        doc.layers[0].blend_if = Some(Settings::default());
        assert!(doc.validate().unwrap_err().to_string().contains("Blend If"));
        assert!(
            render(&doc, 1, 1)
                .unwrap_err()
                .to_string()
                .contains("Blend If")
        );
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn blend_if_gpu_matches_all_hard_byte_boundaries_and_split_clip_stacks() {
    let mut engine = gpu::Engine::new().unwrap();
    let mut doc = Document::new(256, 3).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(256, 3, |x, _| {
            Rgba([x as u8, x as u8, x as u8, 255])
        }))));
    let mut check = |doc: &Document| {
        let prepared = crate::effects::prepare(doc, false).unwrap();
        let scene = gpu::scene::Scene::compile(&prepared, [0., 0.], [1., 1.]).unwrap();
        let actual = engine.render(&scene, [256, 3], [0., 0.], [1., 1.]).unwrap();
        let expected = render(doc, 256, 3).unwrap();
        for (i, (a, b)) in actual.as_raw().iter().zip(expected.as_raw()).enumerate() {
            assert!(a.abs_diff(*b) <= 1, "byte {i}: GPU {a}, CPU {b}");
        }
    };
    for tone in 0..=255 {
        doc.layers[0].blend_if = Some(source_range([tone, tone, tone, tone]));
        check(&doc);
    }
    doc.layers[0].blend_if = Some(source_range([10, 100, 160, 250]));
    doc.layers[0].opacity = 0.61;
    let mut child = doc.layers[0].clone();
    child.id = Uuid::new_v4();
    child.clip_source = Some(doc.layers[0].id);
    child.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        256,
        3,
        Rgba([255, 60, 90, 175]),
    ))));
    child.blend_if = Some(under_range([0, 150, 200, 255]));
    doc.add(child).unwrap();
    check(&doc);
    // A gap changes the child to independent-mask clipping, then a hidden
    // intermediate source exercises nested dependency coverage on both paths.
    let mut gap = doc.layers[0].clone();
    gap.id = Uuid::new_v4();
    gap.blend_if = None;
    doc.layers.insert(1, gap);
    check(&doc);
    doc.layers[0].visible = false;
    doc.layers[1].clip_source = Some(doc.layers[0].id);
    doc.layers[1].visible = false;
    doc.layers[2].clip_source = Some(doc.layers[1].id);
    check(&doc);
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn blend_if_gpu_colored_pixels_on_both_sides_of_gray_cutoffs() {
    let mut engine = gpu::Engine::new().unwrap();
    let mut doc = Document::new(256, 256).unwrap();
    for alpha in [1, 37, 128, 255] {
        for offset in [-1, 0, 1] {
            doc.layers[0].content =
                LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(256, 256, |r, g| {
                    let blue = ((128_000_i32 - r as i32 * 299 - g as i32 * 587) / 114 + offset)
                        .clamp(0, 255) as u8;
                    Rgba([r as u8, g as u8, blue, alpha])
                }))));
            doc.layers[0].blend_if = Some(source_range([128, 128, 255, 255]));
            for underlying in [false, true] {
                if underlying {
                    doc.layers[0].blend_if = None;
                    let mut top = Layer::blank("Condition", 256, 256);
                    top.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
                        256,
                        256,
                        Rgba([255; 4]),
                    ))));
                    top.blend_if = Some(under_range([128, 128, 255, 255]));
                    doc.add(top).unwrap();
                }
                let scene = gpu::scene::Scene::compile(&doc, [0., 0.], [1., 1.]).unwrap();
                let actual = engine
                    .render(&scene, [256, 256], [0., 0.], [1., 1.])
                    .unwrap();
                let expected = render(&doc, 256, 256).unwrap();
                for (i, (a, b)) in actual.as_raw().iter().zip(expected.as_raw()).enumerate() {
                    assert!(
                        a.abs_diff(*b) <= 1,
                        "alpha {alpha}, offset {offset}, underlying {underlying}, byte {i}: GPU {a}, CPU {b}"
                    );
                }
                if underlying {
                    doc.layers.pop();
                }
            }
        }
    }
}
