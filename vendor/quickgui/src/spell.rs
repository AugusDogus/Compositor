//! Text checking services: spelling, grammar, substitutions, and dictionary lookup.
//!
//! Everything here is main-thread owned, bounded, and free of idle work. Checking runs only after
//! an edit settles through one exact one-shot deadline; a settled window never polls a checker.
//!
//! The core owns policy, bounds, projection, and the undoable edits. The dictionary itself lives
//! behind [`SpellCheckProvider`], which an application can replace with
//! [`set_spell_check_provider`]. macOS supplies an `NSSpellChecker`-backed default; every other
//! target falls back to [`NoSpellCheckProvider`], whose services report
//! [`TextServiceError::Unsupported`].

use std::{
    cell::{Cell, RefCell},
    fmt,
    ops::Range,
    rc::Rc,
    sync::Arc,
};
use web_time::Duration;

use unicode_segmentation::UnicodeSegmentation;

use crate::{Color, HighlightStyle, Point, PopoverMenuItem};

/// Maximum UTF-8 bytes handed to a spell checker for one settled check.
///
/// Checking is always restricted to a window of this size around the edited caret, so document
/// size never controls checking cost.
pub const MAX_SPELLCHECK_BYTES: usize = 16 * 1024;

/// Maximum replacement guesses retained for one misspelled word.
pub const MAX_SPELL_GUESSES: usize = 16;

/// Maximum misspelled or ungrammatical ranges retained for one text input.
pub const MAX_MISSPELLED_RANGES: usize = 512;

/// Maximum UTF-8 bytes of one word passed to guesses, learn, ignore, or correction services.
pub const MAX_SPELL_WORD_BYTES: usize = 256;

/// Maximum UTF-8 bytes of one dictionary-lookup request.
pub const MAX_DEFINITION_LOOKUP_BYTES: usize = 1_024;

/// Delay after the last accepted edit before one settled spell check runs.
///
/// This contributes exactly one one-shot deadline. Further edits cancel and re-arm it; an idle
/// input holds no deadline at all.
pub const SPELL_CHECK_SETTLE_DELAY: Duration = Duration::from_millis(300);

/// A service that the current target or provider cannot perform.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextServiceError {
    /// The running platform or the installed provider does not implement this service.
    Unsupported,
    /// The request was empty or exceeded a documented bound.
    InvalidRequest,
}

impl fmt::Display for TextServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => formatter.write_str("text service is unsupported on this target"),
            Self::InvalidRequest => formatter.write_str("text service request was out of bounds"),
        }
    }
}

impl std::error::Error for TextServiceError {}

/// Why one checked range is flagged.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MisspellingKind {
    /// An unknown word.
    #[default]
    Spelling,
    /// A grammar or usage problem reported by the provider.
    Grammar,
}

/// One flagged UTF-8 byte range inside a checked text input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Misspelling {
    range: Range<usize>,
    kind: MisspellingKind,
}

impl Misspelling {
    /// Flag one non-empty UTF-8 byte range.
    pub const fn new(range: Range<usize>, kind: MisspellingKind) -> Self {
        Self { range, kind }
    }

    /// Flag one range as a misspelled word.
    pub const fn spelling(range: Range<usize>) -> Self {
        Self::new(range, MisspellingKind::Spelling)
    }

    /// Flag one range as a grammar problem.
    pub const fn grammar(range: Range<usize>) -> Self {
        Self::new(range, MisspellingKind::Grammar)
    }

    /// The flagged UTF-8 byte range.
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// Why the range is flagged.
    pub const fn kind(&self) -> MisspellingKind {
        self.kind
    }
}

/// One accepted text substitution supplied by a provider's replacement dictionary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextSubstitution {
    range: Range<usize>,
    replacement: Arc<str>,
}

impl TextSubstitution {
    /// Replace `range` with `replacement`.
    pub fn new(range: Range<usize>, replacement: impl Into<Arc<str>>) -> Self {
        Self {
            range,
            replacement: replacement.into(),
        }
    }

    /// The replaced UTF-8 byte range.
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// The replacement text.
    pub fn replacement(&self) -> &Arc<str> {
        &self.replacement
    }
}

/// One applied autocorrection, retained so an application can offer "Change back".
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Autocorrection {
    range: Range<usize>,
    original: Arc<str>,
    replacement: Arc<str>,
}

