use super::*;
use crate::adjustment::{Adjustment, BrightnessContrast, Kind};
use crate::document::Layer;

fn fixture() -> Document {
    let mut doc = Document::new(3, 2).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(image::RgbaImage::from_fn(3, 2, |x, y| {
            image::Rgba([40 + x as u8 * 60, 70 + y as u8 * 80, 120, 90 + x as u8 * 70])
        }))));
    let mut layer = Layer::blank("Unrelated name", 3, 2);
    layer.content = LayerContent::Adjustment(Box::new(
        BrightnessContrast {
            brightness: 42.5,
            contrast: -17.25,
        }
        .adjustment()
        .unwrap(),
    ));
    doc.add(layer).unwrap();
    doc
}

fn settings(doc: &Document) -> &Adjustment {
    let LayerContent::Adjustment(settings) = &doc.active_layer().unwrap().content else {
        panic!("adjustment missing");
    };
    settings
}

#[test]
fn hints_roundtrip_while_native_projection_remains_editable_levels() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("test.comp");
    let doc = fixture();
    crate::project::save(&doc, &path).unwrap();
    let reopened = crate::project::load(&path).unwrap();
    assert_eq!(settings(&doc), settings(&reopened));
    let manifest = fs::read_to_string(path.join("manifest.json")).unwrap();
    assert!(!manifest.contains("editor"));
    assert!(!manifest.contains("BrightnessContrast"));
    let with_hint = crate::project::fingerprint(&path).unwrap();
    fs::remove_file(path.join(NAME)).unwrap();
    assert_ne!(crate::project::fingerprint(&path).unwrap(), with_hint);
    let native = crate::project::load(&path).unwrap();
    assert_eq!(settings(&native).kind, Kind::Levels);
    assert!(settings(&native).brightness_contrast().is_none());
    assert_eq!(settings(&native).levels, settings(&doc).levels);
    assert_eq!(
        crate::render::render(&native, 3, 2).unwrap(),
        crate::render::render(&doc, 3, 2).unwrap()
    );
}

#[test]
fn stale_hints_cannot_change_native_settings() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("test.comp");
    let doc = fixture();
    crate::project::save(&doc, &path).unwrap();
    let mut editors: serde_json::Value =
        serde_json::from_slice(&fs::read(path.join(NAME)).unwrap()).unwrap();
    for value in [serde_json::json!(10.), serde_json::json!(999.)] {
        editors["layers"][0]["editor"]["settings"]["brightness"] = value;
        fs::write(path.join(NAME), serde_json::to_vec(&editors).unwrap()).unwrap();
        let loaded = crate::project::load(&path).unwrap();
        assert_eq!(settings(&loaded).levels, settings(&doc).levels);
        assert!(settings(&loaded).brightness_contrast().is_none());
    }
}

#[test]
fn malformed_hints_fail_with_a_recovery_action_and_preserve_native_data() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("test.comp");
    crate::project::save(&fixture(), &path).unwrap();
    let before = fs::read(path.join("manifest.json")).unwrap();
    fs::write(path.join(NAME), b"not json").unwrap();
    let error = crate::project::load(&path).unwrap_err().to_string();
    assert!(error.contains("Remove linux-editors.json"));
    assert_eq!(fs::read(path.join("manifest.json")).unwrap(), before);
    fs::remove_file(path.join(NAME)).unwrap();
    crate::project::load(&path).unwrap();
}

#[test]
fn duplication_history_and_recovery_packages_retain_hints() {
    let mut session = crate::session::Session::new(fixture(), None);
    let original = session.document.clone();
    session.duplicate().unwrap();
    assert_ne!(session.document.active, original.active);
    assert_eq!(settings(&session.document), settings(&original));
    session.undo();
    assert_eq!(session.document, original);
    session.redo();
    let temp = tempfile::tempdir().unwrap();
    let recovery = temp.path().join("recovery.comp");
    crate::project::save(&session.document, &recovery).unwrap();
    let recovered = crate::project::load(&recovery).unwrap();
    assert_eq!(settings(&recovered), settings(&original));
    assert_eq!(recovered.layers.len(), session.document.layers.len());
}
