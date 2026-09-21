//! Stored shortcut overrides translate at the canvas boundary, leaving text inputs native.
use super::*;
use compositor::invalid;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Chord {
    key: String,
    modifiers: u8,
}
impl Chord {
    pub fn from_event(key: &Key, modifiers: Modifiers) -> Option<Self> {
        let key = match key {
            Key::Character(value) if value.chars().count() == 1 => match value.as_str() {
                "{" => "[".into(),
                "}" => "]".into(),
                "+" => "=".into(),
                "_" => "-".into(),
                _ => value.to_lowercase(),
            },
            Key::Space => "Space".into(),
            Key::Enter => "Enter".into(),
            Key::Escape => "Escape".into(),
            Key::Backspace | Key::Delete => "Delete".into(),
            Key::Tab => "Tab".into(),
            Key::ArrowLeft => "Left".into(),
            Key::ArrowRight => "Right".into(),
            Key::ArrowUp => "Up".into(),
            Key::ArrowDown => "Down".into(),
            Key::Function(number) if (1..=24).contains(number) => format!("F{number}"),
            _ => return None,
        };
        Some(Self {
            key,
            modifiers: u8::from(modifiers.contains(Modifiers::CONTROL))
                | (u8::from(modifiers.contains(Modifiers::ALT)) << 1)
                | (u8::from(modifiers.contains(Modifiers::SHIFT)) << 2)
                | (u8::from(modifiers.contains(Modifiers::SUPER)) << 3),
        })
    }
    pub fn parse(mut label: &str) -> Result<Self> {
        let mut modifiers = 0;
        loop {
            let flag = if label.starts_with("Ctrl+") {
                Some((1, 5))
            } else if label.starts_with("Alt+") {
                Some((2, 4))
            } else if label.starts_with("Shift+") {
                Some((4, 6))
            } else if label.starts_with("Super+") {
                Some((8, 6))
            } else {
                None
            };
            let Some((flag, length)) = flag else {
                break;
            };
            modifiers |= flag;
            label = &label[length..];
        }
        let key = match label {
            "+" => "=".into(),
            "Backspace" => "Delete".into(),
            _ if label.chars().count() == 1 => label.to_lowercase(),
            _ => label.into(),
        };
        let chord = Self { key, modifiers };
        chord.validate()?;
        Ok(chord)
    }
    fn validate(&self) -> Result<()> {
        if self.modifiers > 15 || self.event_key().is_none() {
            return Err(invalid(
                "Choose one key with optional Ctrl, Alt, Shift or Super modifiers.",
            ));
        }
        Ok(())
    }
    fn event_key(&self) -> Option<Key> {
        Some(match self.key.as_str() {
            "Space" => Key::Space,
            "Enter" => Key::Enter,
            "Escape" => Key::Escape,
            "Delete" => Key::Backspace,
            "Tab" => Key::Tab,
            "Left" => Key::ArrowLeft,
            "Right" => Key::ArrowRight,
            "Up" => Key::ArrowUp,
            "Down" => Key::ArrowDown,
            value
                if value.starts_with('F')
                    && value[1..]
                        .parse::<u8>()
                        .ok()
                        .is_some_and(|n| (1..=24).contains(&n)) =>
            {
                Key::Function(value[1..].parse().ok()?)
            }
            value if value.chars().count() == 1 && !value.chars().any(char::is_control) => {
                Key::Character(value.into())
            }
            _ => return None,
        })
    }
    pub fn event(&self) -> Option<(Key, Modifiers)> {
        let mut flags = Modifiers::empty();
        for (bit, flag) in [
            (1, Modifiers::CONTROL),
            (2, Modifiers::ALT),
            (4, Modifiers::SHIFT),
            (8, Modifiers::SUPER),
        ] {
            if self.modifiers & bit != 0 {
                flags |= flag;
            }
        }
        Some((self.event_key()?, flags))
    }
    pub fn label(&self) -> String {
        let mut label = String::new();
        for (bit, name) in [(1, "Ctrl+"), (2, "Alt+"), (4, "Shift+"), (8, "Super+")] {
            if self.modifiers & bit != 0 {
                label.push_str(name);
            }
        }
        label.push_str(
            if self.key.chars().count() == 1 {
                self.key.to_uppercase()
            } else {
                self.key.clone()
            }
            .as_str(),
        );
        label
    }
    fn reserved(&self) -> bool {
        (self.modifiers == 1 && ["q", ","].contains(&self.key.as_str()))
            || (self.modifiers == 2
                && ["f", "e", "v", "s", "i", "t", "l", "h", "F4"].contains(&self.key.as_str()))
            || (self.modifiers == 0 && self.key == "F10")
    }
}

