use crate::{
    AccessibilityOrientation, AccessibilityRole, Element, ElementId, EventContext, FocusHandle,
    KeyBinding, StateAccessor, ViewContext, div,
};

/// Maximum slots one OTP field retains.
///
/// The value is a fixed-size array, so the bound is the retained size of every OTP field rather
/// than a policy check. It comfortably covers the four-, six-, and eight-character codes real
/// verification flows use.
pub const MAX_OTP_LENGTH: usize = 12;

/// Key context used by [`otp_field_key_bindings`].
pub const OTP_FIELD_KEY_CONTEXT: &str = "OtpField";

const OTP_FIELD_INPUT_ID_TAG: u64 = 0x8b25_e0c7_49fd_1a36;
const OTP_FIELD_SEPARATOR_ID_TAG: u64 = 0x60f7_ac13_d582_9e4b;

/// Move OTP focus to the previous slot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OtpFieldPrevious;
/// Move OTP focus to the next slot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OtpFieldNext;
/// Move OTP focus to the first slot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OtpFieldFirst;
/// Move OTP focus to the last slot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OtpFieldLast;
/// Clear the focused slot, or clear the previous one and move back when it is already empty.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OtpFieldBackspace;
/// Clear the focused slot without moving.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OtpFieldDelete;

/// Contextual bindings used by [`OtpField::slot_with`].
///
/// QuickGUI matches contextual bindings before an ordinary text input's own editing, so the OTP
/// field owns Backspace, Delete, the arrows, and Home/End inside its slots while every other key
/// still reaches the composed [`crate::text_input`].
pub fn otp_field_key_bindings() -> [KeyBinding; 6] {
    [
        KeyBinding::new("left", OtpFieldPrevious, Some(OTP_FIELD_KEY_CONTEXT)),
        KeyBinding::new("right", OtpFieldNext, Some(OTP_FIELD_KEY_CONTEXT)),
        KeyBinding::new("home", OtpFieldFirst, Some(OTP_FIELD_KEY_CONTEXT)),
        KeyBinding::new("end", OtpFieldLast, Some(OTP_FIELD_KEY_CONTEXT)),
        KeyBinding::new("backspace", OtpFieldBackspace, Some(OTP_FIELD_KEY_CONTEXT)),
        KeyBinding::new("delete", OtpFieldDelete, Some(OTP_FIELD_KEY_CONTEXT)),
    ]
}

/// Which characters one OTP field accepts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OtpValidationType {
    /// ASCII digits only, the default for verification codes.
    #[default]
    Numeric,
    /// Alphabetic characters only.
    Alpha,
    /// Alphabetic characters and digits.
    Alphanumeric,
    /// Any single non-control character.
    None,
}

impl OtpValidationType {
    /// Whether one typed or pasted character may enter a slot.
    pub fn accepts(self, character: char) -> bool {
        if character.is_control() || character.is_whitespace() {
            return false;
        }
        match self {
            Self::Numeric => character.is_ascii_digit(),
            Self::Alpha => character.is_alphabetic(),
            Self::Alphanumeric => character.is_alphanumeric(),
            Self::None => true,
        }
    }
}

/// Controlled, allocation-free value state for one OTP field.
///
/// The state owns the per-slot characters, the declared length, the accepted character class, the
/// focused slot, and the disabled/read-only/required policy. The application owns every visual
/// declaration and the listener that reacts to a change. It retains no allocation, task, timer,
/// observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OtpFieldState {
    slots: [Option<char>; MAX_OTP_LENGTH],
    length: usize,
    validation: OtpValidationType,
    mask: bool,
    disabled: bool,
    read_only: bool,
    required: bool,
    focused: usize,
}

impl Default for OtpFieldState {
    fn default() -> Self {
        Self::new(6)
    }
}

