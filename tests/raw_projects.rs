use compositor::{
    document::{Document, LayerContent},
    project,
    raw::{DevelopSettings, RawAsset, RawMetadata},
};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

fn document() -> Document {
    let mut doc = Document::new(8, 6).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        8,
        6,
        Rgba([80, 120, 160, 255]),
    ))));
    // Persistence stores opaque source bytes; decoding is independently tested against camera files.
    doc.layers[0].raw = Some(Arc::new(RawAsset {
        filename: "Camera.NEF".into(),
        metadata: RawMetadata {
            width: 8,
            height: 6,
            bits: 14,
            camera: "Nikon test".into(),
            ..Default::default()
        },
        settings: DevelopSettings {
            exposure: 0.75,
            ..Default::default()
        },
        bytes: Arc::new(vec![73, 73, 42, 0, 1, 2, 3, 4]),
    }));
    doc
}

#[test]
fn raw_sources_and_independent_settings_roundtrip_without_changing_upstream_manifest() {
    let mut doc = document();
    compositor::layer_ops::duplicate_active(&mut doc).unwrap();
    let asset = Arc::make_mut(doc.layers[1].raw.as_mut().unwrap());
    asset.settings.exposure = -1.25;
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("Editable.comp");
    project::save(&doc, &path).unwrap();
    assert_eq!(std::fs::read_dir(path.join("raw")).unwrap().count(), 1);
    let loaded = project::load(&path).unwrap();
    assert_eq!(loaded, doc);
    assert!(Arc::ptr_eq(
        &loaded.layers[0].raw.as_ref().unwrap().bytes,
        &loaded.layers[1].raw.as_ref().unwrap().bytes
    ));
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["version"], 8);
    assert!(manifest["layers"][0].get("raw").is_none());
    // A reader without the extension still receives all developed pixels.
    std::fs::remove_file(path.join("linux-raw.json")).unwrap();
    let flattened = project::load(&path).unwrap();
    assert!(flattened.layers.iter().all(|layer| layer.raw.is_none()));
    assert_eq!(flattened.layers[0].raster(), doc.layers[0].raster());
}

#[test]
fn invalid_raw_settings_preserve_previous_save_and_unsafe_sources_are_rejected() {
    let mut doc = document();
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("Editable.comp");
    project::save(&doc, &path).unwrap();
    let before = project::load(&path).unwrap();
    Arc::make_mut(doc.layers[0].raw.as_mut().unwrap())
        .settings
        .exposure = f32::NAN;
    assert!(project::save(&doc, &path).is_err());
    assert_eq!(project::load(&path).unwrap(), before);
    let raw_file = std::fs::read_dir(path.join("raw"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::remove_file(&raw_file).unwrap();
    std::os::unix::fs::symlink("/etc/hostname", raw_file).unwrap();
    assert!(project::load(&path).is_err());
}

#[test]
fn rasterizing_is_undoable_and_pixel_edits_do_not_silently_discard_raw() {
    let doc = document();
    let mut candidate = doc.clone();
    assert!(compositor::edits::fill(&mut candidate, [255; 4], false, false).is_err());
    assert_eq!(candidate, doc);
    assert!(
        compositor::filters::apply(
            &mut candidate,
            compositor::filters::Filter::Gaussian { radius: 2. },
            false
        )
        .is_err()
    );
    assert_eq!(candidate, doc);
    let mut session = compositor::session::Session::new(doc.clone(), None);
    session
        .edit("Rasterize RAW Layer", |doc| {
            doc.layers[0].raw = None;
            Ok(())
        })
        .unwrap();
    assert!(session.document.layers[0].raw.is_none());
    session.undo();
    assert_eq!(session.document, doc);
    // Mask editing is independent of the sensor source.
    compositor::edits::add_mask(&mut candidate, false).unwrap();
    compositor::edits::fill(&mut candidate, [0, 0, 0, 255], false, true).unwrap();
    assert!(candidate.layers[0].raw.is_some());
}

#[test]
fn image_size_preserves_editable_raw_geometry_and_rejects_unrepresentable_shear_atomically() {
    use compositor::geometry::Sampling;
    for (rotation, width, height) in [(0., 16, 18), (90., 16, 18), (37., 16, 12)] {
        let mut doc = document();
        doc.layers[0].transform.rotation = rotation;
        doc.layers[0].transform.flip_x = true;
        let source = doc.layers[0].raster().unwrap().clone();
        let before = doc.layers[0].transform;
        compositor::image_resize::resize(&mut doc, width, height, 72., Sampling::High).unwrap();
        assert!(doc.layers[0].raw.is_some());
        assert!(Arc::ptr_eq(&source, doc.layers[0].raster().unwrap()));
        for p in [[0., 0.], [0., 1.], [1., 0.], [1., 1.]] {
            let old = before.point(p);
            let actual = doc.layers[0].transform.point(p);
            assert!((actual[0] - old[0] * f64::from(width) / 8.).abs() < 1e-8);
            assert!((actual[1] - old[1] * f64::from(height) / 6.).abs() < 1e-8);
        }
    }
    let mut doc = document();
    doc.layers[0].transform.rotation = 37.;
    let before = doc.clone();
    assert!(compositor::image_resize::resize(&mut doc, 16, 18, 72., Sampling::High).is_err());
    assert_eq!(doc, before);
}
