use super::*;
use crate::{
    document::{Document, Layer, LayerContent, Mask},
    geometry::Sampling,
    render,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;
fn document() -> Document {
    let mut doc = Document::new(15, 15).unwrap();
    let layer = &mut doc.layers[0];
    layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        3,
        3,
        Rgba([0, 0, 255, 128]),
    ))));
    layer.transform.origin = [6., 6.];
    layer.transform.size = [3., 3.];
    layer.transform.sampling = Sampling::Nearest;
    doc
}
#[test]
fn overlay_preserves_transparency_and_source_pixels() {
    let mut doc = document();
    let original = doc.layers[0].raster().unwrap().clone();
    doc.layers[0].effects = Some(LayerEffects {
        color_overlay: Some(ColorOverlayEffect {
            red: 1.,
            ..Default::default()
        }),
        ..Default::default()
    });
    let rendered = render::render(&doc, 15, 15).unwrap();
    assert_eq!(rendered[(7, 7)], Rgba([255, 0, 0, 128]));
    assert_eq!(rendered[(5, 5)][3], 0);
    assert!(Arc::ptr_eq(&original, doc.layers[0].raster().unwrap()));
    doc.layers[0]
        .effects
        .as_mut()
        .unwrap()
        .color_overlay
        .as_mut()
        .unwrap()
        .enabled = Some(false);
    assert_eq!(
        render::render(&doc, 15, 15).unwrap()[(7, 7)],
        Rgba([0, 0, 255, 128])
    );
}
#[test]
fn stroke_follows_enabled_mask_and_transform() {
    let mut doc = document();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        3,
        3,
        Rgba([0, 0, 255, 255]),
    ))));
    doc.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_fn(3, 3, |x, y| {
            Luma([if x == 1 && y == 1 { 255 } else { 0 }])
        })),
        enabled: true,
        linked: true,
        placement: None,
    });
    doc.layers[0].effects = Some(LayerEffects {
        stroke: Some(StrokeEffect {
            size: 1.,
            red: 1.,
            ..Default::default()
        }),
        ..Default::default()
    });
    let image = render::render(&doc, 15, 15).unwrap();
    assert_eq!(image[(7, 7)], Rgba([0, 0, 255, 255]));
    assert_eq!(image[(6, 7)], Rgba([255, 0, 0, 255]));
    assert_eq!(image[(5, 7)][3], 0);
    doc.layers[0].transform.origin = [4., 4.];
    doc.layers[0].transform.size = [6., 6.];
    doc.layers[0].transform.rotation = 90.;
    let transformed = render::render(&doc, 15, 15).unwrap();
    assert_eq!(transformed[(7, 7)], Rgba([0, 0, 255, 255]));
    assert_eq!(transformed[(4, 7)], Rgba([255, 0, 0, 255]));
}
#[test]
fn shadow_falls_away_from_light_and_inner_shadow_stays_inside() {
    let mut doc = document();
    doc.layers[0].effects = Some(LayerEffects {
        shadow: Some(ShadowEffect {
            angle: 90.,
            distance: 3.,
            blur: 0.,
            red: 1.,
            opacity: 1.,
            ..Default::default()
        }),
        inner_shadow: Some(ShadowEffect {
            distance: 1.,
            blur: 0.,
            red: 0.,
            green: 1.,
            opacity: 1.,
            ..ShadowEffect::inner_default()
        }),
        ..Default::default()
    });
    let image = render::render(&doc, 15, 15).unwrap();
    assert_eq!(image[(7, 10)], Rgba([255, 0, 0, 128]));
    assert_eq!(image[(7, 6)], Rgba([0, 255, 0, 128]));
    assert_eq!(image[(7, 5)][3], 0);
}
#[test]
fn effects_under_clipping_and_folder_opacity_do_not_thicken_base() {
    let mut doc = document();
    let base = doc.layers[0].id;
    doc.layers[0].effects = Some(LayerEffects {
        color_overlay: Some(ColorOverlayEffect {
            red: 1.,
            ..Default::default()
        }),
        ..Default::default()
    });
    let mut top = Layer::blank("Clipped", 15, 15);
    top.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        15,
        15,
        Rgba([0, 255, 0, 255]),
    ))));
    top.clip_source = Some(base);
    let mut group = Layer::blank("Group", 15, 15);
    group.content = LayerContent::Group;
    group.opacity = 0.5;
    doc.layers[0].parent = Some(group.id);
    top.parent = Some(group.id);
    doc.layers.extend([top, group]);
    doc.validate().unwrap();
    assert_eq!(
        render::render(&doc, 15, 15).unwrap()[(7, 7)],
        Rgba([128, 128, 0, 64])
    );
}
#[test]
fn effect_settings_and_disabled_flags_round_trip_with_undo() {
    let mut doc = document();
    let original = doc.clone();
    let mut session = crate::session::Session::new(doc.clone(), None);
    session
        .edit("Layer Effects", |doc| {
            doc.layers[0].effects = Some(LayerEffects {
                stroke: Some(StrokeEffect {
                    enabled: Some(false),
                    size: 500.,
                    inside: true,
                    ..Default::default()
                }),
                inner_glow: Some(super::InnerGlowEffect::default()),
                shadow: Some(ShadowEffect::default()),
                inner_shadow: Some(ShadowEffect::inner_default()),
                color_overlay: Some(ColorOverlayEffect::default()),
                outer_glow: Some(OuterGlowEffect {
                    enabled: Some(false),
                    ..Default::default()
                }),
            });
            Ok(())
        })
        .unwrap();
    doc = session.document.clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("effects.comp");
    crate::project::save(&doc, &path).unwrap();
    let reopened = crate::project::load(&path).unwrap();
    assert_eq!(reopened.layers[0].effects, doc.layers[0].effects);
    session.undo();
    assert_eq!(session.document, original);
    session.redo();
    assert_eq!(session.document, doc);
}
#[test]
fn rejects_nonfinite_or_unbounded_effects() {
    let mut e = LayerEffects {
        stroke: Some(StrokeEffect {
            size: f64::NAN,
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(!e.validate());
    e.stroke.as_mut().unwrap().size = 501.;
    assert!(!e.validate());
    let old: LayerEffects =
        serde_json::from_str(r#"{"colorOverlay":{"red":1,"green":0,"blue":0,"opacity":1}}"#)
            .unwrap();
    assert!(old.visible().color_overlay.is_some());
}

#[test]
fn distortion_preview_warps_effects_without_baking_the_committed_source() {
    let mut original = document();
    original.layers[0].effects = Some(LayerEffects {
        stroke: Some(StrokeEffect {
            size: 1.,
            red: 1.,
            ..Default::default()
        }),
        ..Default::default()
    });
    let bounds = original.layers[0].transform;
    let corners = [[6., 6.], [12., 6.], [12., 9.], [6., 9.]];
    let mut committed = original.clone();
    crate::distort::apply(&mut committed, bounds, corners, false).unwrap();
    let preview = distorted_preview(&committed, &original, bounds, corners).unwrap();
    assert!(preview.layers[0].effects.is_none());
    assert_eq!(committed.layers[0].effects, original.layers[0].effects);
    let displayed = render::render(&preview, 15, 15).unwrap();
    let applied = render::render(&committed, 15, 15).unwrap();
    assert!(displayed[(4, 7)][3] > 0);
    assert_eq!(applied[(4, 7)][3], 0);
    assert_eq!(original.layers[0].raster().unwrap().dimensions(), (3, 3));
    assert_eq!(committed.layers[0].raster().unwrap().dimensions(), (6, 3));
}

#[test]
fn invalid_effects_return_render_errors_without_panicking_or_mutating_source() {
    let mut doc = document();
    doc.layers[0].effects = Some(LayerEffects {
        stroke: Some(StrokeEffect {
            size: 501.,
            ..Default::default()
        }),
        ..Default::default()
    });
    let original = doc.clone();
    assert!(render::render(&doc, 15, 15).is_err());
    assert!(render::Sampler::new(&doc).is_err());
    assert_eq!(doc, original);
    doc.layers[0].content = LayerContent::Group;
    doc.layers[0].effects.as_mut().unwrap().stroke = Some(StrokeEffect::default());
    assert!(doc.validate().is_err());
}

#[test]
fn outer_glow_matches_upstream_defaults_schema_bounds_and_validation() {
    let mut effects: LayerEffects = serde_json::from_str(r#"{"outerGlow":{}}"#).unwrap();
    let glow = effects.outer_glow.as_ref().unwrap();
    assert_eq!([glow.red, glow.green, glow.blue], [1.; 3]);
    assert_eq!((glow.size, glow.opacity, glow.enabled), (20., 0.75, None));
    assert!(!effects.is_empty());
    assert_eq!(effects.margin(), 62);
    assert!(effects.validate());
    effects.outer_glow.as_mut().unwrap().size = 500.;
    assert!(effects.validate());
    assert_eq!(effects.margin(), 1502);
    assert!(!effects.validate_size(10_000, 10_000));
    for size in [-1., 501., f64::NAN, f64::INFINITY] {
        effects.outer_glow.as_mut().unwrap().size = size;
        assert!(!effects.validate());
    }
    effects.outer_glow = Some(OuterGlowEffect {
        enabled: Some(false),
        ..Default::default()
    });
    assert!(effects.visible().is_empty());
    assert_eq!(effects.margin(), 2);
    let serialized = serde_json::to_value(&effects).unwrap();
    assert_eq!(serialized["outerGlow"]["enabled"], false);
    assert!(serialized.get("outer_glow").is_none());
    assert!(
        serde_json::from_str::<LayerEffects>("{}")
            .unwrap()
            .outer_glow
            .is_none()
    );
}

#[test]
fn outer_glow_is_symmetric_preserves_opaque_pixels_and_fills_holes() {
    let image = RgbaImage::from_fn(31, 31, |x, y| {
        if (8..23).contains(&x) && (8..23).contains(&y) && (x != 15 || y != 15) {
            Rgba([0, 0, 255, 255])
        } else {
            Rgba([0; 4])
        }
    });
    let effects = LayerEffects {
        outer_glow: Some(OuterGlowEffect {
            size: 4.,
            ..Default::default()
        }),
        ..Default::default()
    };
    let result = cpu::render(&image, &effects);
    assert_eq!(result[(10, 10)], image[(10, 10)]);
    assert!(result[(15, 15)][3] > 150);
    assert_eq!(result[(15, 15)].0[..3], [255; 3]);
    assert!(result[(6, 15)][3] > 0);
    assert_eq!(result[(6, 15)], result[(24, 15)]);
    assert_eq!(result[(6, 15)], result[(15, 6)]);
    assert_eq!(result[(0, 0)][3], 0);
    let mut zero = effects;
    zero.outer_glow.as_mut().unwrap().size = 0.;
    assert_eq!(cpu::render(&image, &zero), image);
}

#[test]
fn outer_glow_respects_partial_alpha_and_composites_between_shadow_and_stroke() {
    let image = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 255, 128]));
    let effects = LayerEffects {
        outer_glow: Some(OuterGlowEffect {
            size: 0.01,
            red: 1.,
            green: 0.,
            blue: 0.,
            opacity: 1.,
            ..Default::default()
        }),
        shadow: Some(ShadowEffect {
            distance: 0.,
            blur: 0.,
            red: 0.,
            green: 1.,
            blue: 0.,
            opacity: 1.,
            ..Default::default()
        }),
        ..Default::default()
    };
    let result = cpu::render(&image, &effects);
    let mut tiny = effects.clone();
    tiny.outer_glow.as_mut().unwrap().size = 1e-30;
    assert_eq!(cpu::render(&image, &tiny), result);
    let alpha = 128. / 255.;
    let coverage = alpha * (1. - alpha);
    let expected_alpha = alpha + (coverage + alpha * (1. - coverage)) * (1. - alpha);
    let expected = Rgba([
        (coverage * (1. - alpha) / expected_alpha * 255.0_f64).round() as u8,
        (alpha * (1. - coverage) * (1. - alpha) / expected_alpha * 255.0_f64).round() as u8,
        (alpha / expected_alpha * 255.0_f64).round() as u8,
        (expected_alpha * 255.0_f64).round() as u8,
    ]);
    assert_eq!(result[(0, 0)], expected);
    let image = RgbaImage::from_fn(7, 7, |x, y| {
        if x == 3 && y == 3 {
            Rgba([0, 0, 255, 255])
        } else {
            Rgba([0; 4])
        }
    });
    let effects = LayerEffects {
        stroke: Some(StrokeEffect {
            size: 1.,
            red: 1.,
            ..Default::default()
        }),
        outer_glow: Some(OuterGlowEffect {
            size: 2.,
            ..Default::default()
        }),
        ..Default::default()
    };
    let result = cpu::render(&image, &effects);
    assert_eq!(result[(2, 3)], Rgba([255, 0, 0, 255]));
    assert_eq!(result[(3, 3)], Rgba([0, 0, 255, 255]));
    assert!(result[(1, 3)][3] > 0);
}

#[test]
fn outer_glow_follows_mask_transform_opacity_and_keeps_source_editable() {
    let mut doc = document();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        3,
        3,
        Rgba([0, 0, 255, 255]),
    ))));
    let original = doc.layers[0].raster().unwrap().clone();
    doc.layers[0].effects = Some(LayerEffects {
        outer_glow: Some(OuterGlowEffect {
            size: 2.,
            ..Default::default()
        }),
        ..Default::default()
    });
    doc.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(3, 3, Luma([0]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    let hidden = render::render(&doc, 15, 15).unwrap();
    assert!(hidden.pixels().all(|p| p[3] == 0));
    doc.layers[0].mask.as_mut().unwrap().enabled = false;
    doc.layers[0].transform.origin = [4., 4.];
    doc.layers[0].transform.size = [6., 6.];
    doc.layers[0].transform.rotation = 90.;
    doc.layers[0].opacity = 0.5;
    let rendered = render::render(&doc, 15, 15).unwrap();
    assert_eq!(rendered[(7, 7)], Rgba([0, 0, 255, 128]));
    assert!(rendered[(3, 7)][3] > 0);
    assert!(Arc::ptr_eq(&original, doc.layers[0].raster().unwrap()));
    let full = render::render(&doc, 15, 15).unwrap();
    assert_eq!(full, rendered);
}

#[test]
fn inner_glow_is_clipped_to_shape_and_preserves_editable_schema() {
    let effects: LayerEffects = serde_json::from_str(r#"{"innerGlow":{}}"#).unwrap();
    let glow = effects.inner_glow.as_ref().unwrap();
    assert_eq!((glow.size, glow.opacity, glow.red), (10., 0.75, 1.));
    assert!(effects.validate());
    assert_eq!(effects.margin(), 2);
    let image = RgbaImage::from_fn(41, 41, |x, y| {
        if (8..33).contains(&x) && (8..33).contains(&y) {
            image::Rgba([0, 0, 0, 255])
        } else {
            image::Rgba([0; 4])
        }
    });
    let result = super::cpu::render(&image, &effects);
    assert_eq!(result[(0, 0)][3], 0);
    assert_eq!(result[(8, 20)][3], 255);
    assert!(result[(8, 20)][0] > result[(20, 20)][0]);
    assert_eq!(result[(8, 20)], result[(32, 20)]);
    let mut doc = Document::new(41, 41).unwrap();
    doc.layers[0].content = crate::document::LayerContent::Raster(Some(std::sync::Arc::new(image)));
    doc.layers[0].effects = Some(effects.clone());
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("glow.comp");
    crate::project::save(&doc, &path).unwrap();
    assert_eq!(
        crate::project::load(&path).unwrap().layers[0].effects,
        Some(effects)
    );
}