impl Autocorrection {
    pub(crate) fn new(
        range: Range<usize>,
        original: impl Into<Arc<str>>,
        replacement: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            range,
            original: original.into(),
            replacement: replacement.into(),
        }
    }

    /// The UTF-8 byte range that now holds the replacement.
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// The word the user actually typed.
    pub fn original(&self) -> &Arc<str> {
        &self.original
    }

    /// The word the checker substituted.
    pub fn replacement(&self) -> &Arc<str> {
        &self.replacement
    }
}

/// Resolved per-input text checking behavior.
///
/// Every flag defaults to `false`; QuickGUI never enables a checker an application did not ask
/// for. Use [`set_default_text_checking`] for an application-wide policy and the text-input
/// builders for per-input overrides.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct TextCheckingPolicy {
    /// Underline unknown words after an edit settles.
    pub spellcheck: bool,
    /// Ask the provider for grammar problems during the same settled check.
    pub grammar_check: bool,
    /// Replace a completed word with the checker's correction at a word boundary.
    pub autocorrect: bool,
    /// Convert straight quotes to typographic quotes at insertion time.
    pub smart_quotes: bool,
    /// Convert `--` to an em dash at insertion time.
    pub smart_dashes: bool,
    /// Apply the provider's replacement dictionary at insertion time.
    pub text_replacement: bool,
    /// Show the dictionary popover when a Force Touch trackpad force-clicks a word.
    pub lookup_on_force_click: bool,
}

impl TextCheckingPolicy {
    /// A policy with every service disabled.
    pub const NONE: Self = Self {
        spellcheck: false,
        grammar_check: false,
        autocorrect: false,
        smart_quotes: false,
        smart_dashes: false,
        text_replacement: false,
        lookup_on_force_click: false,
    };

    /// A policy with every service enabled.
    pub const ALL: Self = Self {
        spellcheck: true,
        grammar_check: true,
        autocorrect: true,
        smart_quotes: true,
        smart_dashes: true,
        text_replacement: true,
        lookup_on_force_click: true,
    };

    /// Whether any service needs a settled deadline.
    pub const fn checks_after_settle(self) -> bool {
        self.spellcheck || self.grammar_check
    }

    /// Whether any service inspects text at insertion time.
    pub const fn substitutes_on_insert(self) -> bool {
        self.smart_quotes || self.smart_dashes || self.text_replacement
    }
}

/// Per-input overrides layered over [`default_text_checking`].
///
/// `None` inherits the application policy; `Some(value)` overrides it for one input.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct TextCheckingOverrides {
    pub spellcheck: Option<bool>,
    pub grammar_check: Option<bool>,
    pub autocorrect: Option<bool>,
    pub smart_quotes: Option<bool>,
    pub smart_dashes: Option<bool>,
    pub text_replacement: Option<bool>,
    pub lookup_on_force_click: Option<bool>,
}

impl TextCheckingOverrides {
    /// Resolve this input's overrides against an application-wide default policy.
    pub fn resolve(self, default: TextCheckingPolicy) -> TextCheckingPolicy {
        TextCheckingPolicy {
            spellcheck: self.spellcheck.unwrap_or(default.spellcheck),
            grammar_check: self.grammar_check.unwrap_or(default.grammar_check),
            autocorrect: self.autocorrect.unwrap_or(default.autocorrect),
            smart_quotes: self.smart_quotes.unwrap_or(default.smart_quotes),
            smart_dashes: self.smart_dashes.unwrap_or(default.smart_dashes),
            text_replacement: self.text_replacement.unwrap_or(default.text_replacement),
            lookup_on_force_click: self
                .lookup_on_force_click
                .unwrap_or(default.lookup_on_force_click),
        }
    }
}

/// An opaque per-input checking session identity.
///
/// macOS uses it as the `NSSpellChecker` document tag so ignored words stay scoped to one input.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SpellDocumentTag(pub i64);

impl SpellDocumentTag {
    /// The shared, document-less tag.
    pub const NONE: Self = Self(0);
}

/// A replaceable dictionary, grammar, and substitution service.
///
/// Implementations are main-thread only and use interior mutability, because the retained editor
/// calls them behind a shared reference. Every method has a conservative default so a partial
/// implementation stays valid.
pub trait SpellCheckProvider {
    /// Return flagged ranges inside `range` of `text`.
    ///
    /// `range` is already clamped to [`MAX_SPELLCHECK_BYTES`] and to UTF-8 boundaries. Returned
    /// ranges are absolute offsets into `text`; the caller clamps, sorts, and bounds them.
    fn check(
        &self,
        text: &str,
        range: Range<usize>,
        policy: TextCheckingPolicy,
    ) -> Vec<Misspelling>;

