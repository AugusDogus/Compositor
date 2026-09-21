use std::{fmt, sync::Arc};

use thiserror::Error;
use winit::keyboard::{Key as WinitKey, NamedKey};

use crate::{Key, Keystroke, Modifiers};

/// Maximum UTF-8 bytes retained for a native keyboard-layout identifier.
pub const MAX_KEYBOARD_LAYOUT_ID_BYTES: usize = 4 * 1024;
/// Maximum UTF-8 bytes retained for a native keyboard-layout display name.
pub const MAX_KEYBOARD_LAYOUT_NAME_BYTES: usize = 4 * 1024;

/// An immutable, cheaply cloned snapshot of the active keyboard layout.
///
/// QuickGUI replaces this value only at a native input-source notification boundary. Reading it
/// never queries the operating system and retaining it never keeps a native input-source object
/// alive.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct KeyboardLayout {
    id: Arc<str>,
    name: Arc<str>,
}

impl KeyboardLayout {
    /// Build a validated layout snapshot, primarily for deterministic platform adapters and
    /// tests.
    pub fn new(
        id: impl Into<Arc<str>>,
        name: impl Into<Arc<str>>,
    ) -> Result<Self, KeyboardLayoutError> {
        let id = id.into();
        let name = name.into();
        if id.is_empty() || id.len() > MAX_KEYBOARD_LAYOUT_ID_BYTES || id.contains('\0') {
            return Err(KeyboardLayoutError::InvalidId);
        }
        if name.is_empty() || name.len() > MAX_KEYBOARD_LAYOUT_NAME_BYTES || name.contains('\0') {
            return Err(KeyboardLayoutError::InvalidName);
        }
        Ok(Self { id, name })
    }

    /// Platform-defined stable identifier for this installed input layout.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Localized display name supplied by the platform.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Default for KeyboardLayout {
    fn default() -> Self {
        Self {
            id: Arc::from("unknown"),
            name: Arc::from("System keyboard"),
        }
    }
}

impl fmt::Debug for KeyboardLayout {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KeyboardLayout")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish()
    }
}

/// Validation failure while constructing a deterministic keyboard layout.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum KeyboardLayoutError {
    #[error("a keyboard-layout identifier must be non-empty, null-free, and at most 4096 bytes")]
    InvalidId,
    #[error("a keyboard-layout name must be non-empty, null-free, and at most 4096 bytes")]
    InvalidName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KeyboardState {
    layout: KeyboardLayout,
    #[cfg(target_os = "macos")]
    map: crate::macos_keyboard::MacKeyboardMap,
}

impl KeyboardState {
    pub(crate) fn native() -> Self {
        #[cfg(target_os = "macos")]
        {
            let (layout, map) = crate::macos_keyboard::native_keyboard();
            Self { layout, map }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self {
                layout: KeyboardLayout::default(),
            }
        }
    }

    pub(crate) fn layout(&self) -> &KeyboardLayout {
        &self.layout
    }

    pub(crate) fn key_equivalents(&self) -> &'static [(char, char)] {
        key_equivalents_for_layout(&self.layout)
    }

    pub(crate) fn keystroke(
        &self,
        event: &winit::event::KeyEvent,
        modifiers: Modifiers,
    ) -> Keystroke {
        #[cfg(target_os = "macos")]
        {
            use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

            let modifierless = event.key_without_modifiers();
            let fallback = map_key(&modifierless);
            if matches!(fallback, Key::Character(_) | Key::Other)
                && let Some(translation) = self.map.interpret(event.physical_key, modifiers)
            {
                return Keystroke::from_platform_event(
                    Key::Character(translation.key.to_owned()),
                    translation.modifiers,
                    translation
                        .key_char
                        .map(|value| Key::Character(value.to_owned())),
                );
            }

            let key = if modifiers.contains(Modifiers::SUPER) {
                map_key(&event.logical_key)
            } else {
                fallback
            };
            let key_char = if !modifiers.intersects(Modifiers::CONTROL | Modifiers::SUPER) {
                event
                    .text
                    .as_deref()
                    .filter(|text| !text.is_empty())
                    .map(|text| Key::Character(text.to_owned()))
            } else {
                None
            };
            Keystroke::from_platform_event(key, modifiers, key_char)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Keystroke::from_platform_event(map_key(&event.logical_key), modifiers, None)
        }
    }
}

