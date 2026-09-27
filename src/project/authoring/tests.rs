use super::*;
use crate::adjustment::PhotoFilter;
use image::{Rgba, RgbaImage};

fn document() -> Document {
    let mut document = Document::new(3, 2).unwrap();
    document.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        3,
        2,
        Rgba([50, 100, 150, 255]),
    ))));
    let mut adjustment = Layer::blank("Photo Filter", 3, 2);
    adjustment.content = LayerContent::ExtendedAdjustment(Box::new(
        ExtendedAdjustment::PhotoFilter(PhotoFilter::default()),
    ));
    document.add(adjustment).unwrap();
    document
}

#[test]
fn editable_snapshot_and_native_projection_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Editable.comp");
    let document = document();
    super::super::save(&document, &path).unwrap();
    assert_eq!(super::super::load(&path).unwrap(), document);
    assert!(matches!(
        inspect_open(&path).unwrap(),
        OpenResult::Editable { .. }
    ));
    let projection = load_native(&path).unwrap();
    assert_eq!(projection.layers.len(), 1);
    assert_eq!(
        crate::render::render(&projection, 3, 2).unwrap(),
        crate::render::render(&document, 3, 2).unwrap()
    );
    assert!(
        compatibility_notice(&document)
            .unwrap()
            .contains("full resolution")
    );
}

#[test]
fn native_encoder_rejects_extended_settings_before_creating_assets() {
    let directory = tempfile::tempdir().unwrap();
    assert!(write_native(&document(), directory.path()).is_err());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn exact_bytes_bind_root_source_settings_and_assets() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Editable.comp");
    for target in [
        "manifest.json",
        "authoring/manifest.json",
        "authoring/adjustments.json",
        "images",
        "authoring/images",
    ] {
        super::super::save(&document(), &path).unwrap();
        let mut file = path.join(target);
        if file.is_dir() {
            file = fs::read_dir(file).unwrap().next().unwrap().unwrap().path();
        }
        let mut bytes = fs::read(&file).unwrap();
        // Trailing whitespace preserves JSON meaning and trailing PNG data
        // preserves image pixels. Binding still notices the exact-byte change.
        bytes.push(b' ');
        fs::write(file, bytes).unwrap();
        assert!(super::super::load(&path).is_err(), "{target}");
        let OpenResult::RenderedCopyAvailable { fingerprint, .. } = inspect_open(&path).unwrap()
        else {
            panic!("missing fallback for {target}");
        };
        assert_eq!(
            load_rendered_copy(&path, &fingerprint)
                .unwrap()
                .layers
                .len(),
            1
        );
    }
}

#[test]
fn unknown_version_missing_source_and_removed_binding_are_not_silently_accepted() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Editable.comp");
    for change in 0..3 {
        super::super::save(&document(), &path).unwrap();
        let mut index: serde_json::Value =
            serde_json::from_slice(&fs::read(path.join(NAME)).unwrap()).unwrap();
        match change {
            0 => index["version"] = 99.into(),
            1 => {
                fs::remove_file(path.join("authoring/adjustments.json")).unwrap();
            }
            _ => {
                index["files"].as_array_mut().unwrap().pop();
            }
        }
        fs::write(path.join(NAME), serde_json::to_vec(&index).unwrap()).unwrap();
        assert!(super::super::load(&path).is_err());
        assert!(matches!(
            inspect_open(&path).unwrap(),
            OpenResult::RenderedCopyAvailable { .. }
        ));
    }
}

#[test]
fn fallback_rejects_changes_after_warning() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Editable.comp");
    super::super::save(&document(), &path).unwrap();
    let before = fingerprint(&path).unwrap().unwrap();
    let mut bytes = fs::read(path.join("manifest.json")).unwrap();
    bytes.push(b' ');
    fs::write(path.join("manifest.json"), bytes).unwrap();
    assert!(load_rendered_copy(&path, &before).is_err());
}

#[test]
fn sparse_recovery_preserves_full_resolution_without_projection() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Recovery.comp");
    let mut document = document();
    document.width = 30_000;
    document.height = 30_000;
    super::super::save_recovery(&document, &path).unwrap();
    assert!(!path.join("images").exists());
    assert_eq!(super::super::load(&path).unwrap(), document);
    assert!(
        compatibility_notice(&document)
            .unwrap()
            .contains("14142 × 14142")
    );
    fs::write(path.join(NAME), b"{}").unwrap();
    assert!(inspect_open(&path).is_err());
    assert!(load_native(&path).is_err());
}

