use std::{any::TypeId, collections::HashMap, fmt, str::FromStr, sync::Arc};

use crate::{Action, AnyAction, Key, Modifiers};

/// A parse failure in a keystroke, key context, or context predicate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeymapError {
    offset: usize,
    message: String,
}

impl KeymapError {
    fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for KeymapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for KeymapError {}

/// A normalized command key plus modifiers and optional text-producing key metadata.
///
/// `key` is suitable for command matching and is intentionally independent from Caps Lock and
/// text composition. `key_char` records the printable character that the same native press could
/// produce. QuickGUI checks both identities without ever synthesizing committed text from a
/// command match.
#[derive(Clone, Debug)]
pub struct Keystroke {
    pub key: Key,
    pub modifiers: Modifiers,
    pub key_char: Option<Key>,
}

impl Keystroke {
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        Self {
            key: normalize_key(key),
            modifiers,
            key_char: None,
        }
    }

    pub fn parse(value: &str) -> Result<Self, KeymapError> {
        value.parse()
    }

    pub fn from_key_event(key: &Key, modifiers: Modifiers) -> Self {
        Self::new(key.clone(), modifiers)
    }

    /// Attach the printable character produced by this keypress.
    ///
    /// This is primarily useful for deterministic platform tests. Native applications receive it
    /// from the keyboard backend. It affects keymap matching but not equality, display, or text
    /// insertion.
    pub fn with_key_char(mut self, key_char: Key) -> Self {
        self.key_char = Some(normalize_key(key_char));
        self
    }

    pub(crate) fn from_platform_event(
        key: Key,
        modifiers: Modifiers,
        key_char: Option<Key>,
    ) -> Self {
        let mut stroke = Self::new(key, modifiers);
        stroke.key_char = key_char.map(normalize_key);
        stroke
    }

    fn should_match(&self, target: &Self) -> bool {
        if let Some(key_char) = self
            .key_char
            .as_ref()
            .filter(|key_char| *key_char != &self.key)
        {
            let text_modifiers = self.modifiers & (Modifiers::CONTROL | Modifiers::SUPER);
            if target.key == *key_char && target.modifiers == text_modifiers {
                return true;
            }
        }
        target.key == self.key && target.modifiers == self.modifiers
    }

    fn key_char_identity(&self) -> Option<Self> {
        let key_char = self
            .key_char
            .as_ref()
            .filter(|key_char| *key_char != &self.key)?;
        Some(Self::new(
            key_char.clone(),
            self.modifiers & (Modifiers::CONTROL | Modifiers::SUPER),
        ))
    }
}

// Platform text metadata does not change the command identity. Keymap matching explicitly checks
// it as a second candidate through `should_match`.
impl PartialEq for Keystroke {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.modifiers == other.modifiers
    }
}

impl Eq for Keystroke {}

impl std::hash::Hash for Keystroke {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.modifiers.hash(state);
    }
}

impl FromStr for Keystroke {
    type Err = KeymapError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.is_empty() {
            return Err(KeymapError::new(0, "keystroke cannot be empty"));
        }

        let mut modifiers = Modifiers::empty();
        let mut key = None;
        let mut offset = 0;
        for component in input.split('-') {
            if component.is_empty() {
                return Err(KeymapError::new(offset, "empty keystroke component"));
            }
            let normalized = component.to_ascii_lowercase();
            let modifier = match normalized.as_str() {
                "shift" => Some(Modifiers::SHIFT),
                "ctrl" | "control" => Some(Modifiers::CONTROL),
                "alt" | "option" => Some(Modifiers::ALT),
                "cmd" | "command" | "meta" | "super" => Some(Modifiers::SUPER),
                "platform" => Some(if cfg!(target_os = "macos") {
                    Modifiers::SUPER
                } else {
                    Modifiers::CONTROL
                }),
                _ => None,
            };
            if let Some(modifier) = modifier {
                modifiers.insert(modifier);
            } else {
                if key.is_some() {
                    return Err(KeymapError::new(
                        offset,
                        "a keystroke must contain exactly one non-modifier key",
                    ));
                }
                key = Some(parse_key_component(component, offset)?);
            }
            offset += component.len() + 1;
        }

        let key = key.ok_or_else(|| KeymapError::new(input.len(), "keystroke is missing a key"))?;
        Ok(Self::new(key, modifiers))
    }
}

impl fmt::Display for Keystroke {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (modifier, label) in [
            (Modifiers::CONTROL, "ctrl"),
            (Modifiers::ALT, "alt"),
            (Modifiers::SHIFT, "shift"),
            (Modifiers::SUPER, "cmd"),
        ] {
            if self.modifiers.contains(modifier) {
                write!(formatter, "{label}-")?;
            }
        }
        formatter.write_str(key_name(&self.key).as_ref())
    }
}

fn parse_key_component(component: &str, offset: usize) -> Result<Key, KeymapError> {
    let normalized = component.to_lowercase();
    let named = match normalized.as_str() {
        "up" | "arrowup" => Some(Key::ArrowUp),
        "down" | "arrowdown" => Some(Key::ArrowDown),
        "left" | "arrowleft" => Some(Key::ArrowLeft),
        "right" | "arrowright" => Some(Key::ArrowRight),
        "pageup" => Some(Key::PageUp),
        "pagedown" => Some(Key::PageDown),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "enter" | "return" => Some(Key::Enter),
        "escape" | "esc" => Some(Key::Escape),
        "space" => Some(Key::Space),
        "tab" => Some(Key::Tab),
        "backspace" => Some(Key::Backspace),
        "delete" | "del" => Some(Key::Delete),
        "insert" | "ins" => Some(Key::Insert),
        "minus" | "hyphen" => Some(Key::Character("-".to_owned())),
        "equal" | "equals" => Some(Key::Character("=".to_owned())),
        "comma" => Some(Key::Character(",".to_owned())),
        "period" | "dot" => Some(Key::Character(".".to_owned())),
        "slash" => Some(Key::Character("/".to_owned())),
        "backslash" => Some(Key::Character("\\".to_owned())),
        "semicolon" => Some(Key::Character(";".to_owned())),
        "quote" | "apostrophe" => Some(Key::Character("'".to_owned())),
        "leftbracket" | "bracketleft" => Some(Key::Character("[".to_owned())),
        "rightbracket" | "bracketright" => Some(Key::Character("]".to_owned())),
        "backtick" | "grave" => Some(Key::Character("`".to_owned())),
        _ => None,
    };
    if let Some(key) = named {
        return Ok(key);
    }
    if let Some(number) = normalized
        .strip_prefix('f')
        .and_then(|value| value.parse().ok())
        && (1..=24).contains(&number)
    {
        return Ok(Key::Function(number));
    }
    if component.chars().count() == 1 {
        return Ok(Key::Character(normalized));
    }
    Err(KeymapError::new(
        offset,
        format!("unknown key `{component}`"),
    ))
}

