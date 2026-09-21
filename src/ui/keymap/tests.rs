use super::*;
fn definition(title: &str) -> Definition {
    definitions()
        .into_iter()
        .find(|d| d.title == title)
        .unwrap()
}
#[test]
fn default_shortcuts_have_unique_valid_chords_and_rebinding_replaces_old_keys() {
    let mut keymap = Keymap::default();
    keymap.validate().unwrap();
    let brush = definition("Brush");
    keymap.set(&brush, Chord::parse("F2").unwrap());
    keymap.validate().unwrap();
    assert_eq!(
        keymap.translate(&Key::Function(2), Modifiers::empty(), false),
        Some((Key::Character("b".into()), Modifiers::empty()))
    );
    assert!(
        keymap
            .translate(&Key::Character("b".into()), Modifiers::empty(), false)
            .is_none()
    );
    assert!(
        keymap
            .translate(&Key::Character("B".into()), Modifiers::SHIFT, false)
            .is_none()
    );
    assert_eq!(
        keymap.translate(&Key::Character("b".into()), Modifiers::empty(), true),
        Some((Key::Character("b".into()), Modifiers::empty()))
    );
    let save = definitions()
        .into_iter()
        .find(|d| d.title == "Save")
        .unwrap();
    keymap.set(&save, Chord::parse("Ctrl+F2").unwrap());
    assert_eq!(keymap.menu_label("Ctrl+S"), "Ctrl+F2");
    assert_eq!(
        keymap.translate(&Key::Function(2), Modifiers::CONTROL, false),
        Some((Key::Character("s".into()), Modifiers::CONTROL))
    );
}
#[test]
fn conflicts_reserved_keys_invalid_files_and_text_typing_collisions_are_rejected() {
    let mut keymap = Keymap::default();
    let brush = definition("Brush");
    for chord in ["V", "Ctrl+Q", "Alt+F", "F10"] {
        keymap.set(&brush, Chord::parse(chord).unwrap());
        assert!(keymap.validate().is_err(), "{chord}");
    }
    keymap = Keymap::default();
    keymap.set(&definition("Apply text"), Chord::parse("F2").unwrap());
    assert!(keymap.validate().is_err());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("keys.json");
    std::fs::write(&path, "{\"unknown\":{\"key\":\"q\",\"modifiers\":1}}").unwrap();
    assert!(Keymap::read(&path).is_err());
    assert!(Chord::parse("Ctrl+not-a-key").is_err());
}
#[test]
fn settings_save_roundtrip_and_custom_pan_releases_without_modifiers() {
    let mut keymap = Keymap::default();
    keymap.set(
        &definition("Temporary Hand (hold)"),
        Chord::parse("Ctrl+F3").unwrap(),
    );
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("keys.json");
    keymap.save(&path).unwrap();
    let loaded = Keymap::read(&path).unwrap();
    assert!(loaded.is_pan(&Key::Function(3), Modifiers::CONTROL));
    assert!(loaded.pan_released(&Key::Function(3)));
    assert!(!loaded.is_pan(&Key::Space, Modifiers::empty()));
}

#[test]
fn invalid_shortcut_save_preserves_previous_settings() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("keys.json");
    let mut keymap = Keymap::default();
    keymap.save(&path).unwrap();
    let original = std::fs::read(&path).unwrap();
    keymap.set(&definition("Brush"), Chord::parse("V").unwrap());
    assert!(keymap.save(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
}