    /// Return replacement guesses for one word, best first.
    ///
    /// The caller truncates the result to [`MAX_SPELL_GUESSES`].
    fn guesses(&self, _word: &str) -> Vec<String> {
        Vec::new()
    }

    /// Return the automatic correction for the word at `range`, if the checker has one.
    fn correction(&self, _text: &str, _range: Range<usize>) -> Option<String> {
        None
    }

    /// Return a replacement-dictionary substitution for the word at `range`.
    fn check_text_substitutions(
        &self,
        _text: &str,
        _range: Range<usize>,
        _policy: TextCheckingPolicy,
    ) -> Option<TextSubstitution> {
        None
    }

    /// Add a word to the user dictionary.
    fn learn(&self, _word: &str) {}

    /// Ignore a word for the remainder of one checking session.
    fn ignore(&self, _word: &str, _tag: SpellDocumentTag) {}

    /// Open one checking session. Defaults to the shared session.
    fn open_document(&self) -> SpellDocumentTag {
        SpellDocumentTag::NONE
    }

    /// Release a checking session opened by [`Self::open_document`].
    fn close_document(&self, _tag: SpellDocumentTag) {}

    /// Show the platform dictionary popover for `text` at a window-local point.
    fn show_definition(&self, _text: &str, _position: Point) -> Result<(), TextServiceError> {
        Err(TextServiceError::Unsupported)
    }
}

/// The portable fallback provider. Every service reports [`TextServiceError::Unsupported`].
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSpellCheckProvider;

impl SpellCheckProvider for NoSpellCheckProvider {
    fn check(
        &self,
        _text: &str,
        _range: Range<usize>,
        _policy: TextCheckingPolicy,
    ) -> Vec<Misspelling> {
        Vec::new()
    }
}

/// A deterministic in-memory checker for tests and examples.
///
/// It flags exactly the words it was told about, returns exactly the guesses, corrections, and
/// replacements it was given, and records learned words, ignored words, and open document tags so
/// a test can assert on them without a platform dictionary.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Default)]
pub struct TestSpellCheckProvider {
    misspelled: RefCell<Vec<Arc<str>>>,
    guesses: RefCell<Vec<(Arc<str>, Vec<String>)>>,
    corrections: RefCell<Vec<(Arc<str>, Arc<str>)>>,
    replacements: RefCell<Vec<(Arc<str>, Arc<str>)>>,
    learned: RefCell<Vec<Arc<str>>>,
    ignored: RefCell<Vec<Arc<str>>>,
    next_tag: Cell<i64>,
    open_documents: Cell<usize>,
}

#[cfg(any(test, feature = "test-support"))]
impl TestSpellCheckProvider {
    /// Create an empty checker that flags nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Treat `word` as misspelled until it is learned or ignored.
    #[must_use]
    pub fn misspelling(self, word: &str) -> Self {
        self.misspelled.borrow_mut().push(Arc::from(word));
        self
    }

    /// Return `guesses` for `word`.
    #[must_use]
    pub fn guess(self, word: &str, guesses: &[&str]) -> Self {
        self.guesses.borrow_mut().push((
            Arc::from(word),
            guesses.iter().map(|guess| (*guess).to_owned()).collect(),
        ));
        self
    }

    /// Autocorrect `word` to `correction`.
    #[must_use]
    pub fn correction_for(self, word: &str, correction: &str) -> Self {
        self.corrections
            .borrow_mut()
            .push((Arc::from(word), Arc::from(correction)));
        self
    }

    /// Substitute `word` with `replacement` from the replacement dictionary.
    #[must_use]
    pub fn replacement_for(self, word: &str, replacement: &str) -> Self {
        self.replacements
            .borrow_mut()
            .push((Arc::from(word), Arc::from(replacement)));
        self
    }

    /// Words passed to [`SpellCheckProvider::learn`].
    pub fn learned(&self) -> Vec<Arc<str>> {
        self.learned.borrow().clone()
    }

    /// Words passed to [`SpellCheckProvider::ignore`].
    pub fn ignored(&self) -> Vec<Arc<str>> {
        self.ignored.borrow().clone()
    }