#[test]
fn failed_source_staging_preserves_existing_project() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Existing.comp");
    let original = document();
    super::super::save(&original, &path).unwrap();
    let before = fingerprint(&path).unwrap();
    let mut oversized = original.clone();
    for _ in 0..300 {
        oversized
            .add(Layer::blank("x".repeat(16_000), 3, 2))
            .unwrap();
    }
    oversized.validate().unwrap();
    assert!(super::super::save(&oversized, &path).is_err());
    assert_eq!(fingerprint(&path).unwrap(), before);
    assert_eq!(super::super::load(&path).unwrap(), original);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn orphaned_or_oversized_authoring_still_requires_explicit_rendered_copy() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Editable.comp");
    for mode in 0..4 {
        super::super::save(&document(), &path).unwrap();
        match mode {
            0 => fs::remove_file(path.join(NAME)).unwrap(),
            1 => File::create(path.join(NAME))
                .unwrap()
                .set_len(MANIFEST_LIMIT + 1)
                .unwrap(),
            2 => {
                fs::remove_file(path.join("authoring/adjustments.json")).unwrap();
                std::os::unix::fs::symlink("/missing", path.join("authoring/adjustments.json"))
                    .unwrap();
            }
            _ => {
                fs::remove_dir_all(path.join("authoring")).unwrap();
                std::os::unix::fs::symlink("/missing", path.join("authoring")).unwrap();
            }
        }
        assert!(super::super::load(&path).is_err());
        let OpenResult::RenderedCopyAvailable { fingerprint, .. } = inspect_open(&path).unwrap()
        else {
            panic!("Expected rendered copy warning");
        };
        load_rendered_copy(&path, &fingerprint).unwrap();
    }
}