impl OtpFieldState {
    /// Declare an OTP field of `length` slots.
    ///
    /// # Panics
    ///
    /// Panics when `length` is zero or larger than [`MAX_OTP_LENGTH`].
    pub fn new(length: usize) -> Self {
        assert!(length > 0, "an OTP field needs at least one slot");
        assert!(
            length <= MAX_OTP_LENGTH,
            "an OTP field retains at most {MAX_OTP_LENGTH} slots"
        );
        Self {
            slots: [None; MAX_OTP_LENGTH],
            length,
            validation: OtpValidationType::Numeric,
            mask: false,
            disabled: false,
            read_only: false,
            required: false,
            focused: 0,
        }
    }

    /// Declare the initial value, keeping only accepted characters and only as many as fit.
    #[must_use]
    pub fn value(mut self, value: &str) -> Self {
        self.set_value(value);
        self
    }

    #[must_use]
    pub const fn validation_type(mut self, validation: OtpValidationType) -> Self {
        self.validation = validation;
        self
    }

    /// Present the entered characters as a password would be presented.
    #[must_use]
    pub const fn mask(mut self, mask: bool) -> Self {
        self.mask = mask;
        self
    }

    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[must_use]
    pub const fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    #[must_use]
    pub const fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub const fn length(&self) -> usize {
        self.length
    }

    pub const fn validation(&self) -> OtpValidationType {
        self.validation
    }

    pub const fn is_masked(&self) -> bool {
        self.mask
    }

    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub const fn is_required(&self) -> bool {
        self.required
    }

    pub const fn focused_index(&self) -> usize {
        self.focused
    }

    /// The character in one slot, or `None` while it is empty.
    pub fn slot(&self, index: usize) -> Option<char> {
        (index < self.length).then(|| self.slots[index]).flatten()
    }

    /// One slot's controlled text, for building its [`crate::text_input`].
    pub fn slot_text(&self, index: usize) -> String {
        self.slot(index).map(String::from).unwrap_or_default()
    }

    /// The complete entered value, at most [`MAX_OTP_LENGTH`] characters.
    ///
    /// Empty slots contribute nothing, so a partially entered code is shorter than its length.
    pub fn text(&self) -> String {
        self.slots[..self.length].iter().flatten().collect()
    }

    /// Whether every slot is filled.
    pub fn is_complete(&self) -> bool {
        self.slots[..self.length].iter().all(|slot| slot.is_some())
    }

    /// Whether the field satisfies its declared validity policy.
    pub fn is_valid(&self) -> bool {
        !self.required || self.is_complete()
    }

    /// Replace the whole value, returning whether anything changed.
    ///
    /// Rejected characters are skipped rather than shifting the rest of the code, and the focus
    /// moves to the first empty slot.
    pub fn set_value(&mut self, value: &str) -> bool {
        let mut slots = [None; MAX_OTP_LENGTH];
        let mut index = 0;
        for character in value.chars() {
            if index == self.length {
                break;
            }
            if self.validation.accepts(character) {
                slots[index] = Some(character);
                index += 1;
            }
        }
        let focused = index.min(self.length - 1);
        if self.slots == slots && self.focused == focused {
            return false;
        }
        self.slots = slots;
        self.focused = focused;
        true
    }

    /// Move the focused slot, clamped to the declared length. Returns whether it changed.
    pub fn set_focused_index(&mut self, index: usize) -> bool {
        let index = index.min(self.length - 1);
        if self.focused == index {
            return false;
        }
        self.focused = index;
        true
    }

    /// Move focus one slot toward the start.
    pub fn focus_previous(&mut self) -> bool {
        self.set_focused_index(self.focused.saturating_sub(1))
    }

    /// Move focus one slot toward the end.
    pub fn focus_next(&mut self) -> bool {
        self.set_focused_index(self.focused + 1)
    }

    /// Move focus to the first slot.
    pub fn focus_first(&mut self) -> bool {
        self.set_focused_index(0)
    }

    /// Move focus to the last slot.
    pub fn focus_last(&mut self) -> bool {
        self.set_focused_index(self.length - 1)
    }

