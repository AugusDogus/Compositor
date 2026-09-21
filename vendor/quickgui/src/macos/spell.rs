//! `NSSpellChecker` and `NSView` dictionary services.
//!
//! `NSSpellChecker` is outside the bound `objc2-app-kit` feature set, so it is driven with
//! `objc2::class!` and `msg_send!`. Every entry point requires the main thread and degrades to the
//! portable behavior — no flagged ranges, no guesses, `Unsupported` lookups — off it.
//!
//! AppKit reports ranges in UTF-16 code units while QuickGUI edits UTF-8 byte offsets, so all
//! offsets cross the boundary through explicit conversions.

use std::{cell::RefCell, ops::Range, rc::Rc};

use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
use objc2_app_kit::{NSApplication, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRange, NSString};

use crate::{
    Point,
    spell::{
        MAX_SPELL_GUESSES, MAX_SPELL_WORD_BYTES, Misspelling, SpellCheckProvider, SpellDocumentTag,
        TextCheckingPolicy, TextServiceError, TextSubstitution,
    },
};

/// The process-wide `NSSpellChecker`-backed provider.
pub(crate) fn shared_provider() -> Rc<dyn SpellCheckProvider> {
    thread_local! {
        static PROVIDER: RefCell<Option<Rc<dyn SpellCheckProvider>>> =
            const { RefCell::new(None) };
    }
    PROVIDER.with(|slot| {
        let mut slot = slot.borrow_mut();
        slot.get_or_insert_with(|| Rc::new(MacSpellCheckProvider) as Rc<dyn SpellCheckProvider>)
            .clone()
    })
}

/// The macOS spelling, grammar, substitution, and definition service.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MacSpellCheckProvider;

impl SpellCheckProvider for MacSpellCheckProvider {
    fn check(
        &self,
        text: &str,
        range: Range<usize>,
        policy: TextCheckingPolicy,
    ) -> Vec<Misspelling> {
        let Some(_main) = MainThreadMarker::new() else {
            return Vec::new();
        };
        let Some(checker) = shared_checker() else {
            return Vec::new();
        };
        let Some(slice) = text.get(range.clone()) else {
            return Vec::new();
        };
        if slice.is_empty() || !policy.spellcheck {
            return Vec::new();
        }

        let source = NSString::from_str(slice);
        let length = slice.encode_utf16().count();
        let mut flagged = Vec::new();
        let mut cursor = 0_usize;
        while cursor < length && flagged.len() < crate::spell::MAX_MISSPELLED_RANGES {
            let mut word_count: isize = 0;
            let found: NSRange = unsafe {
                msg_send![
                    checker,
                    checkSpellingOfString: &*source,
                    startingAt: cursor as isize,
                    language: std::ptr::null::<NSString>(),
                    wrap: false,
                    inSpellDocumentWithTag: 0_isize,
                    wordCount: &mut word_count,
                ]
            };
            if found.length == 0 || found.location >= length {
                break;
            }
            let start = utf16_to_utf8_offset(slice, found.location);
            let end = utf16_to_utf8_offset(slice, found.location + found.length);
            if end > start {
                flagged.push(Misspelling::spelling(
                    range.start + start..range.start + end,
                ));
            }
            cursor = found.location + found.length.max(1);
        }
        flagged
    }

    fn guesses(&self, word: &str) -> Vec<String> {
        if word.is_empty() || word.len() > MAX_SPELL_WORD_BYTES {
            return Vec::new();
        }
        let Some(_main) = MainThreadMarker::new() else {
            return Vec::new();
        };
        let Some(checker) = shared_checker() else {
            return Vec::new();
        };
        let source = NSString::from_str(word);
        let range = NSRange {
            location: 0,
            length: word.encode_utf16().count(),
        };
        let guesses: *mut AnyObject = unsafe {
            msg_send![
                checker,
                guessesForWordRange: range,
                inString: &*source,
                language: std::ptr::null::<NSString>(),
                inSpellDocumentWithTag: 0_isize,
            ]
        };
        let Some(guesses) = (unsafe { Retained::retain(guesses) }) else {
            return Vec::new();
        };
        let count: usize = unsafe { msg_send![&*guesses, count] };
        let mut result = Vec::with_capacity(count.min(MAX_SPELL_GUESSES));
        for index in 0..count.min(MAX_SPELL_GUESSES) {
            let value: *mut NSString = unsafe { msg_send![&*guesses, objectAtIndex: index] };
            if let Some(value) = unsafe { Retained::retain(value) } {
                let value = value.to_string();
                if !value.is_empty() && value.len() <= MAX_SPELL_WORD_BYTES {
                    result.push(value);
                }
            }
        }
        result
    }

    fn correction(&self, text: &str, range: Range<usize>) -> Option<String> {
        let word = text.get(range.clone())?;
        if word.is_empty() || word.len() > MAX_SPELL_WORD_BYTES {
            return None;
        }
        MainThreadMarker::new()?;
        let checker = shared_checker()?;
        let source = NSString::from_str(text);
        let native = NSRange {
            location: utf8_to_utf16_offset(text, range.start),
            length: word.encode_utf16().count(),
        };
        let correction: *mut NSString = unsafe {
            msg_send![
                checker,
                correctionForWordRange: native,
                inString: &*source,
                language: std::ptr::null::<NSString>(),
                inSpellDocumentWithTag: 0_isize,
            ]
        };
        let correction = unsafe { Retained::retain(correction) }?.to_string();
        (correction != word && !correction.is_empty() && correction.len() <= MAX_SPELL_WORD_BYTES)
            .then_some(correction)
    }

