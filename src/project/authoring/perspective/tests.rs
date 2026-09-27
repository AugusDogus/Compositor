use super::*;
use crate::{document::Mask, geometry::projective::Projective};
use image::{GrayImage, Luma, Rgba, RgbaImage};

fn document() -> Document {
    let mut document = Document::new(16, 12).unwrap();
    let layer = &mut document.layers[0];
    layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(16, 12, |x, y| {
        Rgba([(x * 13) as u8, (y * 17) as u8, 100, 190])
    }))));
    layer.transform.warp =
        Some(Projective::new([[0.1, 0.2], [0.9, 0.], [1., 1.], [0., 0.9]]).unwrap());
    let mut placement = Transform::new(16, 12);
    placement.warp = Some(Projective::new([[0., 0.], [0.8, 0.1], [1., 1.], [0.1, 0.9]]).unwrap());
    layer.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_fn(16, 12, |x, _| Luma([(x * 17) as u8]))),
        enabled: true,
        linked: false,
        placement: Some(placement),
    });
    document
}

#[test]
fn perspective_projects_and_recovery_preserve_sources_and_independent_masks() {
    let doc = document();
    let directory = tempfile::tempdir().unwrap();
    for recovery in [false, true] {
        let path = directory.path().join(format!("{recovery}.comp"));
        if recovery {
            crate::project::save_recovery(&doc, &path).unwrap();
        } else {
            crate::project::save(&doc, &path).unwrap();
        }
        assert_eq!(crate::project::load(&path).unwrap(), doc);
        let metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(path.join("authoring/adjustments.json")).unwrap())
                .unwrap();
        assert_eq!(metadata["version"], 9);
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(path.join("authoring/manifest.json")).unwrap())
                .unwrap();
        assert!(!manifest.to_string().contains("warp"));
        if !recovery {
            let projected = super::super::super::load_native(&path).unwrap();
            let a = crate::render::render(&projected, 16, 12).unwrap();
            let b = crate::render::render(&doc, 16, 12).unwrap();
            for (a, b) in a.pixels().zip(b.pixels()) {
                assert_eq!(a[3], b[3]);
                if b[3] > 0 {
                    assert_eq!(a, b);
                }
            }
        }
    }
}

#[test]
fn perspective_snapshot_rejects_duplicate_missing_and_mismatched_targets() {
    let original = document();
    let mut source = original.clone();
    let saved = extract(&mut source);
    validate(&saved).unwrap();
    assert!(validate(&[saved[0], saved[0]]).is_err());
    let mut mismatch = source.clone();
    mismatch.layers[0].transform.origin[0] += 1.;
    assert!(preflight(&mismatch, &saved).is_err());
    let mut missing_mask = source.clone();
    missing_mask.layers[0].mask = None;
    assert!(restore(&mut missing_mask, saved).is_err());
    let mut source = original.clone();
    let saved = extract(&mut source);
    source.layers.clear();
    assert!(preflight(&source, &saved).is_err());
}

#[test]
fn perspective_psd_bakes_placement_and_preserves_visible_pixels() {
    let doc = document();
    assert!(
        crate::psd::export_report(&doc)
            .description()
            .contains("perspective placement is baked")
    );
    let reopened = crate::psd::decode(&crate::psd::encode(&doc).unwrap())
        .unwrap()
        .document;
    assert!(
        reopened
            .layers
            .iter()
            .all(|layer| layer.transform.warp.is_none())
    );
    let before = crate::render::render(&doc, 16, 12).unwrap();
    let after = crate::render::render(&reopened, 16, 12).unwrap();
    for (a, b) in before.pixels().zip(after.pixels()) {
        for channel in 0..4 {
            if channel == 3 || a[3] > 0 {
                assert!(a[channel].abs_diff(b[channel]) <= 1, "{a:?} {b:?}");
            }
        }
    }
}

#[test]
fn perspective_and_blend_if_roundtrip_together() {
    let mut doc = document();
    doc.layers[0].blend_if = Some(crate::blend_if::Settings {
        source: crate::blend_if::Range::new([10, 65], [170, 240]).unwrap(),
        underlying: crate::blend_if::Range::new([20, 80], [190, 250]).unwrap(),
        ..Default::default()
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("combined.comp");
    crate::project::save(&doc, &path).unwrap();
    assert_eq!(crate::project::load(&path).unwrap(), doc);
    let reopened = crate::psd::decode(&crate::psd::encode(&doc).unwrap())
        .unwrap()
        .document;
    assert_eq!(reopened.layers[0].blend_if, doc.layers[0].blend_if);
    assert_render_matches(&doc, &reopened);
}

fn assert_render_matches(before: &Document, after: &Document) {
    let a = crate::render::render(before, before.width, before.height).unwrap();
    let b = crate::render::render(after, after.width, after.height).unwrap();
    for (a, b) in a.pixels().zip(b.pixels()) {
        for channel in 0..4 {
            if channel == 3 || a[3] > 0 {
                assert!(a[channel].abs_diff(b[channel]) <= 1, "{a:?} {b:?}");
            }
        }
    }
}

#[test]
fn perspective_psd_bakes_source_space_effects_before_projection() {
    let mut doc = Document::new(100, 100).unwrap();
    let layer = &mut doc.layers[0];
    layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        100,
        100,
        Rgba([255, 0, 0, 255]),
    ))));
    layer.transform.warp =
        Some(Projective::new([[0.2, 0.2], [0.8, 0.1], [0.6, 0.8], [0.4, 0.7]]).unwrap());
    layer.effects = Some(crate::effects::LayerEffects {
        stroke: Some(crate::effects::StrokeEffect {
            size: 5.,
            green: 1.,
            ..Default::default()
        }),
        ..Default::default()
    });
    let reopened = crate::psd::decode(&crate::psd::encode(&doc).unwrap())
        .unwrap()
        .document;
    assert!(reopened.layers[0].effects.is_none());
    assert_render_matches(&doc, &reopened);
    assert!(
        crate::psd::export_report(&doc)
            .description()
            .contains("perspective cannot be represented by Photoshop effect sizes")
    );
}
