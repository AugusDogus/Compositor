use super::*;
use crate::raw::{DevelopSettings, RawAsset, RawMetadata};

fn document() -> Document {
    let mut doc = Document::new(2, 2).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(image::RgbaImage::from_pixel(
        2,
        2,
        image::Rgba([80, 100, 120, 255]),
    ))));
    doc.layers[0].raw = Some(Arc::new(RawAsset {
        filename: "camera.dng".into(),
        metadata: RawMetadata {
            width: 2,
            height: 2,
            ..Default::default()
        },
        settings: DevelopSettings::default(),
        bytes: Arc::new(vec![1, 2, 3, 4]),
    }));
    doc
}

#[test]
fn changed_cached_pixels_cannot_reuse_stale_raw_source() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.comp");
    let doc = document();
    crate::project::save(&doc, &path).unwrap();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([200_u8, 50, 20, 255]))
        .save(
            path.join("images")
                .join(asset_name(doc.layers[0].id, false)),
        )
        .unwrap();
    assert!(crate::project::load(&path).is_err());
}

#[test]
fn raw_sources_roundtrip_and_stay_shared_after_layer_duplication() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.comp");
    let mut doc = document();
    crate::layer_ops::duplicate_active(&mut doc).unwrap();
    crate::project::save(&doc, &path).unwrap();
    let loaded = crate::project::load(&path).unwrap();
    assert_eq!(loaded.layers, doc.layers);
    assert!(Arc::ptr_eq(
        &loaded.layers[0].raw.as_ref().unwrap().bytes,
        &loaded.layers[1].raw.as_ref().unwrap().bytes,
    ));
}

#[test]
fn modified_source_or_develop_settings_cannot_reuse_cached_pixels() {
    for source_change in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.comp");
        let doc = document();
        crate::project::save(&doc, &path).unwrap();
        if source_change {
            fs::write(source_path(&path, doc.layers[0].id), [4, 3, 2, 1]).unwrap();
        } else {
            let metadata = path.join(NAME);
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
            value["layers"][0]["asset"]["settings"]["exposure"] = 2.into();
            fs::write(metadata, serde_json::to_vec(&value).unwrap()).unwrap();
        }
        assert!(crate::project::load(&path).is_err());
    }
}

#[test]
fn placement_edits_keep_valid_raw_sources() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.comp");
    let doc = document();
    crate::project::save(&doc, &path).unwrap();
    let manifest = path.join("manifest.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    value["layers"][0]["name"] = "Moved camera layer".into();
    value["layers"][0]["opacity"] = 0.5.into();
    fs::write(manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    let loaded = crate::project::load(&path).unwrap();
    assert_eq!(loaded.layers[0].raw, doc.layers[0].raw);
    assert_eq!(loaded.layers[0].opacity, 0.5);
}

#[test]
fn legacy_sources_load_and_upgrade_on_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.comp");
    let doc = document();
    crate::project::save(&doc, &path).unwrap();
    let metadata = path.join(NAME);
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
    value["version"] = 1.into();
    value["layers"][0]
        .as_object_mut()
        .unwrap()
        .remove("binding");
    fs::write(&metadata, serde_json::to_vec(&value).unwrap()).unwrap();
    let loaded = crate::project::load(&path).unwrap();
    assert_eq!(loaded.layers, doc.layers);
    crate::project::save(&loaded, &path).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&fs::read(metadata).unwrap()).unwrap();
    assert_eq!(value["version"], 2);
    assert!(value["layers"][0]["binding"].is_array());
}

#[test]
fn new_sidecar_requires_binding_and_cannot_be_copied_to_another_document() {
    for missing_binding in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.comp");
        crate::project::save(&document(), &path).unwrap();
        let file = path.join(if missing_binding {
            NAME
        } else {
            "manifest.json"
        });
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        if missing_binding {
            value["layers"][0]
                .as_object_mut()
                .unwrap()
                .remove("binding");
        } else {
            value["documentID"] = Uuid::new_v4().to_string().into();
        }
        fs::write(file, serde_json::to_vec(&value).unwrap()).unwrap();
        let error = crate::project::load(&path).unwrap_err().to_string();
        assert!(error.contains("no longer match"), "{error}");
    }
}
