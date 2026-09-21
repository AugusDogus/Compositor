use crate::KeyBinding;

const SELECT_KEY_CONTEXT: &str = "Select";
const COMBOBOX_KEY_CONTEXT: &str = "Combobox";

/// Move to the previous enabled suggestion.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ComboboxPrevious;
/// Move to the next enabled suggestion.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ComboboxNext;
/// Move one visible suggestion page upward.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ComboboxPageUp;
/// Move one visible suggestion page downward.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ComboboxPageDown;
/// Move to the first enabled select option.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ComboboxFirst;
/// Move to the final enabled select option.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ComboboxLast;
/// Open a closed control or commit its active option.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ComboboxConfirm;

/// Contextual bindings for the standalone non-editable [`crate::SelectState`].
pub fn select_key_bindings() -> [KeyBinding; 8] {
    [
        KeyBinding::new("up", ComboboxPrevious, Some(SELECT_KEY_CONTEXT)),
        KeyBinding::new("down", ComboboxNext, Some(SELECT_KEY_CONTEXT)),
        KeyBinding::new("pageup", ComboboxPageUp, Some(SELECT_KEY_CONTEXT)),
        KeyBinding::new("pagedown", ComboboxPageDown, Some(SELECT_KEY_CONTEXT)),
        KeyBinding::new("platform-up", ComboboxFirst, Some(SELECT_KEY_CONTEXT)),
        KeyBinding::new("platform-down", ComboboxLast, Some(SELECT_KEY_CONTEXT)),
        KeyBinding::new("enter", ComboboxConfirm, Some(SELECT_KEY_CONTEXT)),
        KeyBinding::new("space", ComboboxConfirm, Some(SELECT_KEY_CONTEXT)),
    ]
}

/// Contextual bindings for editable constrained comboboxes and free-form autocompletes.
///
/// Left, Right, Home, End, deletion, selection, clipboard, undo, and IME keys intentionally
/// remain with the ordinary single-line text editor.
pub fn combobox_key_bindings() -> [KeyBinding; 5] {
    [
        KeyBinding::new("up", ComboboxPrevious, Some(COMBOBOX_KEY_CONTEXT)),
        KeyBinding::new("down", ComboboxNext, Some(COMBOBOX_KEY_CONTEXT)),
        KeyBinding::new("pageup", ComboboxPageUp, Some(COMBOBOX_KEY_CONTEXT)),
        KeyBinding::new("pagedown", ComboboxPageDown, Some(COMBOBOX_KEY_CONTEXT)),
        KeyBinding::new("enter", ComboboxConfirm, Some(COMBOBOX_KEY_CONTEXT)),
    ]
}