    /// Apply one text-input event for a slot, distributing pasted characters across the field.
    ///
    /// A single accepted character fills the slot and advances; several accepted characters — the
    /// paste case — fill consecutive slots from `index`. Clearing a slot's text empties it without
    /// moving. Returns whether the retained value or focus changed.
    pub fn apply_input(&mut self, index: usize, text: &str) -> bool {
        if self.disabled || self.read_only || index >= self.length {
            return false;
        }
        if text.is_empty() {
            return self.clear_slot(index) | self.set_focused_index(index);
        }
        let previous = self.slots[index];
        let mut typed: Vec<char> = text
            .chars()
            .take(MAX_OTP_LENGTH * 2)
            .filter(|character| self.validation.accepts(*character))
            .collect();
        // The composed input still holds the character this slot already showed. Dropping one
        // occurrence of it leaves exactly what the user just typed or pasted, which is what makes
        // typing over a filled slot replace it instead of being rejected as too long.
        if typed.len() > 1
            && let Some(existing) = previous
            && let Some(position) = typed.iter().position(|character| *character == existing)
        {
            typed.remove(position);
        }
        if typed.is_empty() {
            return false;
        }
        let mut changed = false;
        let mut cursor = index;
        for character in typed {
            if cursor == self.length {
                break;
            }
            if self.slots[cursor] != Some(character) {
                self.slots[cursor] = Some(character);
                changed = true;
            }
            cursor += 1;
        }
        changed | self.set_focused_index(cursor.min(self.length - 1))
    }

    /// Clear one slot, returning whether it changed.
    pub fn clear_slot(&mut self, index: usize) -> bool {
        if self.disabled || self.read_only || index >= self.length {
            return false;
        }
        if self.slots[index].is_none() {
            return false;
        }
        self.slots[index] = None;
        true
    }

    /// Apply Backspace at the focused slot.
    ///
    /// A filled slot is emptied in place; an already empty slot moves focus back and empties the
    /// slot it lands on. Returns whether anything changed.
    pub fn backspace(&mut self) -> bool {
        if self.disabled || self.read_only {
            return false;
        }
        if self.slots[self.focused].is_some() {
            return self.clear_slot(self.focused);
        }
        let moved = self.focus_previous();
        moved | self.clear_slot(self.focused)
    }

    /// Empty every slot and return focus to the first, reporting whether anything changed.
    pub fn clear(&mut self) -> bool {
        if self.disabled || self.read_only {
            return false;
        }
        let changed = self.slots[..self.length].iter().any(Option::is_some);
        self.slots = [None; MAX_OTP_LENGTH];
        changed | self.set_focused_index(0)
    }
}

/// A controlled, unstyled OTP-field descriptor.
///
/// The application owns the slot boxes, separators, caret styling, and layout. QuickGUI supplies
/// stable part identities, the Group role with per-slot position semantics, one-character slots
/// composed from the existing [`crate::text_input`], automatic advance, Backspace and arrow
/// behavior, paste distribution, validity, and optional submission of the caller's nearest form.
///
/// The descriptor retains no allocation, task, timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use = "an OtpField descriptor has no effect until its parts are mounted"]
pub struct OtpField {
    root_id: ElementId,
    auto_submit: Option<ElementId>,
}

impl OtpField {
    pub fn new(root_id: impl Into<ElementId>) -> Self {
        Self {
            root_id: root_id.into(),
            auto_submit: None,
        }
    }