    /// Checking sessions currently open.
    pub fn open_documents(&self) -> usize {
        self.open_documents.get()
    }

    fn flags(&self, word: &str) -> bool {
        let matches = |list: &RefCell<Vec<Arc<str>>>| {
            list.borrow().iter().any(|entry| entry.as_ref() == word)
        };
        matches(&self.misspelled) && !matches(&self.learned) && !matches(&self.ignored)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl SpellCheckProvider for TestSpellCheckProvider {
    fn check(
        &self,
        text: &str,
        range: Range<usize>,
        policy: TextCheckingPolicy,
    ) -> Vec<Misspelling> {
        if !policy.spellcheck {
            return Vec::new();
        }
        let Some(slice) = text.get(range.clone()) else {
            return Vec::new();
        };
        slice
            .split_word_bound_indices()
            .filter(|(_, segment)| self.flags(segment))
            .map(|(start, segment)| {
                Misspelling::spelling(range.start + start..range.start + start + segment.len())
            })
            .collect()
    }

    fn guesses(&self, word: &str) -> Vec<String> {
        self.guesses
            .borrow()
            .iter()
            .find(|(entry, _)| entry.as_ref() == word)
            .map(|(_, guesses)| guesses.clone())
            .unwrap_or_default()
    }

    fn correction(&self, text: &str, range: Range<usize>) -> Option<String> {
        let word = text.get(range)?;
        self.corrections
            .borrow()
            .iter()
            .find(|(entry, _)| entry.as_ref() == word)
            .map(|(_, correction)| correction.to_string())
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
        self.replacements
            .borrow()
            .iter()
            .find(|(entry, _)| entry.as_ref() == word)
            .map(|(_, replacement)| TextSubstitution::new(range, replacement.clone()))
    }

    fn learn(&self, word: &str) {
        self.learned.borrow_mut().push(Arc::from(word));
    }

    fn ignore(&self, word: &str, _tag: SpellDocumentTag) {
        self.ignored.borrow_mut().push(Arc::from(word));
    }

    fn open_document(&self) -> SpellDocumentTag {
        self.next_tag.set(self.next_tag.get() + 1);
        self.open_documents.set(self.open_documents.get() + 1);
        SpellDocumentTag(self.next_tag.get())
    }

    fn close_document(&self, _tag: SpellDocumentTag) {
        self.open_documents
            .set(self.open_documents.get().saturating_sub(1));
    }
}

thread_local! {
    static PROVIDER: RefCell<Option<Rc<dyn SpellCheckProvider>>> =
        const { RefCell::new(None) };
    static DEFAULT_POLICY: Cell<TextCheckingPolicy> =
        const { Cell::new(TextCheckingPolicy::NONE) };
    static SPELLING_STYLE: RefCell<Option<HighlightStyle>> = const { RefCell::new(None) };
    static GRAMMAR_STYLE: RefCell<Option<HighlightStyle>> = const { RefCell::new(None) };
}

/// Install an application-supplied checker for this application thread.
///
/// The provider replaces the platform default for every text input and for
/// [`show_definition_for`]. Applications typically install one during startup.
pub fn set_spell_check_provider(provider: impl SpellCheckProvider + 'static) {
    PROVIDER.with(|slot| *slot.borrow_mut() = Some(Rc::new(provider)));
}

/// Install an already shared checker.
pub fn set_shared_spell_check_provider(provider: Rc<dyn SpellCheckProvider>) {
    PROVIDER.with(|slot| *slot.borrow_mut() = Some(provider));
}

/// Remove an application-supplied checker and restore the platform default.
pub fn clear_spell_check_provider() {
    PROVIDER.with(|slot| *slot.borrow_mut() = None);
}

/// Whether an application installed its own checker.
pub fn has_spell_check_provider() -> bool {
    PROVIDER.with(|slot| slot.borrow().is_some())
}

/// The checker used by text inputs: the installed provider, otherwise the platform default.
pub fn spell_check_provider() -> Rc<dyn SpellCheckProvider> {
    if let Some(provider) = PROVIDER.with(|slot| slot.borrow().clone()) {
        return provider;
    }
    platform_spell_check_provider()
}

#[cfg(target_os = "macos")]
fn platform_spell_check_provider() -> Rc<dyn SpellCheckProvider> {
    crate::macos::spell::shared_provider()
}

#[cfg(not(target_os = "macos"))]
fn platform_spell_check_provider() -> Rc<dyn SpellCheckProvider> {
    thread_local! {
        static FALLBACK: Rc<dyn SpellCheckProvider> = Rc::new(NoSpellCheckProvider);
    }
    FALLBACK.with(Rc::clone)
}

/// Replace the application-wide text checking policy inherited by every text input.
pub fn set_default_text_checking(policy: TextCheckingPolicy) {
    DEFAULT_POLICY.with(|slot| slot.set(policy));
}

/// The application-wide text checking policy.
pub fn default_text_checking() -> TextCheckingPolicy {
    DEFAULT_POLICY.with(Cell::get)
}

/// Replace the run style used for misspelled words.
///
/// QuickGUI ships one unstyled default: a one-pixel red wavy underline. Pass `None` to restore it.
pub fn set_misspelling_highlight_style(style: Option<HighlightStyle>) {
    SPELLING_STYLE.with(|slot| *slot.borrow_mut() = style);
}

/// Replace the run style used for grammar problems. Pass `None` to restore the green default.
pub fn set_grammar_highlight_style(style: Option<HighlightStyle>) {
    GRAMMAR_STYLE.with(|slot| *slot.borrow_mut() = style);
}

/// The run style projected onto misspelled words.
pub fn misspelling_highlight_style() -> HighlightStyle {
    SPELLING_STYLE.with(|slot| {
        slot.borrow().clone().unwrap_or_else(|| {
            HighlightStyle::default()
                .underline()
                .text_decoration_wavy()
                .text_decoration_1()
                .underline_color(Color::rgb8(248, 113, 113))
        })
    })
}

/// The run style projected onto grammar problems.
pub fn grammar_highlight_style() -> HighlightStyle {
    GRAMMAR_STYLE.with(|slot| {
        slot.borrow().clone().unwrap_or_else(|| {
            HighlightStyle::default()
                .underline()
                .text_decoration_wavy()
                .text_decoration_1()
                .underline_color(Color::rgb8(52, 211, 153))
        })
    })
}

/// Show the platform dictionary popover for one bounded string.
///
/// `position` is a window-local logical point, normally the caret rectangle's bottom-left corner.
/// Portable targets return [`TextServiceError::Unsupported`].
pub fn show_definition_for(text: &str, position: Point) -> Result<(), TextServiceError> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_DEFINITION_LOOKUP_BYTES {
        return Err(TextServiceError::InvalidRequest);
    }
    spell_check_provider().show_definition(text, position)
}

/// An owned checking session that releases its provider document tag on drop.
pub(crate) struct SpellDocument {
    provider: Rc<dyn SpellCheckProvider>,
    tag: SpellDocumentTag,
}

impl SpellDocument {
    pub(crate) fn open() -> Rc<Self> {
        let provider = spell_check_provider();
        let tag = provider.open_document();
        Rc::new(Self { provider, tag })
    }

