use super::*;
use crate::blend_if::{Range, Settings};
use image::{Rgba, RgbaImage};

fn document(enabled: bool) -> Document {
    let mut doc = Document::new(8, 4).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(8, 4, |x, _| {
            Rgba([x as u8 * 32, 100, 180, 200])
        }))));
    doc.layers[0].blend_if = Some(Settings {
        enabled,
        source: Range::new([10, 100], [180, 230]).unwrap(),
        underlying: Range::new([30, 40], [210, 240]).unwrap(),
    });
    doc
}

#[test]
fn blend_if_native_project_recovery_and_projection_preserve_editable_sources() {
    let directory = tempfile::tempdir().unwrap();
    for enabled in [false, true] {
        let doc = document(enabled);
        assert!(
            crate::project::native_metadata(&doc)
                .unwrap_err()
                .to_string()
                .contains("Blend If")
        );
        for recovery in [false, true] {
            let path = directory.path().join(format!("{enabled}-{recovery}.comp"));
            if recovery {
                crate::project::save_recovery(&doc, &path).unwrap();
            } else {
                crate::project::save(&doc, &path).unwrap();
            }
            assert_eq!(crate::project::load(&path).unwrap(), doc);
            let metadata: serde_json::Value =
                serde_json::from_slice(&fs::read(path.join("authoring/adjustments.json")).unwrap())
                    .unwrap();
            assert_eq!(metadata["version"], 8);
            if !recovery {
                let projection = super::super::load_native(&path).unwrap();
                let a = crate::render::render(&projection, 8, 4).unwrap();
                let b = crate::render::render(&doc, 8, 4).unwrap();
                for (a, b) in a.pixels().zip(b.pixels()) {
                    assert_eq!(a[3], b[3]);
                    if b[3] > 0 {
                        assert_eq!(a, b);
                    }
                }
            }
        }
    }
}

#[test]
fn blend_if_sidecar_rejects_stale_version_invalid_pairs_and_layer_references() {
    let directory = tempfile::tempdir().unwrap();
    for mode in ["version", "pair", "layer", "duplicate"] {
        let path = directory.path().join(format!("{mode}.comp"));
        crate::project::save(&document(true), &path).unwrap();
        let file = path.join("authoring/adjustments.json");
        let mut data: serde_json::Value =
            serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        match mode {
            "version" => data["version"] = 7.into(),
            "pair" => {
                data["blend_if"][0]["settings"]["source"]["black"] = serde_json::json!([100, 10])
            }
            "layer" => data["blend_if"][0]["layer"] = Uuid::new_v4().to_string().into(),
            _ => {
                let copy = data["blend_if"][0].clone();
                data["blend_if"].as_array_mut().unwrap().push(copy);
            }
        }
        fs::write(file, serde_json::to_vec(&data).unwrap()).unwrap();
        super::super::write_json(
            &path.join(super::super::NAME),
            &super::super::Index {
                version: 1,
                purpose: super::super::Purpose::Project,
                files: super::super::bound_files(&path).unwrap(),
            },
        )
        .unwrap();
        assert!(crate::project::load(&path).is_err(), "{mode}");
    }
}

#[test]
fn blend_if_psd_preserves_eight_gray_handles_and_live_rendering() {
    let doc = document(true);
    let bytes = crate::psd::encode(&doc).unwrap();
    let imported = crate::psd::decode(&bytes).unwrap();
    assert_eq!(imported.document.layers[0].blend_if, doc.layers[0].blend_if);
    assert_eq!(
        crate::render::render(&imported.document, 8, 4).unwrap(),
        crate::render::render(&doc, 8, 4).unwrap()
    );
    assert!(
        !imported
            .report
            .description()
            .contains("Blend If ranges are omitted")
    );
    assert!(
        crate::psd::export_report(&document(false))
            .description()
            .contains("disabled Blend If")
    );
}
