use std::{ffi::c_void, str};

use core_foundation::{
    base::CFRelease,
    data::{CFDataGetBytePtr, CFDataRef},
};
use objc2_foundation::NSString;
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::{KeyboardLayout, Modifiers};

const KEY_COUNT: usize = 128;
const UTF16_CAPACITY: usize = 4;
const UTF8_CAPACITY: usize = UTF16_CAPACITY * 4;

const NO_MOD: u32 = 0;
const COMMAND_MOD: u32 = 1;
const SHIFT_MOD: u32 = 2;
const OPTION_MOD: u32 = 8;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct InlineKeyText {
    bytes: [u8; UTF8_CAPACITY],
    len: u8,
}

impl InlineKeyText {
    fn from_utf16(value: &[u16]) -> Self {
        let mut result = Self::default();
        for character in char::decode_utf16(value.iter().copied()) {
            let Ok(character) = character else {
                return Self::default();
            };
            let offset = usize::from(result.len);
            let mut encoded = [0_u8; 4];
            let encoded = character.encode_utf8(&mut encoded).as_bytes();
            let Some(end) = offset.checked_add(encoded.len()) else {
                return Self::default();
            };
            if end > UTF8_CAPACITY {
                return Self::default();
            }
            result.bytes[offset..end].copy_from_slice(encoded);
            result.len = end as u8;
        }
        result
    }

    fn as_str(&self) -> &str {
        // Construction copies only validated UTF-8 and `len` never exceeds the fixed buffer.
        unsafe { str::from_utf8_unchecked(&self.bytes[..usize::from(self.len)]) }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct KeyTranslation {
    plain: InlineKeyText,
    shifted: InlineKeyText,
    command: InlineKeyText,
    command_shifted: InlineKeyText,
    option: InlineKeyText,
    option_shifted: InlineKeyText,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MacKeyboardMap {
    keys: Box<[KeyTranslation; KEY_COUNT]>,
    always_use_command_layout: bool,
}

pub(crate) struct MacKeyInterpretation<'a> {
    pub(crate) key: &'a str,
    pub(crate) key_char: Option<&'a str>,
    pub(crate) modifiers: Modifiers,
}

impl MacKeyboardMap {
    pub(crate) fn interpret(
        &self,
        physical_key: PhysicalKey,
        mut modifiers: Modifiers,
    ) -> Option<MacKeyInterpretation<'_>> {
        let scancode = usize::from(scancode(physical_key)?);
        let translation = &self.keys[scancode];
        let command = modifiers.contains(Modifiers::SUPER);
        let shift = modifiers.contains(Modifiers::SHIFT);
        let use_command_layout = command || self.always_use_command_layout;
        let base = if use_command_layout {
            translation.command.as_str()
        } else {
            translation.plain.as_str()
        };
        let base = if base.is_empty() {
            translation.plain.as_str()
        } else {
            base
        };
        if base.is_empty() {
            return None;
        }
        let shifted = if use_command_layout {
            translation.command_shifted.as_str()
        } else {
            translation.shifted.as_str()
        };
        let key = if shift && base.chars().all(|character| character.is_ascii_lowercase()) {
            base
        } else if shift && !shifted.is_empty() {
            modifiers.remove(Modifiers::SHIFT);
            shifted
        } else {
            base
        };

        let key_char = (!modifiers.intersects(Modifiers::CONTROL | Modifiers::SUPER))
            .then(|| {
                if modifiers.contains(Modifiers::ALT) {
                    if shift {
                        translation.option_shifted.as_str()
                    } else {
                        translation.option.as_str()
                    }
                } else if shift {
                    translation.shifted.as_str()
                } else {
                    translation.plain.as_str()
                }
            })
            .filter(|value| !value.is_empty());

        Some(MacKeyInterpretation {
            key,
            key_char,
            modifiers,
        })
    }
}

pub(crate) fn native_keyboard() -> (KeyboardLayout, MacKeyboardMap) {
    let mut layout = KeyboardLayout::default();
    let mut keys = Box::new([KeyTranslation::default(); KEY_COUNT]);

    let source = unsafe { TISCopyCurrentKeyboardLayoutInputSource() };
    if source.is_null() {
        tracing::warn!("could not read the current macOS keyboard layout");
        return (
            layout,
            MacKeyboardMap {
                keys,
                always_use_command_layout: false,
            },
        );
    }

    if let (Some(id), Some(name)) = unsafe {
        (
            input_source_string(source, kTISPropertyInputSourceID),
            input_source_string(source, kTISPropertyLocalizedName),
        )
    } && let Ok(native_layout) = KeyboardLayout::new(id, name)
    {
        layout = native_layout;
    }

    let layout_data =
        unsafe { TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) as CFDataRef };
    if !layout_data.is_null() {
        let keyboard_layout = unsafe { CFDataGetBytePtr(layout_data) };
        if !keyboard_layout.is_null() {
            let keyboard_type = u32::from(unsafe { LMGetKbdType() });
            for (code, translation) in keys.iter_mut().enumerate() {
                let code = code as u16;
                translation.plain =
                    translate_key(keyboard_layout.cast(), code, NO_MOD, keyboard_type);
                translation.shifted =
                    translate_key(keyboard_layout.cast(), code, SHIFT_MOD, keyboard_type);
                translation.command =
                    translate_key(keyboard_layout.cast(), code, COMMAND_MOD, keyboard_type);
                translation.command_shifted = translate_key(
                    keyboard_layout.cast(),
                    code,
                    COMMAND_MOD | SHIFT_MOD,
                    keyboard_type,
                );
                translation.option =
                    translate_key(keyboard_layout.cast(), code, OPTION_MOD, keyboard_type);
                translation.option_shifted = translate_key(
                    keyboard_layout.cast(),
                    code,
                    OPTION_MOD | SHIFT_MOD,
                    keyboard_type,
                );
            }
        }
    }
    unsafe { CFRelease(source.cast()) };