#[derive(Clone)]
pub(super) struct Definition {
    pub id: String,
    pub group: &'static str,
    pub title: String,
    pub original: Chord,
}
impl Definition {
    fn new(group: &'static str, title: impl Into<String>, chord: &str) -> Self {
        let title = title.into();
        Self {
            id: format!("{group}:{title}"),
            group,
            title,
            original: Chord::parse(chord).expect("Built-in shortcut must be valid"),
        }
    }
}
pub(super) fn definitions() -> Vec<Definition> {
    let mut values: Vec<_> = super::menus::shortcut_definitions()
        .into_iter()
        .filter(|(_, key)| !["Ctrl+Q", "Delete"].contains(key))
        .map(|(name, key)| Definition::new("Menus", name, key))
        .collect();
    values.push(Definition::new("Menus", "Redo alternative", "Ctrl+Y"));
    for (title, key) in [
        ("Select tool", "A"),
        ("Move tool", "V"),
        ("Marquee", "M"),
        ("Lasso", "L"),
        ("Magic Wand", "W"),
        ("Object Selection", "O"),
        ("Crop", "C"),
        ("Brush", "B"),
        ("Eraser", "E"),
        ("Clone Stamp", "S"),
        ("Healing", "J"),
        ("Blur / Smudge / Liquify", "R"),
        ("Gradient", "G"),
        ("Shape", "U"),
        ("Type", "T"),
        ("Eyedropper", "I"),
        ("Hand", "H"),
        ("Zoom", "Z"),
        ("Swap colors", "X"),
        ("Reset colors", "D"),
        ("Temporary Hand (hold)", "Space"),
        ("Apply canvas operation", "Enter"),
        ("Cancel canvas operation", "Escape"),
        ("Delete selection / layer / lasso point", "Delete"),
        ("Decrease brush size", "["),
        ("Increase brush size", "]"),
        ("Decrease brush hardness", "Shift+["),
        ("Increase brush hardness", "Shift+]"),
        ("Previous blend mode", "Shift+-"),
        ("Next blend mode", "Shift+="),
        ("Cycle shape kind", "Shift+U"),
        ("Ungroup", "Ctrl+Shift+G"),
        ("Toggle Levels preview", "Alt+P"),
    ] {
        values.push(Definition::new("Canvas", title, key));
    }
    for digit in 0..=9 {
        values.push(Definition::new(
            "Canvas",
            format!("Opacity digit {digit}"),
            &digit.to_string(),
        ));
    }
    for direction in ["Left", "Right", "Up", "Down"] {
        for (prefix, verb) in [
            ("", "Nudge 1 px"),
            ("Shift+", "Nudge 10 px"),
            ("Ctrl+", "Move pixels 1 px"),
            ("Ctrl+Shift+", "Move pixels 10 px"),
        ] {
            values.push(Definition::new(
                "Canvas",
                format!("{verb} {direction}"),
                &format!("{prefix}{direction}"),
            ));
        }
    }
    values.push(Definition::new("Text", "Apply text", "Ctrl+Enter"));
    for (key, title) in [
        ("Left", "Decrease tracking"),
        ("Right", "Increase tracking"),
        ("Up", "Decrease leading"),
        ("Down", "Increase leading"),
    ] {
        values.push(Definition::new("Text", title, &format!("Alt+{key}")));
        values.push(Definition::new(
            "Text",
            format!("{title} by 10"),
            &format!("Alt+Shift+{key}"),
        ));
    }
    values
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Keymap {
    overrides: BTreeMap<String, Chord>,
}
impl Keymap {
    pub fn chord(&self, definition: &Definition) -> Chord {
        self.overrides
            .get(&definition.id)
            .unwrap_or(&definition.original)
            .clone()
    }
    pub fn set(&mut self, definition: &Definition, chord: Chord) {
        if chord == definition.original {
            self.overrides.remove(&definition.id);
        } else {
            self.overrides.insert(definition.id.clone(), chord);
        }
    }
    pub fn validate(&self) -> Result<()> {
        let definitions = definitions();
        if self
            .overrides
            .keys()
            .any(|id| !definitions.iter().any(|d| &d.id == id))
        {
            return Err(invalid(
                "Shortcut settings reference an unknown command. Reset shortcuts to restore the defaults.",
            ));
        }
        let mut assigned = HashMap::new();
        for definition in &definitions {
            let chord = self.chord(definition);
            chord.validate()?;
            if chord.reserved() {
                return Err(invalid(format!(
                    "{} is reserved for window or application menus.",
                    chord.label()
                )));
            }
            if definition.group == "Text" && chord.modifiers & 11 == 0 {
                return Err(invalid(
                    "Text editing shortcuts need Ctrl, Alt or Super so they do not replace typing.",
                ));
            }
            if let Some(previous) = assigned.insert(chord.clone(), definition.title.as_str()) {
                return Err(invalid(format!(
                    "{} is assigned to both {previous} and {}. Choose another key or reset one command.",
                    chord.label(),
                    definition.title
                )));
            }
        }
        Ok(())
    }
    pub fn translate(
        &self,
        key: &Key,
        modifiers: Modifiers,
        text_edit: bool,
    ) -> Option<(Key, Modifiers)> {
        if self.overrides.is_empty() {
            return Some((key.clone(), modifiers));
        }
        let Some(input) = Chord::from_event(key, modifiers) else {
            return Some((key.clone(), modifiers));
        };
        let definitions = definitions();
        if let Some(definition) = definitions
            .iter()
            .find(|d| (d.group == "Text") == text_edit && self.chord(d) == input)
        {
            return definition.original.event();
        }
        if definitions.iter().any(|d| {
            (d.group == "Text") == text_edit && d.original == input && self.chord(d) != input
        }) {
            return None;
        }
        if !text_edit && input.modifiers == 4 {
            let plain = Chord {
                modifiers: 0,
                ..input.clone()
            };
            if let Some(definition) = definitions.iter().find(|d| {
                d.group == "Canvas" && d.original.modifiers == 0 && self.chord(d) == plain
            }) {
                return Chord {
                    modifiers: 4,
                    ..definition.original.clone()
                }
                .event();
            }
            if definitions
                .iter()
                .any(|d| d.group == "Canvas" && d.original == plain && self.chord(d) != plain)
            {
                return None;
            }
        }
        Some((key.clone(), modifiers))
    }
    pub fn menu_label(&self, original: &str) -> String {
        if self.overrides.is_empty() || original.is_empty() {
            return original.into();
        }
        definitions()
            .iter()
            .find(|d| {
                d.group == "Menus" && Chord::parse(original).ok().as_ref() == Some(&d.original)
            })
            .map_or_else(
                || original.into(),
                |d| {
                    if self.chord(d) == d.original {
                        original.into()
                    } else {
                        self.chord(d).label()
                    }
                },
            )
    }
    pub fn pan_released(&self, key: &Key) -> bool {
        let Some(key) = Chord::from_event(key, Modifiers::empty()) else {
            return false;
        };
        definitions()
            .iter()
            .find(|definition| {
                definition.original.key == "Space" && definition.original.modifiers == 0
            })
            .is_some_and(|definition| self.chord(definition).key == key.key)
    }
    pub fn is_pan(&self, key: &Key, modifiers: Modifiers) -> bool {
        self.translate(key, modifiers, false) == Some((Key::Space, Modifiers::empty()))
    }
    pub fn read(path: &Path) -> Result<Self> {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take(65_537).read_to_end(&mut bytes)?;
        if bytes.len() > 65_536 {
            return Err(invalid("Shortcut settings exceed 64 KiB."));
        }
        let keymap: Self = serde_json::from_slice(&bytes)?;
        keymap.validate()?;
        Ok(keymap)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let directory = path
            .parent()
            .ok_or_else(|| invalid("Shortcut settings have no parent directory."))?;
        std::fs::create_dir_all(directory)?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        temporary.write_all(&serde_json::to_vec_pretty(self)?)?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