pub(crate) fn key_equivalents_for_layout(layout: &KeyboardLayout) -> &'static [(char, char)] {
    #[cfg(target_os = "macos")]
    {
        crate::macos_key_equivalents::for_layout(layout.id())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = layout;
        &[]
    }
}

pub(crate) fn map_key(key: &WinitKey) -> Key {
    match key {
        WinitKey::Character(value) => Key::Character(value.to_string()),
        WinitKey::Named(NamedKey::ArrowUp) => Key::ArrowUp,
        WinitKey::Named(NamedKey::ArrowDown) => Key::ArrowDown,
        WinitKey::Named(NamedKey::ArrowLeft) => Key::ArrowLeft,
        WinitKey::Named(NamedKey::ArrowRight) => Key::ArrowRight,
        WinitKey::Named(NamedKey::PageUp) => Key::PageUp,
        WinitKey::Named(NamedKey::PageDown) => Key::PageDown,
        WinitKey::Named(NamedKey::Home) => Key::Home,
        WinitKey::Named(NamedKey::End) => Key::End,
        WinitKey::Named(NamedKey::Enter) => Key::Enter,
        WinitKey::Named(NamedKey::Escape) => Key::Escape,
        WinitKey::Named(NamedKey::Space) => Key::Space,
        WinitKey::Named(NamedKey::Tab) => Key::Tab,
        WinitKey::Named(NamedKey::Backspace) => Key::Backspace,
        WinitKey::Named(NamedKey::Delete) => Key::Delete,
        WinitKey::Named(NamedKey::Insert) => Key::Insert,
        WinitKey::Named(NamedKey::F1) => Key::Function(1),
        WinitKey::Named(NamedKey::F2) => Key::Function(2),
        WinitKey::Named(NamedKey::F3) => Key::Function(3),
        WinitKey::Named(NamedKey::F4) => Key::Function(4),
        WinitKey::Named(NamedKey::F5) => Key::Function(5),
        WinitKey::Named(NamedKey::F6) => Key::Function(6),
        WinitKey::Named(NamedKey::F7) => Key::Function(7),
        WinitKey::Named(NamedKey::F8) => Key::Function(8),
        WinitKey::Named(NamedKey::F9) => Key::Function(9),
        WinitKey::Named(NamedKey::F10) => Key::Function(10),
        WinitKey::Named(NamedKey::F11) => Key::Function(11),
        WinitKey::Named(NamedKey::F12) => Key::Function(12),
        WinitKey::Named(NamedKey::F13) => Key::Function(13),
        WinitKey::Named(NamedKey::F14) => Key::Function(14),
        WinitKey::Named(NamedKey::F15) => Key::Function(15),
        WinitKey::Named(NamedKey::F16) => Key::Function(16),
        WinitKey::Named(NamedKey::F17) => Key::Function(17),
        WinitKey::Named(NamedKey::F18) => Key::Function(18),
        WinitKey::Named(NamedKey::F19) => Key::Function(19),
        WinitKey::Named(NamedKey::F20) => Key::Function(20),
        WinitKey::Named(NamedKey::F21) => Key::Function(21),
        WinitKey::Named(NamedKey::F22) => Key::Function(22),
        WinitKey::Named(NamedKey::F23) => Key::Function(23),
        WinitKey::Named(NamedKey::F24) => Key::Function(24),
        _ => Key::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_validation_is_bounded() {
        let layout = KeyboardLayout::new("com.example.layout", "Example").unwrap();
        assert_eq!(layout.id(), "com.example.layout");
        assert_eq!(layout.name(), "Example");
        assert_eq!(
            KeyboardLayout::new("", "Example"),
            Err(KeyboardLayoutError::InvalidId)
        );
        assert_eq!(
            KeyboardLayout::new("layout", "bad\0name"),
            Err(KeyboardLayoutError::InvalidName)
        );
    }

    #[test]
    fn layout_clones_share_native_strings() {
        let layout = KeyboardLayout::new("layout", "Layout").unwrap();
        let clone = layout.clone();
        assert!(Arc::ptr_eq(&layout.id, &clone.id));
        assert!(Arc::ptr_eq(&layout.name, &clone.name));
    }
}