    let first = &keys[0];
    let always_use_command_layout = !first.plain.as_str().is_ascii()
        && !first.command.as_str().is_empty()
        && first.command.as_str().is_ascii();
    (
        layout,
        MacKeyboardMap {
            keys,
            always_use_command_layout,
        },
    )
}

unsafe fn input_source_string(source: *mut c_void, property: *const c_void) -> Option<String> {
    let value = unsafe { TISGetInputSourceProperty(source, property) } as *const NSString;
    unsafe { value.as_ref() }.map(ToString::to_string)
}

fn translate_key(
    layout: *const c_void,
    code: u16,
    modifiers: u32,
    keyboard_type: u32,
) -> InlineKeyText {
    const KEY_ACTION_DOWN: u16 = 0;
    const SPACE_KEY: u16 = 49;
    const NO_OPTIONS: u32 = 0;

    let mut dead_key_state = 0_u32;
    let mut buffer = [0_u16; UTF16_CAPACITY];
    let mut length = 0_usize;
    let status = unsafe {
        UCKeyTranslate(
            layout,
            code,
            KEY_ACTION_DOWN,
            modifiers,
            keyboard_type,
            NO_OPTIONS,
            &mut dead_key_state,
            UTF16_CAPACITY,
            &mut length,
            buffer.as_mut_ptr(),
        )
    };
    if status != 0 || length > UTF16_CAPACITY {
        return InlineKeyText::default();
    }
    if dead_key_state != 0 {
        length = 0;
        let status = unsafe {
            UCKeyTranslate(
                layout,
                SPACE_KEY,
                KEY_ACTION_DOWN,
                modifiers,
                keyboard_type,
                NO_OPTIONS,
                &mut dead_key_state,
                UTF16_CAPACITY,
                &mut length,
                buffer.as_mut_ptr(),
            )
        };
        if status != 0 || length > UTF16_CAPACITY {
            return InlineKeyText::default();
        }
    }
    InlineKeyText::from_utf16(&buffer[..length])
}