    /// Submit the named form as soon as the final slot is filled.
    ///
    /// QuickGUI routes the request through [`crate::EventContext::submit_form`], so the form is
    /// validated exactly as a Return press or a submit button would validate it. Pass the id of
    /// the nearest mounted [`crate::form`] ancestor.
    pub fn auto_submit(mut self, form: impl Into<ElementId>) -> Self {
        self.auto_submit = Some(form.into());
        self
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub const fn submitted_form(self) -> Option<ElementId> {
        self.auto_submit
    }

    pub fn input_id(self, index: usize) -> ElementId {
        derived_otp_id(self.root_id, OTP_FIELD_INPUT_ID_TAG, index as u64)
    }

    pub fn separator_id(self, index: usize) -> ElementId {
        derived_otp_id(self.root_id, OTP_FIELD_SEPARATOR_ID_TAG, index as u64)
    }

    /// Decorate an application-owned root without adding layout or appearance.
    ///
    /// The root is the group that names the whole code and carries its validity, so assistive
    /// technology announces one field rather than a row of unrelated one-character inputs.
    pub fn root_with(self, state: &OtpFieldState, root: Element) -> Element {
        root.id(self.root_id)
            .accessibility_role(AccessibilityRole::Group)
            .accessibility_orientation(AccessibilityOrientation::Horizontal)
            .invalid(!state.is_valid())
            .disabled(state.is_disabled())
            .app_region_no_drag()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self, state: &OtpFieldState) -> Element {
        self.root_with(state, crate::div())
    }

    /// Decorate one application-owned slot input without adding appearance.
    ///
    /// Pass a [`crate::text_input`] built from [`OtpFieldState::slot_text`]. QuickGUI adds the
    /// stable identity, the slot's position in the code, the masked presentation, and the required
    /// and disabled policy. Use [`Self::slot_with`] to attach the behavior as well.
    pub fn input_with(self, state: &OtpFieldState, index: usize, input: Element) -> Element {
        input
            .id(self.input_id(index))
            .max_length(MAX_OTP_LENGTH)
            .password(state.is_masked())
            .required(state.is_required())
            .disabled(state.is_disabled())
            .invalid(!state.is_valid())
            .accessibility_position_in_set(index)
            .accessibility_size_of_set(state.length())
            .key_context(OTP_FIELD_KEY_CONTEXT)
            .app_region_no_drag()
    }
    /// Create the unstyled input part. Use [`Self::input_with`] to supply an existing element.
    pub fn input(self, state: &OtpFieldState, index: usize) -> Element {
        self.input_with(state, index, crate::text_input(""))
    }

    /// Decorate an application-owned separator between two slots.
    ///
    /// The separator is decorative: it carries a stable identity and the Separator role, and is
    /// hidden from assistive technology so it never interrupts the announced code.
    pub fn separator_with(self, index: usize, separator: Element) -> Element {
        separator
            .id(self.separator_id(index))
            .accessibility_role(AccessibilityRole::Separator)
            .accessibility_hidden(true)
            .user_select_none()
    }
    /// Create the unstyled separator part. Use [`Self::separator_with`] to supply an existing element.
    pub fn separator(self, index: usize) -> Element {
        self.separator_with(index, crate::div())
    }

    /// Decorate one slot and attach its complete behavior.
    ///
    /// This registers the slot's input listener and its typed keyboard actions. Install
    /// [`otp_field_key_bindings`] once on the application keymap. `on_value_change` runs for every
    /// change with the whole code; `on_complete` runs only on the transition into a full code, and
    /// is followed by [`Self::auto_submit`] when one is declared.
    #[allow(clippy::too_many_arguments)]
    pub fn slot_with<V: 'static, Change, Complete>(
        self,
        cx: &mut ViewContext<'_, V>,
        state: &OtpFieldState,
        index: usize,
        input: Element,
        access: fn(&mut V) -> &mut OtpFieldState,
        on_value_change: Change,
        on_complete: Complete,
    ) -> Element
    where
        Change: Fn(&mut V, &str, &mut EventContext) + Clone + 'static,
        Complete: Fn(&mut V, &str, &mut EventContext) + Clone + 'static,
    {
        self.slot_with_accessor(
            cx,
            state,
            index,
            input,
            StateAccessor::from(access),
            on_value_change,
            on_complete,
        )
    }
    /// Create the unstyled slot part. Use [`Self::slot_with`] to supply an existing element.
    pub fn slot<V: 'static, Change, Complete>(
        self,
        cx: &mut ViewContext<'_, V>,
        state: &OtpFieldState,
        index: usize,
        access: fn(&mut V) -> &mut OtpFieldState,
        on_value_change: Change,
        on_complete: Complete,
    ) -> Element
    where
        Change: Fn(&mut V, &str, &mut EventContext) + Clone + 'static,
        Complete: Fn(&mut V, &str, &mut EventContext) + Clone + 'static,
    {
        self.slot_with(
            cx,
            state,
            index,
            crate::text_input(""),
            access,
            on_value_change,
            on_complete,
        )
    }

    /// Decorate one slot and attach its behavior against a per-instance state accessor.
    #[allow(clippy::too_many_arguments)]
    pub fn slot_with_accessor<V: 'static, Change, Complete>(
        self,
        cx: &mut ViewContext<'_, V>,
        state: &OtpFieldState,
        index: usize,
        input: Element,
        access: StateAccessor<V, OtpFieldState>,
        on_value_change: Change,
        on_complete: Complete,
    ) -> Element
    where
        Change: Fn(&mut V, &str, &mut EventContext) + Clone + 'static,
        Complete: Fn(&mut V, &str, &mut EventContext) + Clone + 'static,
    {
        let id = self.input_id(index);

        let input_access = access.clone();
        let input_change = on_value_change.clone();
        let input_complete = on_complete.clone();
        let input_listener = cx.input_listener(id, move |view, text, cx| {
            let before = input_access.get(view).is_complete();
            if !input_access.get(view).apply_input(index, text) {
                return;
            }
            self.finish(
                view,
                cx,
                &input_access,
                before,
                &input_change,
                &input_complete,
            );
        });

        let previous = self.movement_listener::<V, OtpFieldPrevious>(
            cx,
            id,
            index,
            &access,
            OtpFieldState::focus_previous,
        );
        let next = self.movement_listener::<V, OtpFieldNext>(
            cx,
            id,
            index,
            &access,
            OtpFieldState::focus_next,
        );
        let first = self.movement_listener::<V, OtpFieldFirst>(
            cx,
            id,
            index,
            &access,
            OtpFieldState::focus_first,
        );
        let last = self.movement_listener::<V, OtpFieldLast>(
            cx,
            id,
            index,
            &access,
            OtpFieldState::focus_last,
        );

        let backspace_access = access.clone();
        let backspace_change = on_value_change.clone();
        let backspace_complete = on_complete.clone();
        let backspace = cx.action_listener(id, move |view, _: &OtpFieldBackspace, cx| {
            let before = backspace_access.get(view).is_complete();
            backspace_access.get(view).set_focused_index(index);
            if !backspace_access.get(view).backspace() {
                cx.focus(FocusHandle::new(
                    self.input_id(backspace_access.get(view).focused_index()),
                ));
                return;
            }
            self.finish(
                view,
                cx,
                &backspace_access,
                before,
                &backspace_change,
                &backspace_complete,
            );
        });

        let delete_access = access.clone();
        let delete_change = on_value_change;
        let delete_complete = on_complete;
        let delete = cx.action_listener(id, move |view, _: &OtpFieldDelete, cx| {
            let before = delete_access.get(view).is_complete();
            delete_access.get(view).set_focused_index(index);
            if !delete_access.get(view).clear_slot(index) {
                return;
            }
            self.finish(
                view,
                cx,
                &delete_access,
                before,
                &delete_change,
                &delete_complete,
            );
        });

        self.input_with(state, index, input)
            .on_input(input_listener)
            .on_action(previous)
            .on_action(next)
            .on_action(first)
            .on_action(last)
            .on_action(backspace)
            .on_action(delete)
    }

    fn movement_listener<V: 'static, A: crate::Action>(
        self,
        cx: &mut ViewContext<'_, V>,
        id: ElementId,
        index: usize,
        access: &StateAccessor<V, OtpFieldState>,
        movement: fn(&mut OtpFieldState) -> bool,
    ) -> crate::ActionListener<V, A> {
        let access = access.clone();
        cx.action_listener(id, move |view, _: &A, cx| {
            let state = access.get(view);
            state.set_focused_index(index);
            let changed = movement(state);
            let focused = state.focused_index();
            cx.focus(FocusHandle::new(self.input_id(focused)));
            if changed {
                cx.invalidate();
            }
        })
    }

    fn finish<V: 'static, Change, Complete>(
        self,
        view: &mut V,
        cx: &mut EventContext,
        access: &StateAccessor<V, OtpFieldState>,
        was_complete: bool,
        on_value_change: &Change,
        on_complete: &Complete,
    ) where
        Change: Fn(&mut V, &str, &mut EventContext),
        Complete: Fn(&mut V, &str, &mut EventContext),
    {
        let state = *access.get(view);
        cx.focus(FocusHandle::new(self.input_id(state.focused_index())));
        let text = state.text();
        on_value_change(view, &text, cx);
        if state.is_complete() && !was_complete {
            on_complete(view, &text, cx);
            if let Some(form) = self.auto_submit {
                cx.submit_form(form);
            }
        }
        cx.invalidate();
    }
}