    fn check_text_substitutions(
        &self,
        text: &str,
        range: Range<usize>,
        policy: TextCheckingPolicy,
    ) -> Option<TextSubstitution> {
        if !policy.text_replacement {
            return None;
        }
        let word = text.get(range.clone())?;
        if word.is_empty() || word.len() > MAX_SPELL_WORD_BYTES {
            return None;
        }
        MainThreadMarker::new()?;
        let checker = shared_checker()?;
        let source = NSString::from_str(text);
        let native = NSRange {
            location: utf8_to_utf16_offset(text, range.start),
            length: word.encode_utf16().count(),
        };
        // The user replacement dictionary is exposed for a single word range through the same
        // correction entry point; QuickGUI applies it only while `text_replacement` is enabled.
        let corrected: *mut NSString = unsafe {
            msg_send![
                checker,
                correctionForWordRange: native,
                inString: &*source,
                language: std::ptr::null::<NSString>(),
                inSpellDocumentWithTag: 0_isize,
            ]
        };
        let corrected = unsafe { Retained::retain(corrected) }?.to_string();
        (corrected != word && !corrected.is_empty() && corrected.len() <= MAX_SPELL_WORD_BYTES)
            .then(|| TextSubstitution::new(range, corrected))
    }

    fn learn(&self, word: &str) {
        if word.is_empty() || word.len() > MAX_SPELL_WORD_BYTES || MainThreadMarker::new().is_none()
        {
            return;
        }
        let Some(checker) = shared_checker() else {
            return;
        };
        let value = NSString::from_str(word);
        unsafe { msg_send![checker, learnWord: &*value] }
    }

    fn ignore(&self, word: &str, tag: SpellDocumentTag) {
        if word.is_empty() || word.len() > MAX_SPELL_WORD_BYTES || MainThreadMarker::new().is_none()
        {
            return;
        }
        let Some(checker) = shared_checker() else {
            return;
        };
        let value = NSString::from_str(word);
        unsafe { msg_send![checker, ignoreWord: &*value, inSpellDocumentWithTag: tag.0] }
    }

    fn open_document(&self) -> SpellDocumentTag {
        if MainThreadMarker::new().is_none() {
            return SpellDocumentTag::NONE;
        }
        let tag: isize = unsafe { msg_send![class!(NSSpellChecker), uniqueSpellDocumentTag] };
        SpellDocumentTag(tag as i64)
    }

    fn close_document(&self, tag: SpellDocumentTag) {
        if tag == SpellDocumentTag::NONE || MainThreadMarker::new().is_none() {
            return;
        }
        let Some(checker) = shared_checker() else {
            return;
        };
        unsafe { msg_send![checker, closeSpellDocumentWithTag: tag.0 as isize] }
    }

    fn show_definition(&self, text: &str, position: Point) -> Result<(), TextServiceError> {
        let Some(main) = MainThreadMarker::new() else {
            return Err(TextServiceError::Unsupported);
        };
        let application = NSApplication::sharedApplication(main);
        let window = application
            .keyWindow()
            .ok_or(TextServiceError::Unsupported)?;
        let view: Retained<NSView> = window.contentView().ok_or(TextServiceError::Unsupported)?;
        let value = NSString::from_str(text);
        let attributed: *mut AnyObject = unsafe {
            let allocated: *mut AnyObject = msg_send![class!(NSAttributedString), alloc];
            msg_send![allocated, initWithString: &*value]
        };
        let attributed =
            unsafe { Retained::from_raw(attributed) }.ok_or(TextServiceError::Unsupported)?;
        let height = view.frame().size.height;
        let point = NSPoint {
            x: f64::from(position.x),
            y: height - f64::from(position.y),
        };
        unsafe {
            let _: () = msg_send![
                &*view,
                showDefinitionForAttributedString: &*attributed,
                atPoint: point,
            ];
        }
        Ok(())
    }
}

fn shared_checker() -> Option<&'static AnyObject> {
    let checker: *mut AnyObject = unsafe { msg_send![class!(NSSpellChecker), sharedSpellChecker] };
    // The shared checker is an AppKit singleton retained for the process lifetime.
    unsafe { checker.as_ref() }
}

/// Convert a UTF-8 byte offset into a UTF-16 code-unit offset.
pub(crate) fn utf8_to_utf16_offset(text: &str, offset: usize) -> usize {
    text.get(..offset.min(text.len()))
        .map_or(0, |prefix| prefix.encode_utf16().count())
}

/// Convert a UTF-16 code-unit offset into a UTF-8 byte offset.
pub(crate) fn utf16_to_utf8_offset(text: &str, offset: usize) -> usize {
    let mut units = 0_usize;
    for (index, character) in text.char_indices() {
        if units >= offset {
            return index;
        }
        units += character.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_and_utf8_offsets_round_trip_across_astral_planes() {
        let text = "a🙂b";
        assert_eq!(utf8_to_utf16_offset(text, 0), 0);
        assert_eq!(utf8_to_utf16_offset(text, 1), 1);
        assert_eq!(utf8_to_utf16_offset(text, 5), 3);
        assert_eq!(utf16_to_utf8_offset(text, 0), 0);
        assert_eq!(utf16_to_utf8_offset(text, 1), 1);
        assert_eq!(utf16_to_utf8_offset(text, 3), 5);
        assert_eq!(utf16_to_utf8_offset(text, 99), text.len());
    }
}