fn normalize_key(key: Key) -> Key {
    match key {
        Key::Character(value) => Key::Character(value.to_lowercase()),
        key => key,
    }
}

fn key_name(key: &Key) -> Arc<str> {
    match key {
        Key::Character(value) => Arc::from(value.as_str()),
        Key::ArrowUp => Arc::from("up"),
        Key::ArrowDown => Arc::from("down"),
        Key::ArrowLeft => Arc::from("left"),
        Key::ArrowRight => Arc::from("right"),
        Key::PageUp => Arc::from("pageup"),
        Key::PageDown => Arc::from("pagedown"),
        Key::Home => Arc::from("home"),
        Key::End => Arc::from("end"),
        Key::Enter => Arc::from("enter"),
        Key::Escape => Arc::from("escape"),
        Key::Space => Arc::from("space"),
        Key::Tab => Arc::from("tab"),
        Key::Backspace => Arc::from("backspace"),
        Key::Delete => Arc::from("delete"),
        Key::Insert => Arc::from("insert"),
        Key::Function(number) => Arc::from(format!("f{number}")),
        Key::Other => Arc::from("other"),
    }
}

/// Maximum UTF-8 bytes accepted for one Electron-style accelerator string.
pub const MAX_ACCELERATOR_BYTES: usize = 128;

/// Electron-compatible accelerator syntax parsed into QuickGUI's native [`Keystroke`].
///
/// Accelerators join modifiers and one key with `+`, for example `CmdOrCtrl+Shift+S`. They exist
/// so declarative menus, JavaScript hosts, and documentation can share one portable spelling;
/// QuickGUI keymaps continue to use the shorter `cmd-shift-s` [`Keystroke`] grammar internally.
///
/// Recognized modifiers are `CommandOrControl`/`CmdOrCtrl` (Command on macOS, Control elsewhere),
/// `Command`/`Cmd`, `Control`/`Ctrl`, `Alt`/`Option`, `AltGr`, `Shift`, and `Super`/`Meta`.
/// Recognized keys are `A`-`Z`, `0`-`9`, `F1`-`F24`, `Plus`, `Space`, `Tab`, `Capslock`,
/// `Numlock`, `Scrolllock`, `Backspace`, `Delete`, `Insert`, `Return`/`Enter`, `Up`, `Down`,
/// `Left`, `Right`, `Home`, `End`, `PageUp`, `PageDown`, `Escape`/`Esc`, `VolumeUp`,
/// `VolumeDown`, `VolumeMute`, `MediaNextTrack`, `MediaPreviousTrack`, `MediaStop`,
/// `MediaPlayPause`, `PrintScreen`, `num0`-`num9`, `numdec`, `numadd`, `numsub`, `nummult`,
/// `numdiv`, and any single punctuation character.
///
/// Keys QuickGUI's [`Key`] cannot name — the lock, media, volume, and print-screen keys — parse
/// to [`Key::Other`]. That keeps the declaration valid without inventing a synthetic identity,
/// and platforms which cannot render them simply show no key equivalent.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Accelerator;

impl Accelerator {
    /// Parse one bounded Electron accelerator string into a QuickGUI keystroke.
    pub fn parse(accelerator: &str) -> Result<Keystroke, KeymapError> {
        if accelerator.is_empty() {
            return Err(KeymapError::new(0, "an accelerator cannot be empty"));
        }
        if accelerator.len() > MAX_ACCELERATOR_BYTES {
            return Err(KeymapError::new(
                MAX_ACCELERATOR_BYTES,
                format!("an accelerator cannot exceed {MAX_ACCELERATOR_BYTES} UTF-8 bytes"),
            ));
        }
        if accelerator.contains('\0') {
            return Err(KeymapError::new(0, "an accelerator cannot contain NUL"));
        }

        let mut modifiers = Modifiers::empty();
        let mut key = None;
        let mut offset = 0;
        for component in accelerator.split('+') {
            let trimmed = component.trim();
            if trimmed.is_empty() {
                return Err(KeymapError::new(offset, "empty accelerator component"));
            }
            if let Some(modifier) = accelerator_modifier(trimmed) {
                modifiers.insert(modifier);
            } else if key.is_some() {
                return Err(KeymapError::new(
                    offset,
                    "an accelerator must contain exactly one non-modifier key",
                ));
            } else {
                key = Some(accelerator_key(trimmed, offset)?);
            }
            offset += component.len() + 1;
        }

        let key = key.ok_or_else(|| {
            KeymapError::new(accelerator.len(), "an accelerator is missing its key")
        })?;
        Ok(Keystroke::new(key, modifiers))
    }
}

fn accelerator_modifier(component: &str) -> Option<Modifiers> {
    match component.to_ascii_lowercase().as_str() {
        "commandorcontrol" | "cmdorctrl" => Some(if cfg!(target_os = "macos") {
            Modifiers::SUPER
        } else {
            Modifiers::CONTROL
        }),
        "command" | "cmd" | "super" | "meta" => Some(Modifiers::SUPER),
        "control" | "ctrl" => Some(Modifiers::CONTROL),
        "alt" | "option" | "altgr" => Some(Modifiers::ALT),
        "shift" => Some(Modifiers::SHIFT),
        _ => None,
    }
}