    pub(crate) const fn tag(&self) -> SpellDocumentTag {
        self.tag
    }
}

impl fmt::Debug for SpellDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpellDocument")
            .field("tag", &self.tag)
            .finish_non_exhaustive()
    }
}

impl Drop for SpellDocument {
    fn drop(&mut self) {
        if self.tag != SpellDocumentTag::NONE {
            self.provider.close_document(self.tag);
        }
    }
}

/// Replace one flagged range with a suggestion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceWord {
    /// The UTF-8 byte range being replaced.
    pub range: Range<usize>,
    /// The replacement text.
    pub replacement: Arc<str>,
}

/// Add one word to the user dictionary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LearnWord {
    /// The word to learn.
    pub word: Arc<str>,
}

/// Ignore one word for the remainder of this input's checking session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IgnoreWord {
    /// The word to ignore.
    pub word: Arc<str>,
}

/// Show the dictionary popover for the selection, or the word at the caret.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LookUpSelection;

/// Element-ID namespace for the generated spelling menu.
pub const SPELLING_MENU_ID_PREFIX: &str = "quickgui-spelling";

/// Build the standard right-click spelling entries for one flagged word.
///
/// The returned items dispatch [`ReplaceWord`], [`LearnWord`], and [`IgnoreWord`]. A caller mounts
/// them inside its own `PopoverMenu` or `ContextMenu` and keeps every visual decision.
pub fn spelling_menu_items(
    range: Range<usize>,
    word: &str,
    guesses: &[Arc<str>],
    labels: SpellingMenuLabels,
) -> Vec<PopoverMenuItem> {
    let mut items = Vec::with_capacity(guesses.len() + 3);
    if guesses.is_empty() {
        items.push(
            PopoverMenuItem::action(
                format!("{SPELLING_MENU_ID_PREFIX}-empty"),
                labels.no_guesses.clone(),
                ReplaceWord {
                    range: range.clone(),
                    replacement: Arc::from(word),
                },
            )
            .disabled(true),
        );
    }
    for (index, guess) in guesses.iter().take(MAX_SPELL_GUESSES).enumerate() {
        items.push(PopoverMenuItem::action(
            format!("{SPELLING_MENU_ID_PREFIX}-guess-{index}"),
            guess.clone(),
            ReplaceWord {
                range: range.clone(),
                replacement: guess.clone(),
            },
        ));
    }
    items.push(PopoverMenuItem::separator());
    items.push(PopoverMenuItem::action(
        format!("{SPELLING_MENU_ID_PREFIX}-learn"),
        labels.learn.clone(),
        LearnWord {
            word: Arc::from(word),
        },
    ));
    items.push(PopoverMenuItem::action(
        format!("{SPELLING_MENU_ID_PREFIX}-ignore"),
        labels.ignore.clone(),
        IgnoreWord {
            word: Arc::from(word),
        },
    ));
    items
}

