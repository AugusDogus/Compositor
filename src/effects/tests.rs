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
                shadow: Some(ShadowEffect::default()),
                inner_shadow: Some(ShadowEffect::inner_default()),
                color_overlay: Some(ColorOverlayEffect::default()),
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