fn accelerator_key(component: &str, offset: usize) -> Result<Key, KeymapError> {
    let normalized = component.to_lowercase();
    let named = match normalized.as_str() {
        "plus" => Some(Key::Character("+".to_owned())),
        "space" => Some(Key::Space),
        "tab" => Some(Key::Tab),
        "backspace" => Some(Key::Backspace),
        "delete" | "del" => Some(Key::Delete),
        "insert" => Some(Key::Insert),
        "return" | "enter" => Some(Key::Enter),
        "up" => Some(Key::ArrowUp),
        "down" => Some(Key::ArrowDown),
        "left" => Some(Key::ArrowLeft),
        "right" => Some(Key::ArrowRight),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "pageup" => Some(Key::PageUp),
        "pagedown" => Some(Key::PageDown),
        "escape" | "esc" => Some(Key::Escape),
        // AppKit and Win32 name numeric-keypad keys through their printed character, exactly as
        // Electron resolves keypad accelerators for native menu key equivalents.
        "num0" => Some(Key::Character("0".to_owned())),
        "num1" => Some(Key::Character("1".to_owned())),
        "num2" => Some(Key::Character("2".to_owned())),
        "num3" => Some(Key::Character("3".to_owned())),
        "num4" => Some(Key::Character("4".to_owned())),
        "num5" => Some(Key::Character("5".to_owned())),
        "num6" => Some(Key::Character("6".to_owned())),
        "num7" => Some(Key::Character("7".to_owned())),
        "num8" => Some(Key::Character("8".to_owned())),
        "num9" => Some(Key::Character("9".to_owned())),
        "numdec" => Some(Key::Character(".".to_owned())),
        "numadd" => Some(Key::Character("+".to_owned())),
        "numsub" => Some(Key::Character("-".to_owned())),
        "nummult" => Some(Key::Character("*".to_owned())),
        "numdiv" => Some(Key::Character("/".to_owned())),
        "capslock" | "numlock" | "scrolllock" | "printscreen" | "volumeup" | "volumedown"
        | "volumemute" | "medianexttrack" | "mediaprevioustrack" | "mediastop"
        | "mediaplaypause" => Some(Key::Other),
        _ => None,
    };
    if let Some(key) = named {
        return Ok(key);
    }
    if let Some(number) = normalized
        .strip_prefix('f')
        .and_then(|value| value.parse::<u8>().ok())
        && (1..=24).contains(&number)
    {
        return Ok(Key::Function(number));
    }
    if component.chars().count() == 1 {
        return Ok(Key::Character(normalized));
    }
    Err(KeymapError::new(
        offset,
        format!("unknown accelerator key `{component}`"),
    ))
}

fn parse_keystroke_sequence(input: &str) -> Result<Vec<Keystroke>, KeymapError> {
    let mut keystrokes = Vec::new();
    for part in input.split_whitespace() {
        keystrokes.push(Keystroke::parse(part)?);
    }
    if keystrokes.is_empty() {
        return Err(KeymapError::new(0, "key binding cannot be empty"));
    }
    Ok(keystrokes)
}

/// Context values attached to one element in the focused ancestor path.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KeyContext {
    identifiers: Vec<Arc<str>>,
    values: Vec<(Arc<str>, Arc<str>)>,
}

impl KeyContext {
    pub fn parse(input: &str) -> Result<Self, KeymapError> {
        let mut context = Self::default();
        for (field, offset) in context_fields(input)? {
            if let Some((key, value)) = field.split_once('=') {
                validate_context_name(key, offset)?;
                let value_offset = offset + key.len() + 1;
                let value = decode_literal(value, value_offset)?;
                if value.is_empty() {
                    return Err(KeymapError::new(
                        value_offset,
                        "context value cannot be empty",
                    ));
                }
                context.set(key, value);
            } else {
                validate_context_name(&field, offset)?;
                context.add(field);
            }
        }
        Ok(context)
    }

    pub fn add(&mut self, identifier: impl Into<Arc<str>>) {
        let identifier = identifier.into();
        if !self.identifiers.iter().any(|entry| entry == &identifier) {
            self.identifiers.push(identifier);
        }
    }

    pub fn set(&mut self, key: impl Into<Arc<str>>, value: impl Into<Arc<str>>) {
        let key = key.into();
        let value = value.into();
        if let Some((_, existing)) = self.values.iter_mut().find(|(entry, _)| entry == &key) {
            *existing = value;
        } else {
            self.values.push((key, value));
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.identifiers.iter().any(|entry| entry.as_ref() == name)
            || self.values.iter().any(|(key, _)| key.as_ref() == name)
    }

    pub fn value(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find_map(|(entry, value)| (entry.as_ref() == key).then_some(value.as_ref()))
    }

    pub fn is_empty(&self) -> bool {
        self.identifiers.is_empty() && self.values.is_empty()
    }
}

impl From<&str> for KeyContext {
    fn from(value: &str) -> Self {
        Self::parse(value).unwrap_or_else(|error| panic!("invalid key context `{value}`: {error}"))
    }
}

impl From<String> for KeyContext {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

fn context_fields(input: &str) -> Result<Vec<(String, usize)>, KeymapError> {
    let mut fields = Vec::new();
    let mut start = None;
    let mut quote = None;
    let mut escaped = false;
    for (offset, character) in input.char_indices() {
        if start.is_none() {
            if character.is_whitespace() {
                continue;
            }
            start = Some(offset);
        }
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            }
            continue;
        }
        if character == '"' || character == '\'' {
            quote = Some(character);
        } else if character.is_whitespace() {
            let field_start = start.take().expect("field start is present");
            fields.push((input[field_start..offset].to_owned(), field_start));
        }
    }
    if quote.is_some() {
        return Err(KeymapError::new(
            input.len(),
            "unterminated quoted context value",
        ));
    }
    if let Some(field_start) = start {
        fields.push((input[field_start..].to_owned(), field_start));
    }
    Ok(fields)
}

fn validate_context_name(value: &str, offset: usize) -> Result<(), KeymapError> {
    if value.is_empty()
        || value.chars().any(|character| {
            character.is_whitespace()
                || matches!(
                    character,
                    '=' | '!' | '&' | '|' | '>' | '(' | ')' | '"' | '\''
                )
        })
    {
        return Err(KeymapError::new(offset, "invalid key context name"));
    }
    Ok(())
}