#[test]
fn raw_sources_editor_hints_and_masks_live_only_in_bound_authoring_namespace() {
    use crate::raw::{DevelopSettings, RawAsset, RawMetadata};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Authoring.comp");
    let mut document = document();
    document.layers[0].raw = Some(Arc::new(RawAsset {
        filename: "camera.dng".into(),
        metadata: RawMetadata {
            width: 3,
            height: 2,
            ..Default::default()
        },
        settings: DevelopSettings::default(),
        bytes: Arc::new(vec![1, 2, 3, 4]),
    }));
    document.layers[1].mask = Some(Mask {
        pixels: Arc::new(image::GrayImage::from_pixel(3, 2, image::Luma([120]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    let mut brightness = Layer::blank("Brightness", 3, 2);
    brightness.content = LayerContent::Adjustment(Box::new(
        crate::adjustment::BrightnessContrast {
            brightness: 20.,
            contrast: -10.,
        }
        .adjustment()
        .unwrap(),
    ));
    document.add(brightness).unwrap();
    for target in ["linux-raw.json", "linux-editors.json", "raw", "mask"] {
        super::super::save(&document, &path).unwrap();
        assert_eq!(super::super::load(&path).unwrap(), document);
        assert!(!path.join("linux-raw.json").exists());
        assert!(!path.join("linux-editors.json").exists());
        let file = match target {
            "raw" => fs::read_dir(path.join("authoring/raw"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path(),
            "mask" => path
                .join("authoring/images")
                .join(asset_name(document.layers[1].id, true)),
            _ => path.join("authoring").join(target),
        };
        let before = fingerprint(&path).unwrap();
        let mut bytes = fs::read(&file).unwrap();
        bytes.push(b' ');
        fs::write(file, bytes).unwrap();
        assert_ne!(fingerprint(&path).unwrap(), before);
        assert!(super::super::load(&path).is_err());
        assert!(matches!(
            inspect_open(&path).unwrap(),
            OpenResult::RenderedCopyAvailable { .. }
        ));
    }
}

#[test]
fn grouped_clipped_filter_and_mask_survive_editable_save_and_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let mut document = document();
    let mut folder = Layer::blank("Folder", 3, 2);
    folder.content = LayerContent::Group;
    folder.opacity = 0.6;
    for layer in &mut document.layers {
        layer.parent = Some(folder.id);
    }
    document.layers[1].clip_source = Some(document.layers[0].id);
    document.layers[1].mask = Some(crate::document::Mask {
        pixels: Arc::new(image::GrayImage::from_fn(3, 2, |x, _| {
            image::Luma([(x * 127) as u8])
        })),
        enabled: true,
        linked: true,
        placement: None,
    });
    document.layers.insert(0, folder);
    let expected = crate::render::render(&document, 3, 2).unwrap();
    for recovery in [false, true] {
        let path = directory.path().join(format!("{recovery}.comp"));
        if recovery {
            super::super::save_recovery(&document, &path).unwrap();
        } else {
            super::super::save(&document, &path).unwrap();
        }
        let loaded = super::super::load(&path).unwrap();
        assert_eq!(loaded, document);
        assert_eq!(crate::render::render(&loaded, 3, 2).unwrap(), expected);
    }
}

fn add_path(document: &mut Document) {
    use crate::vector_path::{Anchor, BezierPath, Closure, SavedPath};
    document.paths.push(
        SavedPath::new(
            "Outline",
            BezierPath {
                anchors: vec![
                    Anchor {
                        outgoing: Some([0., 2.]),
                        ..Anchor::corner([0., 0.])
                    },
                    Anchor::corner([3., 2.]),
                ],
                closure: Closure::Open,
            },
        )
        .unwrap(),
    );
}

#[test]
fn paths_roundtrip_without_flattening_native_editable_layers() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Paths.comp");
    let mut document = Document::new(3, 2).unwrap();
    document.add(Layer::blank("Second layer", 3, 2)).unwrap();
    add_path(&mut document);
    super::super::save(&document, &path).unwrap();
    assert_eq!(super::super::load(&path).unwrap(), document);
    let native = load_native(&path).unwrap();
    let mut expected = document.clone();
    expected.paths.clear();
    assert_eq!(native, expected);
    assert!(
        compatibility_notice(&document)
            .unwrap()
            .contains("native editable layers")
    );
    let native_directory = tempfile::tempdir().unwrap();
    assert!(write_native(&document, native_directory.path()).is_err());
    assert_eq!(fs::read_dir(native_directory.path()).unwrap().count(), 0);
}

#[test]
fn paths_and_grouped_clipped_adjustments_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Mixed.comp");
    let mut document = document();
    add_path(&mut document);
    let mut folder = Layer::blank("Folder", 3, 2);
    folder.content = LayerContent::Group;
    let folder_id = folder.id;
    for layer in &mut document.layers {
        layer.parent = Some(folder_id);
    }
    document.layers[1].clip_source = Some(document.layers[0].id);
    document.layers.insert(0, folder);
    document.validate().unwrap();
    super::super::save(&document, &path).unwrap();
    assert_eq!(super::super::load(&path).unwrap(), document);
    assert_eq!(load_native(&path).unwrap().layers.len(), 1);
}

#[test]
fn paths_only_sparse_recovery_needs_no_projection() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Paths.comp");
    let mut document = Document::new(30_000, 30_000).unwrap();
    add_path(&mut document);
    super::super::save_recovery(&document, &path).unwrap();
    assert!(!path.join("images").exists());
    assert_eq!(super::super::load(&path).unwrap(), document);
    assert!(load_native(&path).is_err());
}

#[test]
fn legacy_adjustments_snapshot_remains_readable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Legacy.comp");
    let document = document();
    super::super::save(&document, &path).unwrap();
    let settings_path = path.join(SOURCE).join(SETTINGS);
    let mut settings: serde_json::Value =
        serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    settings["version"] = 1.into();
    settings.as_object_mut().unwrap().remove("paths");
    write_json(&settings_path, &settings).unwrap();
    write_json(
        &path.join(NAME),
        &Index {
            version: 1,
            purpose: Purpose::Project,
            files: bound_files(&path).unwrap(),
        },
    )
    .unwrap();
    assert_eq!(super::super::load(&path).unwrap(), document);
}

#[test]
fn bound_but_invalid_path_sources_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("InvalidPaths.comp");
    let mut document = Document::new(3, 2).unwrap();
    add_path(&mut document);
    for case in 0..5 {
        super::super::save(&document, &path).unwrap();
        let settings_path = path.join(SOURCE).join(SETTINGS);
        let mut settings: serde_json::Value =
            serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
        match case {
            0 => settings["paths"][0]["name"] = "".into(),
            1 => settings["paths"][0]["id"] = Uuid::nil().to_string().into(),
            2 => {
                let duplicate = settings["paths"][0].clone();
                settings["paths"].as_array_mut().unwrap().push(duplicate);
            }
            3 => settings["paths"][0]["geometry"]["anchors"][0]["point"][0] = 1_000_001.into(),
            _ => settings["version"] = 1.into(),
        }
        write_json(&settings_path, &settings).unwrap();
        write_json(
            &path.join(NAME),
            &Index {
                version: 1,
                purpose: Purpose::Project,
                files: bound_files(&path).unwrap(),
            },
        )
        .unwrap();
        assert!(super::super::load(&path).is_err(), "case {case}");
        let OpenResult::RenderedCopyAvailable { fingerprint, .. } = inspect_open(&path).unwrap()
        else {
            panic!("missing explicit fallback")
        };
        let fallback = load_rendered_copy(&path, &fingerprint).unwrap();
        assert!(fallback.paths.is_empty());
        assert_eq!(fallback.layers, document.layers);
    }
}