/// Application-owned copy for the generated spelling menu.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpellingMenuLabels {
    /// Shown, disabled, when the checker has no guesses.
    pub no_guesses: Arc<str>,
    /// Label for the "learn" entry.
    pub learn: Arc<str>,
    /// Label for the "ignore" entry.
    pub ignore: Arc<str>,
}

impl Default for SpellingMenuLabels {
    fn default() -> Self {
        Self {
            no_guesses: Arc::from("No Guesses Found"),
            learn: Arc::from("Learn Spelling"),
            ignore: Arc::from("Ignore Spelling"),
        }
    }
}

/// One insertion-time substitution applied before the edit reaches retained text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SmartSubstitution {
    /// Bytes removed immediately before the insertion point.
    pub(crate) remove_before: usize,
    /// The text inserted instead of the typed value.
    pub(crate) replacement: String,
}

/// Apply smart quote and smart dash substitutions to one typed value.
///
/// `preceding` is the retained text before the replaced range. Only single typed characters
/// participate, so paste, IME commits, and programmatic edits are never rewritten.
pub(crate) fn smart_substitution(
    preceding: &str,
    value: &str,
    policy: TextCheckingPolicy,
) -> Option<SmartSubstitution> {
    if value.chars().count() != 1 {
        return None;
    }
    let character = value.chars().next()?;
    match character {
        '"' | '\'' if policy.smart_quotes => {
            let opening = opens_quotation(preceding);
            let replacement = match (character, opening) {
                ('"', true) => '\u{201c}',
                ('"', false) => '\u{201d}',
                (_, true) => '\u{2018}',
                (_, false) => '\u{2019}',
            };
            Some(SmartSubstitution {
                remove_before: 0,
                replacement: replacement.to_string(),
            })
        }
        '-' if policy.smart_dashes => {
            let mut characters = preceding.chars().rev();
            if characters.next() != Some('-') || characters.next() == Some('-') {
                return None;
            }
            Some(SmartSubstitution {
                remove_before: '-'.len_utf8(),
                replacement: '\u{2014}'.to_string(),
            })
        }
        _ => None,
    }
}

fn opens_quotation(preceding: &str) -> bool {
    preceding.chars().next_back().is_none_or(|character| {
        character.is_whitespace()
            || matches!(
                character,
                '(' | '[' | '{' | '\u{201c}' | '\u{2018}' | '\u{2014}' | '\u{2013}' | '/' | '-'
            )
    })
}

/// Whether typing `character` completes the word before the caret.
pub(crate) fn completes_word(character: char) -> bool {
    character.is_whitespace() || (!character.is_alphanumeric() && character != '_')
}