fn decode_literal(value: &str, offset: usize) -> Result<String, KeymapError> {
    let first = value.chars().next();
    if !matches!(first, Some('"' | '\'')) {
        return Ok(value.to_owned());
    }
    let quote = first.expect("quoted literal has a first character");
    if !value.ends_with(quote) || value.len() < quote.len_utf8() * 2 {
        return Err(KeymapError::new(offset, "unterminated quoted value"));
    }
    let inner = &value[quote.len_utf8()..value.len() - quote.len_utf8()];
    let mut decoded = String::with_capacity(inner.len());
    let mut characters = inner.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            let escaped = characters
                .next()
                .ok_or_else(|| KeymapError::new(offset + value.len(), "incomplete escape"))?;
            decoded.push(escaped);
        } else {
            decoded.push(character);
        }
    }
    Ok(decoded)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ContextAtom {
    Present(Arc<str>),
    Equal(Arc<str>, Arc<str>),
    NotEqual(Arc<str>, Arc<str>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PredicateExpr {
    Atom(ContextAtom),
    Not(Box<PredicateExpr>),
    And(Box<PredicateExpr>, Box<PredicateExpr>),
    Or(Box<PredicateExpr>, Box<PredicateExpr>),
    Descendant(Box<PredicateExpr>, Box<PredicateExpr>),
}

/// A boolean predicate evaluated against the root-to-focus key-context stack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPredicate(PredicateExpr);

impl ContextPredicate {
    pub fn parse(input: &str) -> Result<Self, KeymapError> {
        let tokens = lex_predicate(input)?;
        let mut parser = PredicateParser {
            input,
            tokens,
            index: 0,
        };
        let expression = parser.parse_or()?;
        if let Some(token) = parser.peek() {
            return Err(KeymapError::new(
                token.offset,
                "unexpected token after context predicate",
            ));
        }
        Ok(Self(expression))
    }

    /// Return the one-based depth of the deepest matching context.
    pub fn depth_of(&self, contexts: &[KeyContext]) -> Option<usize> {
        if contexts.is_empty() {
            let empty = KeyContext::default();
            return self
                .0
                .matches_at(std::slice::from_ref(&empty), 0)
                .then_some(0);
        }
        (0..contexts.len())
            .rev()
            .find(|depth| self.0.matches_at(contexts, *depth))
            .map(|depth| depth + 1)
    }
}

impl FromStr for ContextPredicate {
    type Err = KeymapError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl PredicateExpr {
    fn matches_at(&self, contexts: &[KeyContext], depth: usize) -> bool {
        match self {
            Self::Atom(atom) => atom.matches(&contexts[depth]),
            Self::Not(expression) => !expression.matches_at(contexts, depth),
            Self::And(left, right) => {
                left.matches_at(contexts, depth) && right.matches_at(contexts, depth)
            }
            Self::Or(left, right) => {
                left.matches_at(contexts, depth) || right.matches_at(contexts, depth)
            }
            Self::Descendant(ancestor, descendant) => {
                descendant.matches_at(contexts, depth)
                    && (0..depth)
                        .rev()
                        .any(|ancestor_depth| ancestor.matches_at(contexts, ancestor_depth))
            }
        }
    }
}

impl ContextAtom {
    fn matches(&self, context: &KeyContext) -> bool {
        match self {
            Self::Present(name) => context.contains(name),
            Self::Equal(key, value) => context.value(key) == Some(value),
            Self::NotEqual(key, value) => context.value(key) != Some(value),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PredicateTokenKind {
    Word(Arc<str>),
    Equal,
    NotEqual,
    And,
    Or,
    Greater,
    Not,
    LeftParen,
    RightParen,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PredicateToken {
    kind: PredicateTokenKind,
    offset: usize,
}

fn lex_predicate(input: &str) -> Result<Vec<PredicateToken>, KeymapError> {
    let mut tokens = Vec::new();
    let mut offset = 0;
    while offset < input.len() {
        let character = input[offset..]
            .chars()
            .next()
            .expect("offset stays on a character boundary");
        if character.is_whitespace() {
            offset += character.len_utf8();
            continue;
        }
        let remaining = &input[offset..];
        let (kind, consumed) = if remaining.starts_with("&&") {
            (PredicateTokenKind::And, 2)
        } else if remaining.starts_with("||") {
            (PredicateTokenKind::Or, 2)
        } else if remaining.starts_with("!=") {
            (PredicateTokenKind::NotEqual, 2)
        } else if remaining.starts_with("==") {
            (PredicateTokenKind::Equal, 2)
        } else {
            match character {
                '=' => (PredicateTokenKind::Equal, 1),
                '!' => (PredicateTokenKind::Not, 1),
                '>' => (PredicateTokenKind::Greater, 1),
                '(' => (PredicateTokenKind::LeftParen, 1),
                ')' => (PredicateTokenKind::RightParen, 1),
                '&' | '|' => {
                    return Err(KeymapError::new(
                        offset,
                        "boolean operators must use two characters",
                    ));
                }
                '"' | '\'' => {
                    let quote = character;
                    let mut end = offset + quote.len_utf8();
                    let mut escaped = false;
                    let mut closed = false;
                    while end < input.len() {
                        let next = input[end..]
                            .chars()
                            .next()
                            .expect("end stays on a character boundary");
                        end += next.len_utf8();
                        if escaped {
                            escaped = false;
                        } else if next == '\\' {
                            escaped = true;
                        } else if next == quote {
                            closed = true;
                            break;
                        }
                    }
                    if !closed {
                        return Err(KeymapError::new(offset, "unterminated quoted value"));
                    }
                    let value = decode_literal(&input[offset..end], offset)?;
                    (PredicateTokenKind::Word(Arc::from(value)), end - offset)
                }
                _ => {
                    let mut end = offset;
                    while end < input.len() {
                        let next = input[end..]
                            .chars()
                            .next()
                            .expect("end stays on a character boundary");
                        if next.is_whitespace()
                            || matches!(next, '=' | '!' | '&' | '|' | '>' | '(' | ')')
                        {
                            break;
                        }
                        end += next.len_utf8();
                    }
                    if end == offset {
                        return Err(KeymapError::new(offset, "invalid predicate token"));
                    }
                    (
                        PredicateTokenKind::Word(Arc::from(&input[offset..end])),
                        end - offset,
                    )
                }
            }
        };
        tokens.push(PredicateToken { kind, offset });
        offset += consumed;
    }
    if tokens.is_empty() {
        return Err(KeymapError::new(0, "context predicate cannot be empty"));
    }
    Ok(tokens)
}

struct PredicateParser<'a> {
    input: &'a str,
    tokens: Vec<PredicateToken>,
    index: usize,
}

impl PredicateParser<'_> {
    fn peek(&self) -> Option<&PredicateToken> {
        self.tokens.get(self.index)
    }

    fn advance(&mut self) -> Option<PredicateToken> {
        let token = self.tokens.get(self.index).cloned();
        self.index += usize::from(token.is_some());
        token
    }

    fn consume(&mut self, expected: PredicateTokenKind) -> bool {
        if self.peek().is_some_and(|token| token.kind == expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn parse_or(&mut self) -> Result<PredicateExpr, KeymapError> {
        let mut expression = self.parse_and()?;
        while self.consume(PredicateTokenKind::Or) {
            expression = PredicateExpr::Or(Box::new(expression), Box::new(self.parse_and()?));
        }
        Ok(expression)
    }

    fn parse_and(&mut self) -> Result<PredicateExpr, KeymapError> {
        let mut expression = self.parse_descendant()?;
        while self.consume(PredicateTokenKind::And) {
            expression =
                PredicateExpr::And(Box::new(expression), Box::new(self.parse_descendant()?));
        }
        Ok(expression)
    }

    fn parse_descendant(&mut self) -> Result<PredicateExpr, KeymapError> {
        let mut expression = self.parse_unary()?;
        while self.consume(PredicateTokenKind::Greater) {
            expression =
                PredicateExpr::Descendant(Box::new(expression), Box::new(self.parse_unary()?));
        }
        Ok(expression)
    }

    fn parse_unary(&mut self) -> Result<PredicateExpr, KeymapError> {
        if self.consume(PredicateTokenKind::Not) {
            Ok(PredicateExpr::Not(Box::new(self.parse_unary()?)))
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<PredicateExpr, KeymapError> {
        if self.consume(PredicateTokenKind::LeftParen) {
            let expression = self.parse_or()?;
            if !self.consume(PredicateTokenKind::RightParen) {
                let offset = self.peek().map_or(self.input.len(), |token| token.offset);
                return Err(KeymapError::new(offset, "expected `)`"));
            }
            return Ok(expression);
        }

        let token = self
            .advance()
            .ok_or_else(|| KeymapError::new(self.input.len(), "expected context name"))?;
        let PredicateTokenKind::Word(name) = token.kind else {
            return Err(KeymapError::new(token.offset, "expected context name"));
        };
        let atom = if self.consume(PredicateTokenKind::Equal) {
            ContextAtom::Equal(name, self.parse_comparison_value()?)
        } else if self.consume(PredicateTokenKind::NotEqual) {
            ContextAtom::NotEqual(name, self.parse_comparison_value()?)
        } else {
            ContextAtom::Present(name)
        };
        Ok(PredicateExpr::Atom(atom))
    }

    fn parse_comparison_value(&mut self) -> Result<Arc<str>, KeymapError> {
        let token = self
            .advance()
            .ok_or_else(|| KeymapError::new(self.input.len(), "expected comparison value"))?;
        if let PredicateTokenKind::Word(value) = token.kind {
            Ok(value)
        } else {
            Err(KeymapError::new(token.offset, "expected comparison value"))
        }
    }
}

/// One typed action binding, optionally constrained to the focused context path.
#[derive(Clone, Debug)]
pub struct KeyBinding {
    keystrokes: Vec<Keystroke>,
    declared_keystrokes: Option<Arc<[Keystroke]>>,
    action: AnyAction,
    context_predicate: Option<ContextPredicate>,
}

impl KeyBinding {
    #[track_caller]
    pub fn new<A: Action>(keystrokes: &str, action: A, context: Option<&str>) -> Self {
        Self::try_new(keystrokes, action, context)
            .unwrap_or_else(|error| panic!("invalid key binding `{keystrokes}`: {error}"))
    }

    pub fn try_new<A: Action>(
        keystrokes: &str,
        action: A,
        context: Option<&str>,
    ) -> Result<Self, KeymapError> {
        Ok(Self {
            keystrokes: parse_keystroke_sequence(keystrokes)?,
            declared_keystrokes: None,
            action: AnyAction::new(action),
            context_predicate: context.map(ContextPredicate::parse).transpose()?,
        })
    }

    /// Opt this binding into the operating system's localized key-equivalent policy.
    ///
    /// On macOS, characters that are difficult to reach on the active layout are remapped to
    /// Apple's localized equivalent. For example, `cmd-[` becomes `cmd-ö` on a German layout.
    /// QuickGUI retains the original declaration and remaps it only when the native keyboard
    /// layout changes. Other platforms currently leave the declaration unchanged.
    pub fn use_key_equivalents(mut self) -> Self {
        if self.declared_keystrokes.is_none() {
            self.declared_keystrokes = Some(Arc::from(self.keystrokes.clone()));
        }
        self
    }

    /// Disable localized key equivalents and restore the original declaration.
    pub fn without_key_equivalents(mut self) -> Self {
        if let Some(declared) = self.declared_keystrokes.take() {
            self.keystrokes = declared.as_ref().to_vec();
        }
        self
    }

    pub fn uses_key_equivalents(&self) -> bool {
        self.declared_keystrokes.is_some()
    }

    pub fn keystrokes(&self) -> &[Keystroke] {
        &self.keystrokes
    }

    pub fn action(&self) -> &AnyAction {
        &self.action
    }

    pub fn context_predicate(&self) -> Option<&ContextPredicate> {
        self.context_predicate.as_ref()
    }

    fn apply_key_equivalents(&mut self, equivalents: &'static [(char, char)]) {
        let Some(declared) = self.declared_keystrokes.as_ref() else {
            return;
        };
        self.keystrokes.clear();
        self.keystrokes
            .extend(declared.iter().cloned().map(|mut stroke| {
                if let Key::Character(value) = &stroke.key
                    && let Some(character) = single_character(value)
                    && let Some((_, equivalent)) = equivalents
                        .iter()
                        .find(|(declared, _)| *declared == character)
                {
                    stroke.key = normalize_key(Key::Character(equivalent.to_string()));
                }
                stroke
            }));
    }

    fn match_keystrokes(&self, input: &[Keystroke]) -> Option<bool> {
        if input.len() > self.keystrokes.len()
            || !input
                .iter()
                .zip(&self.keystrokes)
                .all(|(typed, target)| typed.should_match(target))
        {
            return None;
        }
        Some(input.len() < self.keystrokes.len())
    }
}

/// The enabled bindings for an input prefix, in dispatch precedence order.
#[derive(Clone, Debug, Default)]
pub struct KeymapMatch {
    pub bindings: Vec<KeyBinding>,
    pub pending: bool,
}

/// Ordered application key bindings. Later bindings win at equal context depth.
#[derive(Clone, Debug, Default)]
pub struct Keymap {
    bindings: Vec<KeyBinding>,
    binding_indices_by_first_stroke: HashMap<Keystroke, Vec<usize>>,
    key_equivalents: &'static [(char, char)],
}

impl Keymap {
    pub fn new(bindings: Vec<KeyBinding>) -> Self {
        let mut keymap = Self::default();
        keymap.add_bindings(bindings);
        keymap
    }

    pub fn add_bindings(&mut self, bindings: impl IntoIterator<Item = KeyBinding>) {
        for mut binding in bindings {
            binding.apply_key_equivalents(self.key_equivalents);
            let index = self.bindings.len();
            let first = binding
                .keystrokes
                .first()
                .expect("key bindings always contain at least one stroke")
                .clone();
            self.bindings.push(binding);
            self.binding_indices_by_first_stroke
                .entry(first)
                .or_default()
                .push(index);
        }
    }

    pub fn clear(&mut self) {
        self.bindings.clear();
        self.binding_indices_by_first_stroke.clear();
    }

    pub(crate) fn set_key_equivalents(&mut self, equivalents: &'static [(char, char)]) {
        if self.key_equivalents == equivalents {
            return;
        }
        self.key_equivalents = equivalents;
        for binding in &mut self.bindings {
            binding.apply_key_equivalents(equivalents);
        }
        self.rebuild_first_stroke_index();
    }

    pub fn bindings(&self) -> &[KeyBinding] {
        &self.bindings
    }

    pub fn bindings_for_action<A: Action>(&self) -> impl DoubleEndedIterator<Item = &KeyBinding> {
        let action_type = TypeId::of::<A>();
        self.bindings
            .iter()
            .filter(move |binding| binding.action.type_id() == action_type)
    }

    /// Return the active single-stroke shortcut suitable for displaying beside an action.
    ///
    /// Context depth and load order use the same precedence as dispatch. A keystroke is omitted if
    /// another action shadows it or if it is currently the prefix of a stronger multi-stroke
    /// binding, preventing a native menu from stealing input that belongs to the keymap.
    pub fn shortcut_for_action<A: Action>(
        &self,
        action: &A,
        contexts: &[KeyContext],
    ) -> Option<Keystroke> {
        self.shortcut_for_action_value(&AnyAction::new(action.clone()), contexts)
    }

    pub(crate) fn shortcut_for_action_value(
        &self,
        action: &AnyAction,
        contexts: &[KeyContext],
    ) -> Option<Keystroke> {
        let mut candidates = self
            .bindings
            .iter()
            .enumerate()
            .filter(|(_, binding)| {
                binding.keystrokes.len() == 1 && binding.action.partial_eq(action)
            })
            .filter_map(|(index, binding)| {
                binding_enabled(binding, contexts).map(|depth| (depth, index, binding))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));

        candidates.into_iter().find_map(|(_, _, binding)| {
            let shortcut = binding.keystrokes[0].clone();
            let matched = self.bindings_for_input(std::slice::from_ref(&shortcut), contexts);
            (!matched.pending
                && matched
                    .bindings
                    .first()
                    .is_some_and(|binding| binding.action.partial_eq(action)))
            .then_some(shortcut)
        })
    }

    pub fn bindings_for_input(&self, input: &[Keystroke], contexts: &[KeyContext]) -> KeymapMatch {
        let Some(first) = input.first() else {
            return KeymapMatch::default();
        };
        let mut exact = Vec::new();
        let mut pending = Vec::new();
        self.for_each_binding_index_for_first(first, |index| {
            let binding = &self.bindings[index];
            let Some(depth) = binding_enabled(binding, contexts) else {
                return;
            };
            match binding.match_keystrokes(input) {
                Some(false) => exact.push((depth, index, binding)),
                Some(true) => pending.push(index),
                None => {}
            }
        });
        exact.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
        let strongest_exact = exact.first().map(|(_, index, _)| *index);
        let pending = pending
            .into_iter()
            .any(|index| strongest_exact.is_none_or(|exact| index > exact));
        KeymapMatch {
            bindings: exact
                .into_iter()
                .map(|(_, _, binding)| binding.clone())
                .collect(),
            pending,
        }
    }

    pub fn possible_next_bindings_for_input(
        &self,
        input: &[Keystroke],
        contexts: &[KeyContext],
    ) -> Vec<KeyBinding> {
        let Some(first) = input.first() else {
            return Vec::new();
        };
        let mut matches = Vec::new();
        self.for_each_binding_index_for_first(first, |index| {
            let binding = &self.bindings[index];
            let Some(depth) = binding_enabled(binding, contexts) else {
                return;
            };
            if binding.match_keystrokes(input) == Some(true) {
                matches.push((depth, index, binding));
            }
        });
        matches.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
        matches
            .into_iter()
            .map(|(_, _, binding)| binding.clone())
            .collect()
    }

    fn for_each_binding_index_for_first(&self, first: &Keystroke, mut visit: impl FnMut(usize)) {
        if let Some(indices) = self.binding_indices_by_first_stroke.get(first) {
            for &index in indices {
                visit(index);
            }
        }
        if let Some(key_char) = first.key_char_identity()
            && let Some(indices) = self.binding_indices_by_first_stroke.get(&key_char)
        {
            for &index in indices {
                visit(index);
            }
        }
    }

    fn rebuild_first_stroke_index(&mut self) {
        self.binding_indices_by_first_stroke.clear();
        for (index, binding) in self.bindings.iter().enumerate() {
            let first = binding
                .keystrokes
                .first()
                .expect("key bindings always contain at least one stroke")
                .clone();
            self.binding_indices_by_first_stroke
                .entry(first)
                .or_default()
                .push(index);
        }
    }
}

fn single_character(value: &str) -> Option<char> {
    let mut characters = value.chars();
    let character = characters.next()?;
    characters.next().is_none().then_some(character)
}

fn binding_enabled(binding: &KeyBinding, contexts: &[KeyContext]) -> Option<usize> {
    binding
        .context_predicate
        .as_ref()
        .map_or(Some(contexts.len()), |predicate| {
            predicate.depth_of(contexts)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct WorkspaceAction;

    #[derive(Clone, Debug, PartialEq)]
    struct EditorAction;

    #[derive(Clone, Debug, PartialEq)]
    struct OpenLine(u32);

    #[test]
    fn parses_and_normalizes_common_keystrokes() {
        assert_eq!(
            Keystroke::parse("cmd-shift-Z").unwrap(),
            Keystroke::new(
                Key::Character("z".to_owned()),
                Modifiers::SUPER | Modifiers::SHIFT,
            )
        );
        assert_eq!(Keystroke::parse("ctrl-left").unwrap().key, Key::ArrowLeft);
        assert_eq!(Keystroke::parse("f12").unwrap().key, Key::Function(12));
        assert!(Keystroke::parse("cmd-shift").is_err());
        assert!(Keystroke::parse("cmd-not-a-key").is_err());
    }

    #[test]
    fn printable_key_char_matches_without_option_or_shift_modifiers() {
        let keymap = Keymap::new(vec![KeyBinding::new("å", WorkspaceAction, None)]);
        let typed = Keystroke::new(Key::Character("a".to_owned()), Modifiers::ALT)
            .with_key_char(Key::Character("Å".to_owned()));
        let matched = keymap.bindings_for_input(&[typed], &[]);
        assert_eq!(matched.bindings.len(), 1);

        let command = Keystroke::new(
            Key::Character("a".to_owned()),
            Modifiers::SUPER | Modifiers::ALT,
        )
        .with_key_char(Key::Character("å".to_owned()));
        assert!(
            keymap
                .bindings_for_input(&[command], &[])
                .bindings
                .is_empty()
        );
    }

    #[test]
    fn alternate_first_strokes_keep_the_keymap_index_narrow() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("{ left", WorkspaceAction, None),
            KeyBinding::new("a", EditorAction, None),
        ]);
        let first = Keystroke::new(Key::Character("8".to_owned()), Modifiers::ALT)
            .with_key_char(Key::Character("{".to_owned()));
        let matched = keymap.bindings_for_input(&[first], &[]);
        assert!(matched.pending);
        assert!(matched.bindings.is_empty());
        assert_eq!(keymap.possible_next_bindings_for_input(&[], &[]).len(), 0);
    }

    #[test]
    fn localized_key_equivalents_are_opt_in_and_rebuilt_from_the_declaration() {
        static GERMAN: &[(char, char)] = &[('[', 'ö'), (']', 'ä')];
        static FRENCH: &[(char, char)] = &[('[', '^'), (']', '$')];

        let mut keymap = Keymap::new(vec![
            KeyBinding::new("cmd-[", WorkspaceAction, None),
            KeyBinding::new("cmd-[ left", EditorAction, None).use_key_equivalents(),
            KeyBinding::new("cmd-]", OpenLine(7), None).use_key_equivalents(),
        ]);
        assert!(!keymap.bindings()[0].uses_key_equivalents());
        assert!(keymap.bindings()[1].uses_key_equivalents());

        keymap.set_key_equivalents(GERMAN);
        assert_eq!(
            keymap.bindings()[1].keystrokes()[0],
            Keystroke::parse("cmd-ö").unwrap()
        );
        assert_eq!(
            keymap.shortcut_for_action(&OpenLine(7), &[]),
            Some(Keystroke::parse("cmd-ä").unwrap())
        );
        let german = keymap.bindings_for_input(&[Keystroke::parse("cmd-ö").unwrap()], &[]);
        assert!(german.pending);
        assert!(german.bindings.is_empty());
        assert_eq!(
            keymap.possible_next_bindings_for_input(&[Keystroke::parse("cmd-ö").unwrap()], &[])[0]
                .action()
                .downcast_ref::<EditorAction>(),
            Some(&EditorAction)
        );
        assert_eq!(
            keymap
                .bindings_for_input(&[Keystroke::parse("cmd-[").unwrap()], &[])
                .bindings[0]
                .action()
                .downcast_ref::<WorkspaceAction>(),
            Some(&WorkspaceAction)
        );

        keymap.set_key_equivalents(FRENCH);
        assert_eq!(
            keymap.bindings()[1].keystrokes()[0],
            Keystroke::parse("cmd-^").unwrap()
        );
        assert!(
            keymap
                .bindings_for_input(&[Keystroke::parse("cmd-ö").unwrap()], &[])
                .bindings
                .is_empty()
        );

        keymap.set_key_equivalents(&[]);
        assert_eq!(
            keymap.bindings()[1].keystrokes()[0],
            Keystroke::parse("cmd-[").unwrap()
        );
    }

    #[test]
    fn disabling_key_equivalents_restores_the_original_binding() {
        let binding = KeyBinding::new("cmd-[", WorkspaceAction, None)
            .use_key_equivalents()
            .without_key_equivalents();
        assert!(!binding.uses_key_equivalents());
        assert_eq!(binding.keystrokes(), &[Keystroke::parse("cmd-[").unwrap()]);
    }

    #[test]
    fn parses_context_values_and_quoted_strings() {
        let context = KeyContext::parse("Editor mode=insert language='rust source'").unwrap();
        assert!(context.contains("Editor"));
        assert_eq!(context.value("mode"), Some("insert"));
        assert_eq!(context.value("language"), Some("rust source"));
    }

    #[test]
    fn predicates_resolve_boolean_and_descendant_contexts() {
        let contexts = [
            KeyContext::parse("Workspace").unwrap(),
            KeyContext::parse("Pane active=true").unwrap(),
            KeyContext::parse("Editor mode=insert").unwrap(),
        ];
        assert_eq!(
            ContextPredicate::parse("Workspace > Editor && mode == insert")
                .unwrap()
                .depth_of(&contexts),
            Some(3)
        );
        assert_eq!(
            ContextPredicate::parse("Pane && active != false")
                .unwrap()
                .depth_of(&contexts),
            Some(2)
        );
        assert_eq!(
            ContextPredicate::parse("Editor && !readonly")
                .unwrap()
                .depth_of(&contexts),
            Some(3)
        );
        assert_eq!(
            ContextPredicate::parse("Terminal || Dialog")
                .unwrap()
                .depth_of(&contexts),
            None
        );
    }

    #[test]
    fn deeper_context_then_later_binding_determine_precedence() {
        let mut keymap = Keymap::default();
        keymap.add_bindings([
            KeyBinding::new("cmd-s", WorkspaceAction, Some("Workspace")),
            KeyBinding::new("cmd-s", WorkspaceAction, None),
            KeyBinding::new("cmd-s", EditorAction, Some("Editor")),
        ]);
        let contexts = [
            KeyContext::parse("Workspace").unwrap(),
            KeyContext::parse("Editor").unwrap(),
        ];
        let matched = keymap.bindings_for_input(&[Keystroke::parse("cmd-s").unwrap()], &contexts);
        assert!(!matched.pending);
        assert!(
            matched.bindings[0]
                .action
                .downcast_ref::<EditorAction>()
                .is_some()
        );
        assert!(
            matched.bindings[1]
                .action
                .downcast_ref::<WorkspaceAction>()
                .is_some()
        );
    }

    #[test]
    fn later_multistroke_binding_defers_an_exact_prefix() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("ctrl-k", WorkspaceAction, None),
            KeyBinding::new("ctrl-k left", EditorAction, None),
        ]);
        let matched = keymap.bindings_for_input(&[Keystroke::parse("ctrl-k").unwrap()], &[]);
        assert!(matched.pending);
        assert_eq!(matched.bindings.len(), 1);

        let reversed = Keymap::new(vec![
            KeyBinding::new("ctrl-k left", EditorAction, None),
            KeyBinding::new("ctrl-k", WorkspaceAction, None),
        ]);
        assert!(
            !reversed
                .bindings_for_input(&[Keystroke::parse("ctrl-k").unwrap()], &[])
                .pending
        );
    }

    #[test]
    fn displayed_shortcuts_follow_dispatch_precedence() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("cmd-s", WorkspaceAction, None),
            KeyBinding::new("cmd-shift-s", WorkspaceAction, None),
            KeyBinding::new("cmd-s", EditorAction, Some("Editor")),
        ]);
        let editor = [KeyContext::parse("Editor").unwrap()];

        assert_eq!(
            keymap.shortcut_for_action(&EditorAction, &editor),
            Some(Keystroke::parse("cmd-s").unwrap())
        );
        assert_eq!(
            keymap.shortcut_for_action(&WorkspaceAction, &editor),
            Some(Keystroke::parse("cmd-shift-s").unwrap())
        );
        assert_eq!(
            keymap.shortcut_for_action(&WorkspaceAction, &[]),
            Some(Keystroke::parse("cmd-shift-s").unwrap())
        );
    }

    #[test]
    fn displayed_shortcuts_do_not_steal_multistroke_prefixes() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("cmd-k", WorkspaceAction, None),
            KeyBinding::new("cmd-k left", EditorAction, None),
        ]);

        assert_eq!(keymap.shortcut_for_action(&WorkspaceAction, &[]), None);
    }

    #[test]
    fn displayed_shortcuts_match_action_payloads_not_only_types() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("cmd-1", OpenLine(1), None),
            KeyBinding::new("cmd-2", OpenLine(2), None),
        ]);

        assert_eq!(
            keymap.shortcut_for_action(&OpenLine(1), &[]),
            Some(Keystroke::parse("cmd-1").unwrap())
        );
        assert_eq!(
            keymap.shortcut_for_action(&OpenLine(2), &[]),
            Some(Keystroke::parse("cmd-2").unwrap())
        );
    }
    #[test]
    fn electron_accelerators_parse_modifiers_and_named_keys() {
        let platform = if cfg!(target_os = "macos") {
            Modifiers::SUPER
        } else {
            Modifiers::CONTROL
        };
        for (accelerator, expected) in [
            (
                "CmdOrCtrl+Shift+S",
                Keystroke::new(Key::Character("s".to_owned()), platform | Modifiers::SHIFT),
            ),
            (
                "CommandOrControl+O",
                Keystroke::new(Key::Character("o".to_owned()), platform),
            ),
            (
                "Cmd+Alt+I",
                Keystroke::new(
                    Key::Character("i".to_owned()),
                    Modifiers::SUPER | Modifiers::ALT,
                ),
            ),
            (
                "Control+Option+Shift+F12",
                Keystroke::new(
                    Key::Function(12),
                    Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT,
                ),
            ),
            ("Super+Space", Keystroke::new(Key::Space, Modifiers::SUPER)),
            ("Meta+Return", Keystroke::new(Key::Enter, Modifiers::SUPER)),
            ("Esc", Keystroke::new(Key::Escape, Modifiers::empty())),
            (
                "Alt+PageDown",
                Keystroke::new(Key::PageDown, Modifiers::ALT),
            ),
            ("AltGr+Up", Keystroke::new(Key::ArrowUp, Modifiers::ALT)),
            (
                "Shift+Backspace",
                Keystroke::new(Key::Backspace, Modifiers::SHIFT),
            ),
            (
                "Cmd+Plus",
                Keystroke::new(Key::Character("+".to_owned()), Modifiers::SUPER),
            ),
            (
                "Cmd+numadd",
                Keystroke::new(Key::Character("+".to_owned()), Modifiers::SUPER),
            ),
            (
                "Cmd+num7",
                Keystroke::new(Key::Character("7".to_owned()), Modifiers::SUPER),
            ),
            (
                "Ctrl+numdiv",
                Keystroke::new(Key::Character("/".to_owned()), Modifiers::CONTROL),
            ),
            (
                "Cmd+,",
                Keystroke::new(Key::Character(",".to_owned()), Modifiers::SUPER),
            ),
            ("Tab", Keystroke::new(Key::Tab, Modifiers::empty())),
            ("Delete", Keystroke::new(Key::Delete, Modifiers::empty())),
            ("Insert", Keystroke::new(Key::Insert, Modifiers::empty())),
            ("Home", Keystroke::new(Key::Home, Modifiers::empty())),
            ("End", Keystroke::new(Key::End, Modifiers::empty())),
            ("Left", Keystroke::new(Key::ArrowLeft, Modifiers::empty())),
            ("Right", Keystroke::new(Key::ArrowRight, Modifiers::empty())),
            ("Down", Keystroke::new(Key::ArrowDown, Modifiers::empty())),
            ("PageUp", Keystroke::new(Key::PageUp, Modifiers::empty())),
        ] {
            assert_eq!(
                Accelerator::parse(accelerator).expect(accelerator),
                expected,
                "accelerator {accelerator}"
            );
        }
    }

    #[test]
    fn unnameable_accelerator_keys_stay_valid_but_opaque() {
        for accelerator in [
            "VolumeUp",
            "VolumeDown",
            "VolumeMute",
            "MediaNextTrack",
            "MediaPreviousTrack",
            "MediaStop",
            "MediaPlayPause",
            "PrintScreen",
            "Capslock",
            "Numlock",
            "Scrolllock",
        ] {
            assert_eq!(
                Accelerator::parse(accelerator).expect(accelerator).key,
                Key::Other
            );
        }
    }

    #[test]
    fn accelerator_parsing_is_bounded_and_rejects_malformed_input() {
        assert!(Accelerator::parse("").is_err());
        assert!(Accelerator::parse("Cmd+").is_err());
        assert!(Accelerator::parse("Cmd").is_err());
        assert!(Accelerator::parse("Cmd+S+T").is_err());
        assert!(Accelerator::parse("Cmd+Unknown").is_err());
        assert!(Accelerator::parse("Cmd+F25").is_err());
        assert!(Accelerator::parse("Cmd+\0").is_err());
        assert!(Accelerator::parse(&"a".repeat(MAX_ACCELERATOR_BYTES + 1)).is_err());
    }
}