fn scancode(physical_key: PhysicalKey) -> Option<u8> {
    let PhysicalKey::Code(code) = physical_key else {
        return None;
    };
    Some(match code {
        KeyCode::KeyA => 0x00,
        KeyCode::KeyS => 0x01,
        KeyCode::KeyD => 0x02,
        KeyCode::KeyF => 0x03,
        KeyCode::KeyH => 0x04,
        KeyCode::KeyG => 0x05,
        KeyCode::KeyZ => 0x06,
        KeyCode::KeyX => 0x07,
        KeyCode::KeyC => 0x08,
        KeyCode::KeyV => 0x09,
        KeyCode::KeyB => 0x0b,
        KeyCode::KeyQ => 0x0c,
        KeyCode::KeyW => 0x0d,
        KeyCode::KeyE => 0x0e,
        KeyCode::KeyR => 0x0f,
        KeyCode::KeyY => 0x10,
        KeyCode::KeyT => 0x11,
        KeyCode::Digit1 => 0x12,
        KeyCode::Digit2 => 0x13,
        KeyCode::Digit3 => 0x14,
        KeyCode::Digit4 => 0x15,
        KeyCode::Digit6 => 0x16,
        KeyCode::Digit5 => 0x17,
        KeyCode::Equal => 0x18,
        KeyCode::Digit9 => 0x19,
        KeyCode::Digit7 => 0x1a,
        KeyCode::Minus => 0x1b,
        KeyCode::Digit8 => 0x1c,
        KeyCode::Digit0 => 0x1d,
        KeyCode::BracketRight => 0x1e,
        KeyCode::KeyO => 0x1f,
        KeyCode::KeyU => 0x20,
        KeyCode::BracketLeft => 0x21,
        KeyCode::KeyI => 0x22,
        KeyCode::KeyP => 0x23,
        KeyCode::KeyL => 0x25,
        KeyCode::KeyJ => 0x26,
        KeyCode::Quote => 0x27,
        KeyCode::KeyK => 0x28,
        KeyCode::Semicolon => 0x29,
        KeyCode::Backslash => 0x2a,
        KeyCode::Comma => 0x2b,
        KeyCode::Slash => 0x2c,
        KeyCode::KeyN => 0x2d,
        KeyCode::KeyM => 0x2e,
        KeyCode::Period => 0x2f,
        KeyCode::Space => 0x31,
        KeyCode::Backquote => 0x32,
        KeyCode::NumpadDecimal => 0x41,
        KeyCode::NumpadMultiply => 0x43,
        KeyCode::NumpadAdd => 0x45,
        KeyCode::NumpadDivide => 0x4b,
        KeyCode::NumpadSubtract => 0x4e,
        KeyCode::NumpadEqual => 0x51,
        KeyCode::Numpad0 => 0x52,
        KeyCode::Numpad1 => 0x53,
        KeyCode::Numpad2 => 0x54,
        KeyCode::Numpad3 => 0x55,
        KeyCode::Numpad4 => 0x56,
        KeyCode::Numpad5 => 0x57,
        KeyCode::Numpad6 => 0x58,
        KeyCode::Numpad7 => 0x59,
        KeyCode::Numpad8 => 0x5b,
        KeyCode::Numpad9 => 0x5c,
        _ => return None,
    })
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut c_void;
    fn TISGetInputSourceProperty(
        input_source: *mut c_void,
        property_key: *const c_void,
    ) -> *mut c_void;
    fn UCKeyTranslate(
        key_layout: *const c_void,
        virtual_key_code: u16,
        key_action: u16,
        modifier_key_state: u32,
        keyboard_type: u32,
        key_translate_options: u32,
        dead_key_state: *mut u32,
        max_string_length: usize,
        actual_string_length: *mut usize,
        unicode_string: *mut u16,
    ) -> i32;
    fn LMGetKbdType() -> u8;
    static kTISPropertyUnicodeKeyLayoutData: *const c_void;
    static kTISPropertyInputSourceID: *const c_void;
    static kTISPropertyLocalizedName: *const c_void;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> InlineKeyText {
        InlineKeyText::from_utf16(&value.encode_utf16().collect::<Vec<_>>())
    }

    fn map(translation: KeyTranslation, command_layout: bool) -> MacKeyboardMap {
        let mut keys = Box::new([KeyTranslation::default(); KEY_COUNT]);
        keys[0] = translation;
        MacKeyboardMap {
            keys,
            always_use_command_layout: command_layout,
        }
    }

    #[test]
    fn interpretation_separates_command_key_from_option_text() {
        let map = map(
            KeyTranslation {
                plain: text("a"),
                shifted: text("A"),
                command: text("a"),
                command_shifted: text("A"),
                option: text("å"),
                option_shifted: text("Å"),
            },
            false,
        );
        let option = map
            .interpret(PhysicalKey::Code(KeyCode::KeyA), Modifiers::ALT)
            .unwrap();
        assert_eq!(option.key, "a");
        assert_eq!(option.key_char, Some("å"));
        assert_eq!(option.modifiers, Modifiers::ALT);

        let command = map
            .interpret(PhysicalKey::Code(KeyCode::KeyA), Modifiers::SUPER)
            .unwrap();
        assert_eq!(command.key, "a");
        assert_eq!(command.key_char, None);
    }

    #[test]
    fn interpretation_keeps_letter_shift_but_consumes_symbol_shift() {
        let letters = map(
            KeyTranslation {
                plain: text("a"),
                shifted: text("A"),
                command: text("a"),
                command_shifted: text("A"),
                ..KeyTranslation::default()
            },
            false,
        );
        let shifted = letters
            .interpret(PhysicalKey::Code(KeyCode::KeyA), Modifiers::SHIFT)
            .unwrap();
        assert_eq!(shifted.key, "a");
        assert_eq!(shifted.modifiers, Modifiers::SHIFT);

        let symbols = map(
            KeyTranslation {
                plain: text("."),
                shifted: text(">"),
                command: text("."),
                command_shifted: text(">"),
                ..KeyTranslation::default()
            },
            false,
        );
        let shifted = symbols
            .interpret(PhysicalKey::Code(KeyCode::KeyA), Modifiers::SHIFT)
            .unwrap();
        assert_eq!(shifted.key, ">");
        assert_eq!(shifted.modifiers, Modifiers::empty());
    }

    #[test]
    fn non_ascii_layout_can_use_the_command_translation_for_shortcuts() {
        let map = map(
            KeyTranslation {
                plain: text("ս"),
                shifted: text("Ս"),
                command: text("s"),
                command_shifted: text("S"),
                ..KeyTranslation::default()
            },
            true,
        );
        let stroke = map
            .interpret(PhysicalKey::Code(KeyCode::KeyA), Modifiers::SUPER)
            .unwrap();
        assert_eq!(stroke.key, "s");
    }
}
