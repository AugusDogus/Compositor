use super::*;
use image::{Rgba, RgbaImage};

fn document(enabled: bool) -> Document {
    let mut doc = Document::new(7, 5).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        7,
        5,
        Rgba([37, 91, 180, 173]),
    ))));
    let mut overlay = Overlay::new(
        Pattern::from_pixels(
            "Checker",
            RgbaImage::from_fn(2, 2, |x, y| {
                Rgba([
                    x as u8 * 255,
                    y as u8 * 255,
                    77,
                    if x == y { 128 } else { 255 },
                ])
            }),
        )
        .unwrap(),
    );
    overlay.settings.enabled = enabled;
    overlay.settings.scale = 1.7;
    overlay.settings.opacity = 0.6;
    doc.layers[0].effects = Some(crate::effects::LayerEffects {
        pattern_overlay: Some(Box::new(overlay)),
        ..Default::default()
    });
    let mut duplicate = doc.layers[0].clone();
    duplicate.id = Uuid::new_v4();
    doc.add(duplicate).unwrap();
    doc
}

#[test]
fn pattern_project_recovery_and_projection_preserve_sources_and_share_assets() {
    let dir = tempfile::tempdir().unwrap();
    for enabled in [false, true] {
        let doc = document(enabled);
        for recovery in [false, true] {
            let path = dir.path().join(format!("{enabled}-{recovery}.comp"));
            if recovery {
                crate::project::save_recovery(&doc, &path).unwrap();
            } else {
                crate::project::save(&doc, &path).unwrap();
            }
            let reopened = crate::project::load(&path).unwrap();
            assert_eq!(reopened, doc);
            assert_eq!(
                fs::read_dir(path.join("authoring/patterns"))
                    .unwrap()
                    .count(),
                1
            );
            let tile = |index: usize| {
                reopened.layers[index]
                    .effects
                    .as_ref()
                    .unwrap()
                    .pattern_overlay
                    .as_ref()
                    .unwrap()
                    .pattern
                    .pixels()
            };
            assert!(Arc::ptr_eq(tile(0), tile(1)));
            if !recovery {
                let projection = super::super::load_native(&path).unwrap();
                assert_eq!(
                    crate::render::render(&projection, 7, 5).unwrap(),
                    crate::render::render(&doc, 7, 5).unwrap()
                );
            }
        }
    }
}

#[test]
fn pattern_native_writer_refuses_silent_loss_and_psd_reports_rasterization() {
    let doc = document(true);
    assert!(
        crate::project::native_metadata(&doc)
            .unwrap_err()
            .to_string()
            .contains("Pattern Overlay")
    );
    let report = crate::psd::export_report(&doc).description();
    assert!(report.contains("Pattern Overlay"), "{report}");
    let exported = crate::psd::decode(&crate::psd::encode(&doc).unwrap())
        .unwrap()
        .document;
    assert_eq!(
        crate::render::render(&exported, 7, 5).unwrap(),
        crate::render::render(&doc, 7, 5).unwrap()
    );
}

#[test]
fn pattern_tampering_is_fingerprinted_and_never_opened_as_editable() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let doc = document(true);
    for mode in ["pixels", "symlink", "extra"] {
        let path = dir.path().join(format!("{mode}.comp"));
        crate::project::save(&doc, &path).unwrap();
        let before = crate::project::fingerprint(&path).unwrap();
        let tile = fs::read_dir(path.join("authoring/patterns"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        match mode {
            "pixels" => RgbaImage::from_pixel(2, 2, Rgba([255; 4]))
                .save(&tile)
                .unwrap(),
            "symlink" => {
                fs::remove_file(&tile).unwrap();
                symlink(path.join("manifest.json"), &tile).unwrap();
            }
            _ => {
                fs::copy(&tile, tile.with_file_name("extra.png")).unwrap();
            }
        }
        assert_ne!(before, crate::project::fingerprint(&path).unwrap());
        assert!(crate::project::load(&path).is_err());
    }
}

#[test]
fn pattern_sidecar_rejects_paths_versions_duplicate_layers_and_bad_dimensions() {
    let dir = tempfile::tempdir().unwrap();
    for mode in ["path", "version", "layer", "size", "settings"] {
        let path = dir.path().join(format!("{mode}.comp"));
        crate::project::save(&document(true), &path).unwrap();
        let file = path.join("authoring/adjustments.json");
        let mut settings: serde_json::Value =
            serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        match mode {
            "path" => settings["patterns"][0]["asset"] = "../manifest".into(),
            "version" => settings["version"] = 4.into(),
            "layer" => settings["patterns"][1]["layer"] = settings["patterns"][0]["layer"].clone(),
            "size" => settings["patterns"][0]["width"] = 30001.into(),
            _ => settings["patterns"][0]["settings"]["scale"] = 0.into(),
        }
        fs::write(file, serde_json::to_vec(&settings).unwrap()).unwrap();
        let index = super::super::Index {
            version: 1,
            purpose: super::super::Purpose::Project,
            files: super::super::bound_files(&path).unwrap(),
        };
        super::super::write_json(&path.join(super::super::NAME), &index).unwrap();
        assert!(crate::project::load(&path).is_err(), "{mode}");
    }
}

#[test]
fn pattern_overlay_and_editable_path_geometry_survive_the_same_snapshot() {
    use crate::{
        path_shape,
        vector_path::{Anchor, BezierPath, Closure},
    };
    let mut doc = document(true);
    let effects = doc.layers[0].effects.clone();
    let id = path_shape::create(
        &mut doc,
        "Triangle",
        BezierPath {
            anchors: [[1., 1.], [5., 1.], [3., 4.]].map(Anchor::corner).to_vec(),
            closure: Closure::Closed,
        },
        path_shape::Style {
            fill: Some([255; 4]),
            stroke: None,
        },
    )
    .unwrap();
    doc.layers.iter_mut().find(|l| l.id == id).unwrap().effects = effects;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Path-pattern.comp");
    crate::project::save(&doc, &path).unwrap();
    let reopened = crate::project::load(&path).unwrap();
    assert_eq!(reopened, doc);
    assert!(reopened.layer(id).unwrap().is_path_shape());
    assert_eq!(
        fs::read_dir(path.join("authoring/patterns"))
            .unwrap()
            .count(),
        1
    );
}