/// The word range containing or immediately preceding `offset`.
///
/// Offsets inside a word return that word; an offset at a word's trailing boundary returns the
/// word that just ended. Returns `None` when no alphabetic word is adjacent.
pub fn word_range_at(text: &str, offset: usize) -> Option<Range<usize>> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    let mut preceding = None;
    for (start, segment) in text.split_word_bound_indices() {
        if !segment.chars().any(char::is_alphanumeric) {
            continue;
        }
        let end = start + segment.len();
        if offset >= start && offset < end {
            return Some(start..end);
        }
        if end == offset {
            preceding = Some(start..end);
        }
        if start > offset {
            break;
        }
    }
    preceding
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_quotes_follow_the_preceding_character() {
        let policy = TextCheckingPolicy {
            smart_quotes: true,
            ..TextCheckingPolicy::NONE
        };
        assert_eq!(
            smart_substitution("", "\"", policy).unwrap().replacement,
            "\u{201c}"
        );
        assert_eq!(
            smart_substitution("say ", "\"", policy)
                .unwrap()
                .replacement,
            "\u{201c}"
        );
        assert_eq!(
            smart_substitution("hello", "\"", policy)
                .unwrap()
                .replacement,
            "\u{201d}"
        );
        assert_eq!(
            smart_substitution("it", "'", policy).unwrap().replacement,
            "\u{2019}"
        );
        assert!(smart_substitution("hello", "\"", TextCheckingPolicy::NONE).is_none());
        assert!(smart_substitution("hi", "ab", policy).is_none());
    }

    #[test]
    fn smart_dashes_collapse_exactly_two_hyphens() {
        let policy = TextCheckingPolicy {
            smart_dashes: true,
            ..TextCheckingPolicy::NONE
        };
        let substitution = smart_substitution("a-", "-", policy).unwrap();
        assert_eq!(substitution.remove_before, 1);
        assert_eq!(substitution.replacement, "\u{2014}");
        assert!(smart_substitution("a", "-", policy).is_none());
        assert!(smart_substitution("a--", "-", policy).is_none());
    }

    #[test]
    fn word_ranges_prefer_the_containing_then_the_completed_word() {
        let text = "alpha beta";
        assert_eq!(word_range_at(text, 2), Some(0..5));
        assert_eq!(word_range_at(text, 5), Some(0..5));
        assert_eq!(word_range_at(text, 6), Some(6..10));
        assert_eq!(word_range_at(text, 10), Some(6..10));
        assert_eq!(word_range_at("  ", 1), None);
        assert_eq!(word_range_at(text, 99), None);
    }

    #[test]
    fn overrides_resolve_against_the_application_policy() {
        let default = TextCheckingPolicy {
            spellcheck: true,
            smart_quotes: true,
            ..TextCheckingPolicy::NONE
        };
        let overrides = TextCheckingOverrides {
            spellcheck: Some(false),
            autocorrect: Some(true),
            ..TextCheckingOverrides::default()
        };
        let resolved = overrides.resolve(default);
        assert!(!resolved.spellcheck);
        assert!(resolved.smart_quotes);
        assert!(resolved.autocorrect);
        assert!(!resolved.grammar_check);
    }

    #[test]
    fn spelling_menu_items_expose_bounded_typed_actions() {
        let guesses: Vec<Arc<str>> = vec![Arc::from("hello"), Arc::from("halo")];
        let items = spelling_menu_items(0..4, "helo", &guesses, SpellingMenuLabels::default());
        assert_eq!(items.len(), 5);
        assert_eq!(items[0].label().as_ref(), "hello");
        assert_eq!(items[3].label().as_ref(), "Learn Spelling");
        assert_eq!(items[4].label().as_ref(), "Ignore Spelling");

        let empty = spelling_menu_items(0..4, "helo", &[], SpellingMenuLabels::default());
        assert_eq!(empty.len(), 4);
        assert!(empty[0].is_disabled());
    }

    #[test]
    fn definition_requests_are_bounded_and_unsupported_without_a_provider() {
        clear_spell_check_provider();
        set_spell_check_provider(NoSpellCheckProvider);
        assert_eq!(
            show_definition_for("", Point::new(0.0, 0.0)),
            Err(TextServiceError::InvalidRequest)
        );
        assert_eq!(
            show_definition_for(
                &"x".repeat(MAX_DEFINITION_LOOKUP_BYTES + 1),
                Point::default()
            ),
            Err(TextServiceError::InvalidRequest)
        );
        assert_eq!(
            show_definition_for("word", Point::default()),
            Err(TextServiceError::Unsupported)
        );
        clear_spell_check_provider();
    }
}