/// Create an unstyled OTP-field root.
///
/// This shorthand is equivalent to `OtpField::new(id).root_with(state, div())`.
pub fn otp_field(id: impl Into<ElementId>, state: &OtpFieldState) -> Element {
    OtpField::new(id).root_with(state, div())
}

fn derived_otp_id(scope: ElementId, tag: u64, index: u64) -> ElementId {
    let mut hash = scope
        .as_u64()
        .rotate_left(31)
        .wrapping_add(index.rotate_right(17))
        ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == u64::MAX || hash == scope.as_u64() {
        hash ^= tag.rotate_left(19);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Application, Color, IntoElement, KeyContext, View, WindowOptions, form, text, text_input,
    };

    #[test]
    fn typing_paste_and_backspace_stay_bounded() {
        let mut state = OtpFieldState::new(4);
        assert_eq!(state.length(), 4);
        assert_eq!(state.text(), "");
        assert!(!state.is_complete());
        assert!(state.is_valid(), "an optional field is valid while empty");

        assert!(state.apply_input(0, "1"));
        assert_eq!(state.text(), "1");
        assert_eq!(state.focused_index(), 1);
        assert!(!state.apply_input(1, "a"), "letters are rejected");

        assert!(state.apply_input(1, "2"));
        assert!(state.apply_input(2, "3"));
        assert_eq!(state.focused_index(), 3);

        // Typing over a filled slot replaces it: the composed input still carries the old
        // character, which is dropped before the typed one is applied.
        assert!(state.apply_input(0, "19"));
        assert_eq!(state.text(), "923");
        assert_eq!(state.focused_index(), 1);

        // Backspace clears in place, then walks back.
        assert!(state.set_focused_index(2));
        assert!(state.backspace());
        assert_eq!(state.text(), "92");
        assert_eq!(state.focused_index(), 2);
        assert!(state.backspace());
        assert_eq!(state.text(), "9");
        assert_eq!(state.focused_index(), 1);

        // A paste distributes across slots from the focused index and stops at the end.
        assert!(state.apply_input(1, "45678"));
        assert_eq!(state.text(), "9456");
        assert!(state.is_complete());
        assert_eq!(state.focused_index(), 3);

        assert!(state.focus_first());
        assert!(!state.focus_previous());
        assert!(state.focus_last());
        assert!(!state.focus_next());
        assert!(state.clear());
        assert_eq!(state.text(), "");
        assert_eq!(state.focused_index(), 0);

        assert!(state.apply_input(0, "7"));
        assert!(state.apply_input(0, ""));
        assert_eq!(state.text(), "");

        let required = OtpFieldState::new(2).required(true);
        assert!(!required.is_valid());
        assert!(required.is_required());

        let mut alpha = OtpFieldState::new(3).validation_type(OtpValidationType::Alpha);
        assert!(!alpha.apply_input(0, "1"));
        assert!(alpha.apply_input(0, "q"));
        let mut anything = OtpFieldState::new(2).validation_type(OtpValidationType::None);
        assert!(anything.apply_input(0, "!"));
        assert!(!anything.apply_input(1, "\n"));
        let mut alnum = OtpFieldState::new(2).validation_type(OtpValidationType::Alphanumeric);
        assert!(alnum.apply_input(0, "z"));
        assert!(alnum.apply_input(1, "8"));

        let mut read_only = OtpFieldState::new(2).read_only(true).value("12");
        assert_eq!(read_only.text(), "12");
        assert!(!read_only.apply_input(0, "9"));
        assert!(!read_only.backspace());
        assert!(!read_only.clear());
        assert!(read_only.is_read_only());

        let mut disabled = OtpFieldState::new(2).disabled(true);
        assert!(!disabled.apply_input(0, "1"));
        assert!(disabled.is_disabled());

        let preset = OtpFieldState::new(4).value("1a2b3");
        assert_eq!(preset.text(), "123", "rejected characters are skipped");
        assert_eq!(preset.slot(0), Some('1'));
        assert_eq!(preset.slot(9), None);
        assert_eq!(preset.slot_text(1), "2");
        assert_eq!(preset.slot_text(3), "");
        assert!(OtpFieldState::new(4).mask(true).is_masked());
        assert_eq!(OtpFieldState::default().length(), 6);
    }

    #[test]
    #[should_panic(expected = "at least one slot")]
    fn empty_fields_are_rejected() {
        let _ = OtpFieldState::new(0);
    }

    #[test]
    #[should_panic(expected = "at most")]
    fn oversized_fields_are_rejected() {
        let _ = OtpFieldState::new(MAX_OTP_LENGTH + 1);
    }

    #[test]
    fn parts_add_exact_semantics_without_appearance() {
        let state = OtpFieldState::new(4).required(true).value("12");
        let field = OtpField::new("code").auto_submit("verify");
        let root = field.root_with(&state, div().bg(Color::rgb8(1, 2, 3)));
        assert_eq!(root.explicit_id, Some("code".into()));
        assert_eq!(root.accessibility.role, AccessibilityRole::Group);
        assert_eq!(
            root.accessibility.orientation,
            Some(AccessibilityOrientation::Horizontal)
        );
        assert!(root.accessibility.invalid, "an incomplete required code");
        assert_eq!(root.visual.background, Some(Color::rgb8(1, 2, 3)));
        assert_eq!(field.submitted_form(), Some("verify".into()));

        let input = field.input_with(&state, 1, text_input(state.slot_text(1)));
        assert_eq!(input.explicit_id, Some(field.input_id(1)));
        assert_eq!(input.accessibility.collection.position_in_set, 1);
        assert_eq!(input.accessibility.collection.size_of_set, 4);
        assert!(input.accessibility.required);
        assert!(input.accessibility.invalid);

        let masked =
            OtpField::new("code").input_with(&OtpFieldState::new(4).mask(true), 0, text_input(""));
        assert_eq!(masked.accessibility.role, AccessibilityRole::PasswordInput);

        let separator = field.separator_with(0, div());
        assert_eq!(separator.explicit_id, Some(field.separator_id(0)));
        assert_eq!(separator.accessibility.role, AccessibilityRole::Separator);
        assert!(separator.accessibility.hidden);

        let ids = [
            field.root_id(),
            field.input_id(0),
            field.input_id(1),
            field.separator_id(0),
            field.separator_id(1),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(*id, ElementId::new(0));
            assert_ne!(*id, ElementId::new(u64::MAX));
            assert!(!ids[..index].contains(id));
        }

        let shorthand = otp_field("code", &state);
        assert_eq!(shorthand.accessibility.role, AccessibilityRole::Group);
        assert!(shorthand.children.is_empty());
    }

    #[test]
    fn bindings_are_contextual_and_complete() {
        let bindings = otp_field_key_bindings();
        assert_eq!(bindings.len(), 6);
        assert!(bindings.iter().all(|binding| {
            binding.context_predicate().is_some_and(|context| {
                context
                    .depth_of(&[KeyContext::parse(OTP_FIELD_KEY_CONTEXT).unwrap()])
                    .is_some()
            })
        }));
    }

    struct OtpView {
        code: OtpFieldState,
        values: Vec<String>,
        completed: Vec<String>,
        submitted: usize,
    }

    impl OtpView {
        fn code(view: &mut Self) -> &mut OtpFieldState {
            &mut view.code
        }
    }

    impl View for OtpView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let field = OtpField::new("code").auto_submit("verify");
            let submit = cx.form_submit_listener("verify", |view, _, _| {
                view.submitted += 1;
            });
            let mut root = field.root_with(&self.code, div().flex_row());
            for index in 0..self.code.length() {
                if index > 0 {
                    root = root.child(field.separator_with(index - 1, text("-")));
                }
                root = root.child(field.slot_with(
                    cx,
                    &self.code,
                    index,
                    text_input(self.code.slot_text(index)).w(24.0),
                    Self::code,
                    |view, value, _| view.values.push(value.to_owned()),
                    |view, value, _| view.completed.push(value.to_owned()),
                ));
            }
            form().id("verify").on_form_submit(submit).child(root)
        }
    }

    #[test]
    fn slots_advance_clear_and_auto_submit() {
        let (mut cx, view) = Application::new()
            .bind_keys(otp_field_key_bindings())
            .into_test_context(
                WindowOptions::default(),
                OtpView {
                    code: OtpFieldState::new(4),
                    values: Vec::new(),
                    completed: Vec::new(),
                    submitted: 0,
                },
            )
            .unwrap();
        let window = view.window_handle();
        let field = OtpField::new("code");

        cx.focus(window, field.input_id(0)).unwrap();
        cx.simulate_input(window, "1").unwrap();
        assert_eq!(cx.read(view, |view| view.code.text()).unwrap(), "1");
        assert_eq!(
            cx.focused(window).unwrap(),
            Some(field.input_id(1)),
            "an accepted character advances to the next slot"
        );

        cx.simulate_input(window, "2").unwrap();
        cx.simulate_input(window, "3").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(field.input_id(3)));

        // Backspace empties the slot it lands on and walks back.
        cx.simulate_keystrokes(window, "backspace").unwrap();
        assert_eq!(cx.read(view, |view| view.code.text()).unwrap(), "12");
        assert_eq!(cx.focused(window).unwrap(), Some(field.input_id(2)));

        cx.simulate_keystrokes(window, "left").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(field.input_id(1)));
        cx.simulate_keystrokes(window, "end").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(field.input_id(3)));
        cx.simulate_keystrokes(window, "home").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(field.input_id(0)));

        // A paste into the first slot fills the code and submits the nearest form once.
        cx.simulate_input(window, "9876").unwrap();
        assert_eq!(cx.read(view, |view| view.code.text()).unwrap(), "9876");
        assert_eq!(
            cx.read(view, |view| view.completed.clone()).unwrap(),
            vec!["9876".to_owned()]
        );
        assert_eq!(cx.read(view, |view| view.submitted).unwrap(), 1);
        assert!(cx.read(view, |view| view.code.is_valid()).unwrap());

        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("otp accessibility node")
        };
        assert_eq!(node(field.root_id()).role(), accesskit::Role::Group);
        let slot = node(field.input_id(1));
        assert_eq!(slot.role(), accesskit::Role::TextInput);
        assert_eq!(slot.position_in_set(), Some(1));
        assert_eq!(slot.size_of_set(), Some(4));

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
