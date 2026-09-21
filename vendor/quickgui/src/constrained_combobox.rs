use std::{fmt, ops::Range, sync::Arc};

use crate::{
    AccessibilityLive, AccessibilityRole, AutocompleteListState, AutocompleteOptionState,
    AutocompletePopoverLayout, AutocompleteSelectionBehavior, AutocompleteState, Element,
    ElementId, EventContext, PickerError, PickerFilter, PickerFilterMode, PickerItem,
    StateAccessor, ViewContext, WindowHandle, autocomplete::AutocompleteAccess,
};

/// Maximum option rows mounted by one constrained combobox popover.
pub const MAX_COMBOBOX_VISIBLE_ROWS: usize = crate::MAX_AUTOCOMPLETE_VISIBLE_ROWS;
/// Maximum values one multiple combobox retains as chips.
pub const MAX_COMBOBOX_VALUES: usize = 64;

const COMBOBOX_LABEL_ID_TAG: u64 = 0x2a67_bd91_04ec_53f8;
const COMBOBOX_VALUE_ID_TAG: u64 = 0xb385_1c0f_7de2_a946;
const COMBOBOX_ICON_ID_TAG: u64 = 0x7c04_e5a3_182b_df60;
const COMBOBOX_INPUT_GROUP_ID_TAG: u64 = 0x419d_60b8_2f7a_c3e5;
const COMBOBOX_CLEAR_ID_TAG: u64 = 0xe8f2_49d7_5c30_ab16;
const COMBOBOX_TRIGGER_ID_TAG: u64 = 0x53b7_a2ce_9016_4f8d;
const COMBOBOX_CHIPS_ID_TAG: u64 = 0x9d18_f473_6ba5_20ce;
const COMBOBOX_CHIP_ID_TAG: u64 = 0x0c6a_35e9_d871_4b2f;
const COMBOBOX_CHIP_REMOVE_ID_TAG: u64 = 0xa74e_1859_c2f0_36bd;
const COMBOBOX_BACKDROP_ID_TAG: u64 = 0x6f30_c98a_41d5_7e2b;
const COMBOBOX_ARROW_ID_TAG: u64 = 0x1b52_de06_98c4_a37f;
const COMBOBOX_STATUS_ID_TAG: u64 = 0xf209_7ac3_5eb1_460d;
const COMBOBOX_EMPTY_ID_TAG: u64 = 0x38c6_b105_e792_df4a;
const COMBOBOX_LIST_ID_TAG: u64 = 0x4e7b_20fd_a163_895c;
const COMBOBOX_COLLECTION_ID_TAG: u64 = 0xcb41_86e2_073f_d519;

/// Structural geometry for the constrained combobox's separate native suggestion surface.
///
/// This is the same geometry contract used by free-form autocomplete. It contains no color,
/// typography, border, radius, shadow, icon, or animation tokens.
pub type ComboboxPopoverLayout = AutocompletePopoverLayout;

/// State supplied to the caller-owned suggestion-surface renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComboboxListState {
    pub result_count: usize,
    pub total_match_count: usize,
    pub active_index: Option<usize>,
    pub results_truncated: bool,
}

/// A copyable render-state snapshot for one unstyled combobox.
///
/// These are the same facts Base UI publishes on a combobox input as `data-popup-open`,
/// `data-pressed`, `data-placeholder`, `data-valid`, `data-invalid`, `data-dirty`,
/// `data-touched`, `data-filled`, `data-focused`, `data-readonly`, and `data-required`. Build one
/// with [`ComboboxState::state`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ComboboxPartState {
    /// Whether the suggestion surface is open.
    pub popup_open: bool,
    /// Whether the trigger is currently held down.
    pub pressed: bool,
    /// Whether no value is committed, so the input shows its placeholder.
    pub placeholder: bool,
    /// Whether the control currently satisfies its declared constraints.
    pub valid: bool,
    /// Whether the application marked the control invalid.
    pub invalid: bool,
    /// Whether the value changed at least once since the last [`ComboboxState::reset_dirty`].
    pub dirty: bool,
    /// Whether the control has been focused and left at least once.
    pub touched: bool,
    /// Whether the control holds at least one committed value.
    pub filled: bool,
    /// Whether the input currently owns keyboard focus.
    pub focused: bool,
    /// Whether the control refuses value changes while staying focusable.
    pub read_only: bool,
    /// Whether the control requires a value before submission.
    pub required: bool,
}

/// A copyable render-state snapshot for one unstyled combobox row.
///
/// Base UI publishes this as `data-highlighted`, `data-selected`, and `data-disabled`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ComboboxItemPartState {
    /// Whether the roving highlight is on this row.
    pub highlighted: bool,
    /// Whether this row is the committed value.
    pub selected: bool,
    /// Whether the row refuses activation.
    pub disabled: bool,
}

/// State supplied to one caller-owned constrained option renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct ComboboxOptionState {
    pub result_index: usize,
    pub source_index: usize,
    pub active: bool,
    pub selected: bool,
    pub disabled: bool,
    pub label_ranges: Arc<[Range<usize>]>,
}

impl ComboboxOptionState {
    /// The Base UI-named snapshot for this row.
    pub const fn part_state(&self) -> ComboboxItemPartState {
        ComboboxItemPartState {
            highlighted: self.active,
            selected: self.selected,
            disabled: self.disabled,
        }
    }
}

#[derive(Clone)]
struct CommittedSelection<T> {
    stable_id: Option<ElementId>,
    source_index: Option<usize>,
    label: Arc<str>,
    value: T,
}

#[derive(Clone, Copy)]
struct SelectionMarker {
    stable_id: Option<ElementId>,
    source_index: Option<usize>,
}

impl SelectionMarker {
    fn matches<T>(self, item: &PickerItem<T>, source_index: usize) -> bool {
        self.stable_id
            .map_or(self.source_index == Some(source_index), |id| {
                item.stable_id() == Some(id)
            })
    }
}

/// Controlled, editable, single-value combobox constrained to declared items.
///
/// The application owns the input, popover-root, and option elements. QuickGUI owns bounded
/// matching, keyboard navigation, a never-key overflow-capable native child, exact dismissal,
/// committed-value restoration, pointer selection, owner-tree accessibility proxies, and
/// visible-only mounting. Unlike [`crate::AutocompleteState`], arbitrary text is an editing query,
/// not a committable value. Unlike [`crate::SelectState`], the owner control is an editable input.
/// Closed state owns no native window, task, timer, observer, renderer, or scheduler source.
pub struct ComboboxState<T> {
    autocomplete: AutocompleteState<T>,
    selection: Option<CommittedSelection<T>>,
    chips: Vec<CommittedSelection<T>>,
    query: Arc<str>,
    multiple: bool,
    auto_highlight: bool,
    open_on_input_click: bool,
    highlight_item_on_hover: bool,
    loop_focus: bool,
    read_only: bool,
    required: bool,
    pressed: bool,
    focused: bool,
    dirty: bool,
    touched: bool,
}

impl<T> fmt::Debug for ComboboxState<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ComboboxState")
            .field("autocomplete", &self.autocomplete)
            .field("query", &self.query)
            .field(
                "selected_source",
                &self
                    .selection
                    .as_ref()
                    .and_then(|selection| selection.source_index),
            )
            .field(
                "selected_id",
                &self
                    .selection
                    .as_ref()
                    .and_then(|selection| selection.stable_id),
            )
            .field(
                "selected_label",
                &self.selection.as_ref().map(|selection| &selection.label),
            )
            .field("chips", &self.chips.len())
            .field("multiple", &self.multiple)
            .finish_non_exhaustive()
    }
}

impl<T> ComboboxState<T>
where
    T: Clone,
{
    pub fn new(items: impl IntoIterator<Item = PickerItem<T>>) -> Result<Self, PickerError> {
        let mut autocomplete = AutocompleteState::new(items)?;
        autocomplete.set_selection_behavior(AutocompleteSelectionBehavior::DismissOnly);
        // Base UI's combobox filters by substring; the fuzzy ranker stays one call away.
        let autocomplete = autocomplete.with_filter_mode(PickerFilterMode::Contains);
        Ok(Self {
            autocomplete,
            selection: None,
            chips: Vec::new(),
            query: Arc::from(""),
            multiple: false,
            auto_highlight: false,
            open_on_input_click: true,
            highlight_item_on_hover: true,
            loop_focus: true,
            read_only: false,
            required: false,
            pressed: false,
            focused: false,
            dirty: false,
            touched: false,
        })
    }

    pub fn with_layout(mut self, layout: ComboboxPopoverLayout) -> Self {
        self.autocomplete = self.autocomplete.with_layout(layout);
        self
    }

    pub fn with_filter_mode(mut self, mode: PickerFilterMode) -> Self {
        self.autocomplete = self.autocomplete.with_filter_mode(mode);
        self
    }

    /// Set an initial selection before a runtime context exists.
    pub fn with_selected_source(mut self, source_index: usize) -> Self {
        self.set_initial_selection(source_index);
        self
    }

    /// Set an initial stable-ID selection before a runtime context exists.
    pub fn with_selected_id(mut self, id: impl Into<ElementId>) -> Self {
        let id = id.into();
        if let Some(source_index) = self
            .autocomplete
            .items()
            .iter()
            .position(|item| item.stable_id() == Some(id))
        {
            self.set_initial_selection(source_index);
        }
        self
    }

    pub const fn layout(&self) -> ComboboxPopoverLayout {
        self.autocomplete.layout()
    }

    /// Replace popover geometry and synchronously dismiss an obsolete fixed-size child.
    pub fn set_layout(&mut self, layout: ComboboxPopoverLayout, cx: &mut EventContext) -> bool {
        if !self.autocomplete.set_layout(layout, cx) {
            return false;
        }
        self.restore_committed(cx);
        true
    }

    pub fn items(&self) -> &[PickerItem<T>] {
        self.autocomplete.items()
    }

    /// Atomically replace the bounded suggestion source.
    ///
    /// A committed item with a stable ID is rebound across filtering or reordering. If it is
    /// temporarily absent, its cloned committed value and label remain authoritative while its
    /// current source index becomes `None`.
    pub fn set_items(
        &mut self,
        items: impl IntoIterator<Item = PickerItem<T>>,
        cx: &mut EventContext,
    ) -> Result<(), PickerError> {
        self.autocomplete.set_items(items, cx)?;
        self.rebind_selection();
        if !self.is_open() {
            self.restore_committed(cx);
        }
        Ok(())
    }

    pub const fn filter_mode(&self) -> PickerFilterMode {
        self.autocomplete.filter_mode()
    }

    pub fn set_filter_mode(&mut self, mode: PickerFilterMode, cx: &mut EventContext) -> bool {
        self.autocomplete.set_filter_mode(mode, cx)
    }

    pub fn query(&self) -> &Arc<str> {
        &self.query
    }

    /// The visible input value: an edit query while open, otherwise the committed label.
    pub fn input_value(&self) -> &Arc<str> {
        self.autocomplete.value()
    }

    pub fn selected_source_index(&self) -> Option<usize> {
        self.selection
            .as_ref()
            .and_then(|selection| selection.source_index)
    }

    pub fn selected_item(&self) -> Option<&PickerItem<T>> {
        let selection = self.selection.as_ref()?;
        let source_index = selection.source_index?;
        let item = self.autocomplete.items().get(source_index)?;
        selection.stable_id.map_or_else(
            || Some(item),
            |id| (item.stable_id() == Some(id)).then_some(item),
        )
    }

    pub fn selected_value(&self) -> Option<&T> {
        self.selection.as_ref().map(|selection| &selection.value)
    }

    pub fn selected_label(&self) -> Option<&Arc<str>> {
        self.selection.as_ref().map(|selection| &selection.label)
    }

    /// Replace the committed selection and close any open suggestion child.
    pub fn set_selected_source(&mut self, source_index: usize, cx: &mut EventContext) -> bool {
        let Some(selection) = self.selection_from_source(source_index) else {
            return false;
        };
        if self.same_selection(&selection) {
            return false;
        }
        self.autocomplete.close(cx);
        self.selection = Some(selection);
        self.restore_committed(cx);
        true
    }

    pub fn set_selected_id(&mut self, id: impl Into<ElementId>, cx: &mut EventContext) -> bool {
        let id = id.into();
        self.autocomplete
            .items()
            .iter()
            .position(|item| item.stable_id() == Some(id))
            .is_some_and(|source_index| self.set_selected_source(source_index, cx))
    }

    pub fn clear_selection(&mut self, cx: &mut EventContext) -> bool {
        if self.selection.take().is_none() {
            return false;
        }
        self.autocomplete.close(cx);
        self.restore_committed(cx);
        true
    }

    /// Accept more than one committed value, Base UI's `multiple`.
    ///
    /// A multiple combobox keeps its committed values as bounded chips instead of writing one
    /// label back into the input, so the input stays an editing query after every commit.
    pub fn multiple(mut self, multiple: bool) -> Self {
        self.set_multiple(multiple);
        self
    }

    /// Replace the multiple flag in place, reporting whether anything changed.
    pub fn set_multiple(&mut self, multiple: bool) -> bool {
        if self.multiple == multiple {
            return false;
        }
        self.multiple = multiple;
        if !multiple {
            self.chips.truncate(1);
        }
        true
    }

    pub const fn is_multiple(&self) -> bool {
        self.multiple
    }

    /// Highlight the first result as soon as one exists, Base UI's `autoHighlight`.
    pub const fn auto_highlight(mut self, auto_highlight: bool) -> Self {
        self.auto_highlight = auto_highlight;
        self
    }

    pub const fn auto_highlights(&self) -> bool {
        self.auto_highlight
    }

    /// Open the suggestion surface when the input itself is clicked, Base UI's `openOnInputClick`.
    pub const fn open_on_input_click(mut self, open_on_input_click: bool) -> Self {
        self.open_on_input_click = open_on_input_click;
        self
    }

    pub const fn opens_on_input_click(&self) -> bool {
        self.open_on_input_click
    }

    /// Move the highlight with the pointer, Base UI's `highlightItemOnHover`.
    pub const fn highlight_item_on_hover(mut self, highlight: bool) -> Self {
        self.highlight_item_on_hover = highlight;
        self
    }

    pub const fn highlights_item_on_hover(&self) -> bool {
        self.highlight_item_on_hover
    }

    /// Wrap the highlight at the ends of the result set, Base UI's `loop`.
    pub const fn loop_focus(mut self, loop_focus: bool) -> Self {
        self.loop_focus = loop_focus;
        self
    }

    pub const fn loops_focus(&self) -> bool {
        self.loop_focus
    }

    /// Refuse value changes while staying focusable, Base UI's `readOnly`.
    pub const fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Require a value before submission, Base UI's `required`.
    pub const fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub const fn is_required(&self) -> bool {
        self.required
    }

    /// Replace the built-in filter policy with an application-supplied predicate, Base UI's
    /// `filter`.
    ///
    /// Passing `None` restores [`Self::filter_mode`], whose combobox default is
    /// [`PickerFilterMode::Contains`].
    pub fn set_filter(&mut self, filter: Option<PickerFilter>, cx: &mut EventContext) -> bool {
        self.autocomplete.set_filter(filter, cx)
    }

    /// Declare an application-supplied filter predicate at construction time.
    pub fn with_filter(mut self, filter: PickerFilter) -> Self {
        self.autocomplete = self.autocomplete.with_filter(filter);
        self
    }

    /// The custom filter predicate, when one is installed.
    pub fn filter(&self) -> Option<&PickerFilter> {
        self.autocomplete.filter()
    }

    /// Every committed chip label, in commit order, Base UI's `Combobox.Chips` contents.
    pub fn chip_labels(&self) -> impl Iterator<Item = &Arc<str>> {
        self.chips.iter().map(|chip| &chip.label)
    }

    /// Every committed chip value, in commit order.
    pub fn chip_values(&self) -> impl Iterator<Item = &T> {
        self.chips.iter().map(|chip| &chip.value)
    }

    pub fn chip_count(&self) -> usize {
        self.chips.len()
    }

    /// Commit one more source row as a chip, up to [`MAX_COMBOBOX_VALUES`].
    ///
    /// Returns `false` for a disabled row, an already-committed row, a read-only combobox, or a
    /// chip set that is already full.
    pub fn add_chip_source(&mut self, source_index: usize) -> bool {
        if self.read_only || self.chips.len() >= MAX_COMBOBOX_VALUES {
            return false;
        }
        let Some(chip) = self.selection_from_source(source_index) else {
            return false;
        };
        if self
            .chips
            .iter()
            .any(|existing| existing.source_index == chip.source_index)
        {
            return false;
        }
        self.chips.push(chip);
        self.dirty = true;
        self.touched = true;
        true
    }

    /// Remove one chip, Base UI's `Combobox.ChipRemove`.
    pub fn remove_chip(&mut self, chip_index: usize) -> bool {
        if self.read_only || chip_index >= self.chips.len() {
            return false;
        }
        self.chips.remove(chip_index);
        self.dirty = true;
        self.touched = true;
        true
    }

    /// Drop every chip.
    pub fn clear_chips(&mut self) -> bool {
        if self.read_only || self.chips.is_empty() {
            return false;
        }
        self.chips.clear();
        self.dirty = true;
        true
    }

    /// Record that the trigger is held down, Base UI's `data-pressed`.
    pub const fn set_pressed(&mut self, pressed: bool) -> bool {
        if self.pressed == pressed {
            return false;
        }
        self.pressed = pressed;
        true
    }

    /// Record input focus, and mark the control touched when focus leaves it.
    pub const fn set_focused(&mut self, focused: bool) -> bool {
        if self.focused == focused {
            return false;
        }
        self.focused = focused;
        if !focused {
            self.touched = true;
        }
        true
    }

    /// Forget that the control was interacted with, for a form that has just been reset.
    ///
    /// This is the counterpart of [`Self::reset_dirty`]: a value applied from a declaration is not
    /// an interaction, so a host that seeds the control reports Base UI's `data-touched` only for
    /// what the user really did.
    pub const fn reset_touched(&mut self) -> bool {
        if !self.touched {
            return false;
        }
        self.touched = false;
        true
    }

    /// Forget that the value changed, for a form that has just been submitted or reset.
    pub const fn reset_dirty(&mut self) -> bool {
        if !self.dirty {
            return false;
        }
        self.dirty = false;
        true
    }

    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub const fn is_touched(&self) -> bool {
        self.touched
    }

    pub const fn is_focused(&self) -> bool {
        self.focused
    }

    /// A copyable render-state snapshot the application styles from.
    pub fn state(&self) -> ComboboxPartState {
        let filled = if self.multiple {
            !self.chips.is_empty()
        } else {
            self.selection.is_some()
        };
        ComboboxPartState {
            popup_open: self.is_open(),
            pressed: self.pressed,
            placeholder: !filled,
            valid: !self.is_invalid(),
            invalid: self.is_invalid(),
            dirty: self.dirty,
            touched: self.touched,
            filled,
            focused: self.focused,
            read_only: self.read_only,
            required: self.required,
        }
    }

    /// The live-region text a mounted `Status` part announces, Base UI's `Combobox.Status`.
    ///
    /// The wording is intentionally minimal and English-free of punctuation so an application can
    /// replace it; QuickGUI owns only the counting and the exactly-once announcement.
    pub fn status_text(&self) -> Arc<str> {
        match self.result_count() {
            0 => Arc::from("No results"),
            1 => Arc::from("1 result"),
            count => Arc::from(format!("{count} results")),
        }
    }

    /// Whether the `Empty` part should be mounted, Base UI's `Combobox.Empty`.
    pub fn is_empty_result(&self) -> bool {
        self.result_count() == 0
    }

    pub const fn popover_window(&self) -> Option<WindowHandle> {
        self.autocomplete.popover_window()
    }

    pub const fn is_open(&self) -> bool {
        self.autocomplete.is_open()
    }

    pub const fn is_disabled(&self) -> bool {
        self.autocomplete.is_disabled()
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut EventContext) -> bool {
        if !self.autocomplete.set_disabled(disabled, cx) {
            return false;
        }
        if disabled {
            self.restore_committed(cx);
        }
        true
    }

    pub const fn is_invalid(&self) -> bool {
        self.autocomplete.is_invalid()
    }

    pub fn set_invalid(&mut self, invalid: bool) -> bool {
        self.autocomplete.set_invalid(invalid)
    }

    pub fn validation_message(&self) -> Option<&Arc<str>> {
        self.autocomplete.validation_message()
    }

    pub fn set_validation_message(&mut self, message: impl Into<Arc<str>>) -> bool {
        self.autocomplete.set_validation_message(message)
    }

    pub fn clear_validation_message(&mut self) -> bool {
        self.autocomplete.clear_validation_message()
    }

    pub fn result_count(&self) -> usize {
        self.autocomplete.result_count()
    }

    pub fn total_match_count(&self) -> usize {
        self.autocomplete.total_match_count()
    }

    pub fn active_result_index(&self) -> Option<usize> {
        self.autocomplete.active_result_index()
    }

    pub fn active_source_index(&self) -> Option<usize> {
        self.autocomplete.active_source_index()
    }

    pub fn close(&mut self, cx: &mut EventContext) -> bool {
        let closed = self.autocomplete.close(cx);
        let (restored, _) = self.restore_committed(cx);
        closed || restored
    }

    pub fn surface_id(id: impl Into<ElementId>) -> ElementId {
        AutocompleteState::<T>::surface_id(id)
    }

    /// Stable identity of the mounted label part.
    pub fn label_id(id: impl Into<ElementId>) -> ElementId {
        derived_combobox_id(id.into(), COMBOBOX_LABEL_ID_TAG, 0)
    }

    /// Stable identity of the mounted value part.
    pub fn value_id(id: impl Into<ElementId>) -> ElementId {
        derived_combobox_id(id.into(), COMBOBOX_VALUE_ID_TAG, 0)
    }

    /// Stable identity of the mounted icon part.
    pub fn icon_id(id: impl Into<ElementId>) -> ElementId {
        derived_combobox_id(id.into(), COMBOBOX_ICON_ID_TAG, 0)
    }

    /// Stable identity of the input group wrapping the input and its affordances.
    pub fn input_group_id(id: impl Into<ElementId>) -> ElementId {
        derived_combobox_id(id.into(), COMBOBOX_INPUT_GROUP_ID_TAG, 0)
    }

    /// Stable identity of the clear control.
    pub fn clear_id(id: impl Into<ElementId>) -> ElementId {
        derived_combobox_id(id.into(), COMBOBOX_CLEAR_ID_TAG, 0)
    }

    /// Stable identity of the surface trigger.
    pub fn trigger_id(id: impl Into<ElementId>) -> ElementId {
        derived_combobox_id(id.into(), COMBOBOX_TRIGGER_ID_TAG, 0)
    }

    /// Stable identity of the chip container.
    pub fn chips_id(id: impl Into<ElementId>) -> ElementId {
        derived_combobox_id(id.into(), COMBOBOX_CHIPS_ID_TAG, 0)
    }

    /// Stable identity of one chip.
    pub fn chip_id(id: impl Into<ElementId>, chip_index: usize) -> ElementId {
        derived_combobox_id(id.into(), COMBOBOX_CHIP_ID_TAG, chip_index as u64 + 1)
    }

    /// Stable identity of one chip's remove control.
    pub fn chip_remove_id(id: impl Into<ElementId>, chip_index: usize) -> ElementId {
        derived_combobox_id(
            id.into(),
            COMBOBOX_CHIP_REMOVE_ID_TAG,
            chip_index as u64 + 1,
        )
    }

    /// Stable identity of the optional owner-window backdrop.
    pub fn backdrop_id(id: impl Into<ElementId>) -> ElementId {
        derived_combobox_id(id.into(), COMBOBOX_BACKDROP_ID_TAG, 0)
    }

    /// Stable identity of the decorative popup arrow.
    pub fn arrow_id(id: impl Into<ElementId>) -> ElementId {
        derived_combobox_id(id.into(), COMBOBOX_ARROW_ID_TAG, 0)
    }

    /// Stable identity of the result-count live region.
    pub fn status_id(id: impl Into<ElementId>) -> ElementId {
        derived_combobox_id(id.into(), COMBOBOX_STATUS_ID_TAG, 0)
    }

    /// Stable identity of the no-results part.
    pub fn empty_id(id: impl Into<ElementId>) -> ElementId {
        derived_combobox_id(id.into(), COMBOBOX_EMPTY_ID_TAG, 0)
    }

    /// Stable identity of the scrolling result list inside the popup.
    pub fn list_id(id: impl Into<ElementId>) -> ElementId {
        derived_combobox_id(id.into(), COMBOBOX_LIST_ID_TAG, 0)
    }

    /// Stable identity of the collection wrapper inside the list.
    pub fn collection_id(id: impl Into<ElementId>) -> ElementId {
        derived_combobox_id(id.into(), COMBOBOX_COLLECTION_ID_TAG, 0)
    }

    /// Decorate the optional application-owned structural wrapper, Base UI's `Combobox.Root`.
    pub fn root_with(root: Element) -> Element {
        root.app_region_no_drag()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root() -> Element {
        Self::root_with(crate::div())
    }

    /// Decorate the caller-owned visible label, Base UI's `Combobox.Label`.
    pub fn label_with(id: impl Into<ElementId>, label: Element) -> Element {
        label
            .id(Self::label_id(id))
            .accessibility_role(AccessibilityRole::Label)
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled label part. Use [`Self::label_with`] to supply an existing element.
    pub fn label(id: impl Into<ElementId>) -> Element {
        Self::label_with(id, crate::div())
    }

    /// Decorate the caller-owned committed-value text, Base UI's `Combobox.Value`.
    ///
    /// The input already exposes the value, so this part is decoration.
    pub fn value_with(id: impl Into<ElementId>, value: Element) -> Element {
        value
            .id(Self::value_id(id))
            .accessibility_hidden(true)
            .app_region_no_drag()
    }
    /// Create the unstyled value part. Use [`Self::value_with`] to supply an existing element.
    pub fn value(id: impl Into<ElementId>) -> Element {
        Self::value_with(id, crate::div())
    }

    /// Decorate the caller-owned affordance glyph, Base UI's `Combobox.Icon`.
    pub fn icon_with(id: impl Into<ElementId>, icon: Element) -> Element {
        icon.id(Self::icon_id(id))
            .accessibility_hidden(true)
            .app_region_no_drag()
    }
    /// Create the unstyled icon part. Use [`Self::icon_with`] to supply an existing element.
    pub fn icon(id: impl Into<ElementId>) -> Element {
        Self::icon_with(id, crate::div())
    }

    /// Decorate the wrapper holding the input, chips, and affordances, Base UI's
    /// `Combobox.InputGroup`.
    pub fn input_group_with(id: impl Into<ElementId>, group: Element) -> Element {
        let id = id.into();
        group
            .id(Self::input_group_id(id))
            .accessibility_role(AccessibilityRole::Group)
            .accessibility_labelled_by(Self::label_id(id))
            .app_region_no_drag()
    }
    /// Create the unstyled input group part. Use [`Self::input_group_with`] to supply an existing element.
    pub fn input_group(id: impl Into<ElementId>) -> Element {
        Self::input_group_with(id, crate::div())
    }

    /// Decorate the caller-owned editable input, Base UI's `Combobox.Input`.
    ///
    /// The full interaction — filtering, navigation, the suggestion surface, and committing — is
    /// attached by [`Self::element`]; this part adds the form-state projection Base UI publishes
    /// alongside it, so a composition that already owns the interaction still reports the same
    /// required, read-only, and label relationships.
    pub fn input_with(&self, id: impl Into<ElementId>, input: Element) -> Element {
        let id = id.into();
        input
            .accessibility_labelled_by(Self::label_id(id))
            .accessibility_read_only(self.read_only)
            .required(self.required)
            .invalid(self.is_invalid())
            .app_region_no_drag()
    }
    /// Create the unstyled input part. Use [`Self::input_with`] to supply an existing element.
    pub fn input(&self, id: impl Into<ElementId>) -> Element {
        self.input_with(id, crate::text_input(""))
    }

    /// Decorate the caller-owned chip container, Base UI's `Combobox.Chips`.
    pub fn chips_with(id: impl Into<ElementId>, chips: Element) -> Element {
        let id = id.into();
        chips
            .id(Self::chips_id(id))
            .accessibility_role(AccessibilityRole::List)
            .accessibility_labelled_by(Self::label_id(id))
            .app_region_no_drag()
    }
    /// Create the unstyled chips part. Use [`Self::chips_with`] to supply an existing element.
    pub fn chips(id: impl Into<ElementId>) -> Element {
        Self::chips_with(id, crate::div())
    }

    /// Decorate one caller-owned chip, Base UI's `Combobox.Chip`.
    pub fn chip_with(
        id: impl Into<ElementId>,
        chip_index: usize,
        label: impl Into<Arc<str>>,
        chip: Element,
    ) -> Element {
        let id = id.into();
        chip.id(Self::chip_id(id, chip_index))
            .accessibility_role(AccessibilityRole::ListItem)
            .accessibility_label(label)
            .accessibility_position_in_set(chip_index)
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled chip part. Use [`Self::chip_with`] to supply an existing element.
    pub fn chip(
        id: impl Into<ElementId>,
        chip_index: usize,
        label: impl Into<Arc<str>>,
    ) -> Element {
        Self::chip_with(id, chip_index, label, crate::div())
    }

    /// Decorate one chip's remove control, Base UI's `Combobox.ChipRemove`.
    ///
    /// The control is a real button with an accessible name, so a keyboard user can reach and
    /// remove a chip without the pointer.
    pub fn chip_remove_with(
        id: impl Into<ElementId>,
        chip_index: usize,
        label: impl Into<Arc<str>>,
        remove: Element,
    ) -> Element {
        let id = id.into();
        remove
            .id(Self::chip_remove_id(id, chip_index))
            .clickable()
            .focusable()
            .accessibility_role(AccessibilityRole::Button)
            .accessibility_label(label)
            .app_region_no_drag()
            .user_select_none()
            .cursor_default()
    }
    /// Create the unstyled chip remove part. Use [`Self::chip_remove_with`] to supply an existing element.
    pub fn chip_remove(
        id: impl Into<ElementId>,
        chip_index: usize,
        label: impl Into<Arc<str>>,
    ) -> Element {
        Self::chip_remove_with(id, chip_index, label, crate::button())
    }

    /// Decorate the caller-owned clear control, Base UI's `Combobox.Clear`.
    ///
    /// Mount it only while the control holds a value; Base UI hides it otherwise.
    pub fn clear_with(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        clear: Element,
    ) -> Element {
        clear
            .id(Self::clear_id(id))
            .clickable()
            .focusable()
            .accessibility_role(AccessibilityRole::Button)
            .accessibility_label(label)
            .app_region_no_drag()
            .user_select_none()
            .cursor_default()
    }
    /// Create the unstyled clear part. Use [`Self::clear_with`] to supply an existing element.
    pub fn clear(id: impl Into<ElementId>, label: impl Into<Arc<str>>) -> Element {
        Self::clear_with(id, label, crate::button())
    }

    /// Decorate the caller-owned surface trigger, Base UI's `Combobox.Trigger`.
    pub fn trigger_with(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        trigger: Element,
    ) -> Element {
        let id = id.into();
        trigger
            .id(Self::trigger_id(id))
            .clickable()
            .accessibility_role(AccessibilityRole::Button)
            .accessibility_label(label)
            .accessibility_has_popover(crate::AccessibilityPopover::ListBox)
            .accessibility_expanded(self.is_open())
            .accessibility_controls(Self::surface_id(id))
            .disabled(self.is_disabled())
            .app_region_no_drag()
            .user_select_none()
            .cursor_default()
    }
    /// Create the unstyled trigger part. Use [`Self::trigger_with`] to supply an existing element.
    pub fn trigger(&self, id: impl Into<ElementId>, label: impl Into<Arc<str>>) -> Element {
        self.trigger_with(id, label, crate::button())
    }

    /// Decorate an optional caller-painted owner-window backdrop, Base UI's `Combobox.Backdrop`.
    pub fn backdrop_with(id: impl Into<ElementId>, backdrop: Element) -> Element {
        backdrop
            .id(Self::backdrop_id(id))
            .overlay()
            .inset_0()
            .size_full()
            .app_region_no_drag()
            .cursor_default()
            .accessibility_hidden(true)
    }
    /// Create the unstyled backdrop part. Use [`Self::backdrop_with`] to supply an existing element.
    pub fn backdrop(id: impl Into<ElementId>) -> Element {
        Self::backdrop_with(id, crate::div())
    }

    /// Decorate the suggestion-surface boundary, Base UI's `Combobox.Portal`.
    ///
    /// The suggestion surface is its own native window, so the portal, the positioner, and the
    /// popup are one element and all three names decorate it identically.
    pub fn portal_with(&self, id: impl Into<ElementId>, portal: Element) -> Element {
        self.popup_with(id, portal)
    }
    /// Create the unstyled portal part. Use [`Self::portal_with`] to supply an existing element.
    pub fn portal(&self, id: impl Into<ElementId>) -> Element {
        self.portal_with(id, crate::div())
    }

    /// Decorate the suggestion-surface boundary, Base UI's `Combobox.Positioner`.
    pub fn positioner_with(&self, id: impl Into<ElementId>, positioner: Element) -> Element {
        self.popup_with(id, positioner)
    }
    /// Create the unstyled positioner part. Use [`Self::positioner_with`] to supply an existing element.
    pub fn positioner(&self, id: impl Into<ElementId>) -> Element {
        self.positioner_with(id, crate::div())
    }

    /// Decorate the caller-owned suggestion surface, Base UI's `Combobox.Popup`.
    pub fn popup_with(&self, id: impl Into<ElementId>, popup: Element) -> Element {
        popup
            .id(Self::surface_id(id))
            .accessibility_role(AccessibilityRole::ListBox)
            .accessibility_size_of_set(self.result_count())
            .accessibility_multiselectable(self.multiple)
            .app_region_no_drag()
            .cursor_default()
    }
    /// Create the unstyled popup part. Use [`Self::popup_with`] to supply an existing element.
    pub fn popup(&self, id: impl Into<ElementId>) -> Element {
        self.popup_with(id, crate::div())
    }

    /// Position a caller-owned decorative arrow, Base UI's `Combobox.Arrow`.
    pub fn arrow_with(id: impl Into<ElementId>, arrow: Element) -> Element {
        arrow
            .id(Self::arrow_id(id))
            .absolute()
            .accessibility_hidden(true)
            .app_region_no_drag()
    }
    /// Create the unstyled arrow part. Use [`Self::arrow_with`] to supply an existing element.
    pub fn arrow(id: impl Into<ElementId>) -> Element {
        Self::arrow_with(id, crate::div())
    }

    /// Decorate the caller-owned live region, Base UI's `Combobox.Status`.
    ///
    /// Render [`Self::status_text`] inside it. The region is polite and is rebuilt only when the
    /// application rebuilds the tree, so an unchanged count announces exactly once.
    pub fn status_with(id: impl Into<ElementId>, status: Element) -> Element {
        status
            .id(Self::status_id(id))
            .accessibility_role(AccessibilityRole::Status)
            .accessibility_live(AccessibilityLive::Polite)
            .app_region_no_drag()
    }
    /// Create the unstyled status part. Use [`Self::status_with`] to supply an existing element.
    pub fn status(id: impl Into<ElementId>) -> Element {
        Self::status_with(id, crate::div())
    }

    /// Decorate the caller-owned no-results part, Base UI's `Combobox.Empty`.
    ///
    /// Mount it only while [`Self::is_empty_result`] is true. The `Status` region already
    /// announces the count, so this part is visual.
    pub fn empty_with(id: impl Into<ElementId>, empty: Element) -> Element {
        empty
            .id(Self::empty_id(id))
            .accessibility_hidden(true)
            .app_region_no_drag()
    }
    /// Create the unstyled empty part. Use [`Self::empty_with`] to supply an existing element.
    pub fn empty(id: impl Into<ElementId>) -> Element {
        Self::empty_with(id, crate::div())
    }

    /// Decorate the caller-owned scrolling result list, Base UI's `Combobox.List`.
    pub fn list_with(&self, id: impl Into<ElementId>, list: Element) -> Element {
        list.id(Self::list_id(id))
            .accessibility_hidden(self.result_count() == 0)
            .app_region_no_drag()
    }
    /// Create the unstyled list part. Use [`Self::list_with`] to supply an existing element.
    pub fn list(&self, id: impl Into<ElementId>) -> Element {
        self.list_with(id, crate::div())
    }

    /// Decorate a caller-owned wrapper around the mounted rows, Base UI's `Combobox.Collection`.
    pub fn collection_with(id: impl Into<ElementId>, collection: Element) -> Element {
        collection.id(Self::collection_id(id)).app_region_no_drag()
    }
    /// Create the unstyled collection part. Use [`Self::collection_with`] to supply an existing element.
    pub fn collection(id: impl Into<ElementId>) -> Element {
        Self::collection_with(id, crate::div())
    }

    /// Decorate a caller-owned row wrapper for a grid-shaped result, Base UI's `Combobox.Row`.
    pub fn row_with(row: Element) -> Element {
        row.accessibility_role(AccessibilityRole::Group)
            .app_region_no_drag()
    }
    /// Create the unstyled row part. Use [`Self::row_with`] to supply an existing element.
    pub fn row() -> Element {
        Self::row_with(crate::div())
    }

    /// Decorate one caller-owned result row, Base UI's `Combobox.Item`.
    pub fn item_with(
        &self,
        id: impl Into<ElementId>,
        source_index: usize,
        state: ComboboxItemPartState,
        item: Element,
    ) -> Option<Element> {
        let row_id = self.option_id_for_source(id, source_index)?;
        Some(
            item.id(row_id)
                .clickable()
                .tab_index(-1)
                .disabled(state.disabled)
                .selected(state.selected)
                .accessibility_role(AccessibilityRole::ListBoxOption)
                .accessibility_position_in_set(source_index)
                .app_region_no_drag()
                .user_select_none()
                .cursor_default(),
        )
    }
    /// Create the unstyled item part. Use [`Self::item_with`] to supply an existing element.
    pub fn item(
        &self,
        id: impl Into<ElementId>,
        source_index: usize,
        state: ComboboxItemPartState,
    ) -> Option<Element> {
        self.item_with(id, source_index, state, crate::div())
    }

    /// Decorate one row's selected mark, Base UI's `Combobox.ItemIndicator`.
    pub fn item_indicator_with(indicator: Element) -> Element {
        indicator.accessibility_hidden(true).app_region_no_drag()
    }
    /// Create the unstyled item indicator part. Use [`Self::item_indicator_with`] to supply an existing element.
    pub fn item_indicator() -> Element {
        Self::item_indicator_with(crate::div())
    }

    /// Decorate a caller-composed result group, Base UI's `Combobox.Group`.
    pub fn group_with(group: Element) -> Element {
        group
            .accessibility_role(AccessibilityRole::Group)
            .app_region_no_drag()
    }
    /// Create the unstyled group part. Use [`Self::group_with`] to supply an existing element.
    pub fn group() -> Element {
        Self::group_with(crate::div())
    }

    /// Decorate a group's visible label, Base UI's `Combobox.GroupLabel`.
    pub fn group_label_with(label_id: impl Into<ElementId>, label: Element) -> Element {
        label
            .id(label_id)
            .accessibility_role(AccessibilityRole::Label)
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled group label part. Use [`Self::group_label_with`] to supply an existing element.
    pub fn group_label(label_id: impl Into<ElementId>) -> Element {
        Self::group_label_with(label_id, crate::div())
    }

    /// Decorate a caller-owned divider between groups, Base UI's `Combobox.Separator`.
    pub fn separator_with(separator: Element) -> Element {
        separator
            .accessibility_role(AccessibilityRole::Separator)
            .app_region_no_drag()
    }
    /// Create the unstyled separator part. Use [`Self::separator_with`] to supply an existing element.
    pub fn separator() -> Element {
        Self::separator_with(crate::div())
    }

    pub fn option_id_for_source(
        &self,
        id: impl Into<ElementId>,
        source_index: usize,
    ) -> Option<ElementId> {
        self.autocomplete.option_id_for_source(id, source_index)
    }

    /// Build the complete unstyled constrained interaction from caller-owned parts.
    #[allow(clippy::too_many_arguments)]
    pub fn element<V, PopoverRoot, RenderOption, QueryChanged, Change>(
        &mut self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        access: fn(&mut V) -> &mut ComboboxState<T>,
        input: Element,
        popover_root: PopoverRoot,
        render_option: RenderOption,
        query_changed: QueryChanged,
        change: Change,
    ) -> Element
    where
        V: 'static,
        T: 'static,
        PopoverRoot: Fn(ComboboxListState) -> Element + Clone + 'static,
        RenderOption: Fn(&PickerItem<T>, ComboboxOptionState) -> Element + Clone + 'static,
        QueryChanged: Fn(&mut V, Arc<str>, &mut EventContext) + Clone + 'static,
        Change: Fn(&mut V, T, &mut EventContext) + Clone + 'static,
    {
        self.element_with(
            cx,
            id,
            label,
            StateAccessor::from(access),
            input,
            popover_root,
            render_option,
            query_changed,
            change,
        )
    }

    /// Build the constrained combobox against a per-instance retained-state accessor.
    ///
    /// A host that renders many declared comboboxes through one view passes an accessor that
    /// captures which [`ComboboxState`] each registered listener resolves.
    #[allow(clippy::too_many_arguments)]
    pub fn element_with<V, PopoverRoot, RenderOption, QueryChanged, Change>(
        &mut self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        access: StateAccessor<V, ComboboxState<T>>,
        input: Element,
        popover_root: PopoverRoot,
        render_option: RenderOption,
        query_changed: QueryChanged,
        change: Change,
    ) -> Element
    where
        V: 'static,
        T: 'static,
        PopoverRoot: Fn(ComboboxListState) -> Element + Clone + 'static,
        RenderOption: Fn(&PickerItem<T>, ComboboxOptionState) -> Element + Clone + 'static,
        QueryChanged: Fn(&mut V, Arc<str>, &mut EventContext) + Clone + 'static,
        Change: Fn(&mut V, T, &mut EventContext) + Clone + 'static,
    {
        let id = id.into();
        let selected_source = self.selected_source_index();
        let selection_marker = self.selection.as_ref().map(|selection| SelectionMarker {
            stable_id: selection.stable_id,
            source_index: selection.source_index,
        });

        let list_root = move |state: AutocompleteListState| {
            popover_root(ComboboxListState {
                result_count: state.result_count,
                total_match_count: state.total_match_count,
                active_index: state.active_index,
                results_truncated: state.results_truncated,
            })
        };
        let option = move |item: &PickerItem<T>, state: AutocompleteOptionState| {
            render_option(
                item,
                ComboboxOptionState {
                    result_index: state.result_index,
                    source_index: state.source_index,
                    active: state.active,
                    selected: selection_marker
                        .is_some_and(|marker| marker.matches(item, state.source_index)),
                    disabled: state.disabled,
                    label_ranges: state.label_ranges,
                },
            )
        };
        let inner_access = AutocompleteAccess::new(access.clone(), combobox_autocomplete::<T>);

        let edit_query_changed = query_changed.clone();
        let edit_access = access.clone();
        let edited = move |view: &mut V, _value: Arc<str>, cx: &mut EventContext| {
            let query = {
                let state = edit_access.get(view);
                let query = state.autocomplete.picker_query().clone();
                if state.query == query {
                    return;
                }
                state.query = query.clone();
                query
            };
            edit_query_changed(view, query, cx);
        };

        let commit_query_changed = query_changed.clone();
        let commit_access = access.clone();
        let committed =
            move |view: &mut V, source_index: usize, value: T, cx: &mut EventContext| {
                let query_changed = {
                    let state = commit_access.get(view);
                    let Some(item) = state.autocomplete.items().get(source_index) else {
                        return;
                    };
                    state.selection = Some(CommittedSelection {
                        stable_id: item.stable_id(),
                        source_index: Some(source_index),
                        label: item.label().clone(),
                        value: value.clone(),
                    });
                    state.dirty = true;
                    state.touched = true;
                    if state.multiple {
                        state.add_chip_source(source_index);
                        // A multiple combobox keeps typing where it was, so the input becomes an
                        // empty query rather than the committed label.
                        state.selection = None;
                    }
                    state.restore_committed(cx).1
                };
                if query_changed {
                    commit_query_changed(view, Arc::from(""), cx);
                }
                change(view, value, cx);
            };

        let dismiss_query_changed = query_changed;
        let dismissed = move |view: &mut V, cx: &mut EventContext| {
            let query_changed = access.get(view).restore_committed(cx).1;
            if query_changed {
                dismiss_query_changed(view, Arc::from(""), cx);
            }
        };

        self.autocomplete.element_with_source(
            cx,
            id,
            label,
            inner_access,
            input,
            list_root,
            option,
            edited,
            committed,
            dismissed,
            selected_source,
        )
    }

    fn set_initial_selection(&mut self, source_index: usize) -> bool {
        let Some(selection) = self.selection_from_source(source_index) else {
            return false;
        };
        if self.same_selection(&selection) {
            return false;
        }
        self.autocomplete.set_display_value(selection.label.clone());
        self.selection = Some(selection);
        true
    }

    fn selection_from_source(&self, source_index: usize) -> Option<CommittedSelection<T>> {
        let item = self
            .autocomplete
            .items()
            .get(source_index)
            .filter(|item| !item.is_disabled())?;
        Some(CommittedSelection {
            stable_id: item.stable_id(),
            source_index: Some(source_index),
            label: item.label().clone(),
            value: item.value().clone(),
        })
    }

    fn same_selection(&self, next: &CommittedSelection<T>) -> bool {
        self.selection.as_ref().is_some_and(|current| {
            if let Some(id) = next.stable_id {
                current.stable_id == Some(id)
            } else {
                current.stable_id.is_none() && current.source_index == next.source_index
            }
        })
    }

    fn rebind_selection(&mut self) {
        let Some(selection) = &mut self.selection else {
            return;
        };
        let source_index = selection.stable_id.and_then(|id| {
            self.autocomplete
                .items()
                .iter()
                .position(|item| item.stable_id() == Some(id))
        });
        let source_index = source_index.or_else(|| {
            selection
                .stable_id
                .is_none()
                .then_some(selection.source_index)
                .flatten()
                .filter(|index| *index < self.autocomplete.items().len())
        });
        selection.source_index = source_index;
        if let Some(item) = source_index.and_then(|index| self.autocomplete.items().get(index)) {
            selection.stable_id = item.stable_id();
            selection.label = item.label().clone();
            selection.value = item.value().clone();
        }
    }

    /// Restore the committed label and reset the edit query. Returns `(changed, query_changed)`.
    fn restore_committed(&mut self, cx: &mut EventContext) -> (bool, bool) {
        let query_changed = !self.query.is_empty();
        if query_changed {
            self.query = Arc::from("");
        }
        let value = self
            .selection
            .as_ref()
            .map(|selection| selection.label.clone())
            .unwrap_or_else(|| Arc::from(""));
        let value_changed = self.autocomplete.set_display_value(value);
        let filter_changed = self.autocomplete.set_query_only("", cx);
        (
            query_changed || value_changed || filter_changed,
            query_changed,
        )
    }
}

fn combobox_autocomplete<T>(state: &mut ComboboxState<T>) -> &mut AutocompleteState<T> {
    &mut state.autocomplete
}

fn derived_combobox_id(parent: ElementId, tag: u64, value: u64) -> ElementId {
    let mut hash = parent.as_u64() ^ tag ^ value.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == parent.as_u64() || hash == u64::MAX {
        hash ^= tag.rotate_left(19);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Application, Color, MouseDownEvent, View, WindowOptions, button, combobox_key_bindings,
        div, text, text_input,
    };
    use std::{cell::Cell, rc::Rc};

    fn options() -> [PickerItem<&'static str>; 4] {
        [
            PickerItem::new("Apple", "apple").id("apple"),
            PickerItem::new("Disabled", "disabled")
                .id("disabled")
                .disabled(true),
            PickerItem::new("Apricot", "apricot").id("apricot"),
            PickerItem::new("Banana", "banana").id("banana"),
        ]
    }

    fn popover_root(state: ComboboxListState) -> Element {
        div()
            .bg(Color::BLACK)
            .child(text(format!("{} results", state.result_count)))
    }

    fn option_row(item: &PickerItem<&'static str>, state: ComboboxOptionState) -> Element {
        div()
            .opacity(if state.active { 1.0 } else { 0.8 })
            .child(text(item.label().clone()))
    }

    #[test]
    fn filter_policy_defaults_to_contains_and_accepts_a_custom_predicate() {
        let mut state = ComboboxState::new(options()).unwrap();
        let mut cx = EventContext::default();
        assert_eq!(state.filter_mode(), PickerFilterMode::Contains);
        assert!(state.filter().is_none());

        state.autocomplete.set_value("ric", &mut cx);
        assert_eq!(state.result_count(), 1, "contains keeps only Apricot");

        assert!(state.set_filter_mode(PickerFilterMode::StartsWith, &mut cx));
        state.autocomplete.set_value("ap", &mut cx);
        assert_eq!(state.result_count(), 2, "Apple and Apricot start with ap");
        state.autocomplete.set_value("ric", &mut cx);
        assert_eq!(state.result_count(), 0, "no label starts with ric");

        let ends_with =
            PickerFilter::new(|label: &str, query: &str| label.to_lowercase().ends_with(query));
        assert!(state.set_filter(Some(ends_with.clone()), &mut cx));
        state.autocomplete.set_value("cot", &mut cx);
        assert_eq!(state.result_count(), 1);
        assert_eq!(state.filter(), Some(&ends_with));
        assert!(!state.set_filter(Some(ends_with), &mut cx));

        assert!(state.set_filter(None, &mut cx));
        state.autocomplete.set_value("ric", &mut cx);
        assert_eq!(state.result_count(), 0, "the declared mode is restored");

        assert!(state.set_filter_mode(PickerFilterMode::None, &mut cx));
        assert_eq!(state.result_count(), 4);
    }

    #[test]
    fn multiple_comboboxes_retain_bounded_chips_that_can_be_removed() {
        let mut state = ComboboxState::new(options()).unwrap().multiple(true);
        assert!(state.is_multiple());
        assert_eq!(state.chip_count(), 0);
        assert!(state.add_chip_source(0));
        assert!(state.add_chip_source(3));
        assert!(!state.add_chip_source(0), "a chip is never duplicated");
        assert!(
            !state.add_chip_source(1),
            "a disabled row cannot become a chip"
        );
        assert_eq!(
            state.chip_labels().map(Arc::as_ref).collect::<Vec<_>>(),
            vec!["Apple", "Banana"]
        );
        assert_eq!(
            state.chip_values().copied().collect::<Vec<_>>(),
            vec!["apple", "banana"]
        );
        assert!(state.is_dirty() && state.is_touched());

        assert!(state.remove_chip(0));
        assert_eq!(
            state.chip_labels().map(Arc::as_ref).collect::<Vec<_>>(),
            vec!["Banana"]
        );
        assert!(!state.remove_chip(9));
        assert!(state.clear_chips());
        assert!(!state.clear_chips());

        let mut bounded = ComboboxState::new(
            (0..MAX_COMBOBOX_VALUES + 4)
                .map(|index| PickerItem::new(format!("Option {index}"), index)),
        )
        .unwrap()
        .multiple(true);
        for index in 0..MAX_COMBOBOX_VALUES + 4 {
            bounded.add_chip_source(index);
        }
        assert_eq!(bounded.chip_count(), MAX_COMBOBOX_VALUES);

        let mut locked = ComboboxState::new(options())
            .unwrap()
            .multiple(true)
            .read_only(true);
        assert!(!locked.add_chip_source(0));
        assert!(!locked.remove_chip(0));
    }

    #[test]
    fn part_state_status_and_empty_mirror_base_ui_attributes() {
        let mut state = ComboboxState::new(options()).unwrap();
        let mut cx = EventContext::default();
        assert_eq!(
            state.state(),
            ComboboxPartState {
                popup_open: false,
                pressed: false,
                placeholder: true,
                valid: true,
                invalid: false,
                dirty: false,
                touched: false,
                filled: false,
                focused: false,
                read_only: false,
                required: false,
            }
        );
        assert_eq!(state.status_text().as_ref(), "4 results");
        assert!(!state.is_empty_result());

        state.autocomplete.set_value("zzz", &mut cx);
        assert!(state.is_empty_result());
        assert_eq!(state.status_text().as_ref(), "No results");
        state.autocomplete.set_value("banana", &mut cx);
        assert_eq!(state.status_text().as_ref(), "1 result");

        assert!(state.set_selected_source(0, &mut cx));
        assert!(state.set_pressed(true));
        assert!(state.set_focused(true));
        let filled = state.state();
        assert!(filled.filled && !filled.placeholder && filled.pressed && filled.focused);
        assert!(state.set_focused(false));
        assert!(state.is_touched());
        assert!(state.set_invalid(true));
        let invalid = state.state();
        assert!(invalid.invalid && !invalid.valid);

        let flagged = ComboboxState::new(options())
            .unwrap()
            .required(true)
            .read_only(true)
            .auto_highlight(true)
            .open_on_input_click(false)
            .highlight_item_on_hover(false)
            .loop_focus(false);
        assert!(flagged.is_required());
        assert!(flagged.is_read_only());
        assert!(flagged.auto_highlights());
        assert!(!flagged.opens_on_input_click());
        assert!(!flagged.highlights_item_on_hover());
        assert!(!flagged.loops_focus());
        let flags = flagged.state();
        assert!(flags.required && flags.read_only);
    }

    #[test]
    fn unstyled_combobox_parts_add_semantics_without_appearance() {
        let state = ComboboxState::new(options()).unwrap().multiple(true);
        type Combo = ComboboxState<&'static str>;

        assert_eq!(
            Combo::root_with(div().bg(Color::BLACK)).visual.background,
            Some(Color::BLACK)
        );
        let label = Combo::label_with("combo", div());
        assert_eq!(label.explicit_id, Some(Combo::label_id("combo")));
        assert_eq!(label.accessibility.role, AccessibilityRole::Label);

        assert!(Combo::value_with("combo", div()).accessibility.hidden);
        assert!(Combo::icon_with("combo", div()).accessibility.hidden);
        assert!(Combo::arrow_with("combo", div()).accessibility.hidden);
        assert!(Combo::backdrop_with("combo", div()).accessibility.hidden);
        assert!(Combo::item_indicator_with(div()).accessibility.hidden);

        let group = Combo::input_group_with("combo", div());
        assert_eq!(group.accessibility.role, AccessibilityRole::Group);
        assert_eq!(
            group.accessibility.relations.labelled_by(),
            Some(Combo::label_id("combo"))
        );

        let input = state.input_with("combo", text_input(""));
        assert!(!input.accessibility.read_only);
        assert_eq!(
            input.accessibility.relations.labelled_by(),
            Some(Combo::label_id("combo"))
        );
        let locked = ComboboxState::new(options())
            .unwrap()
            .read_only(true)
            .required(true)
            .input_with("combo", text_input(""));
        assert!(locked.accessibility.read_only);
        assert!(locked.accessibility.required);

        let chips = Combo::chips_with("combo", div());
        assert_eq!(chips.accessibility.role, AccessibilityRole::List);
        let chip = Combo::chip_with("combo", 0, "Apple", div());
        assert_eq!(chip.accessibility.role, AccessibilityRole::ListItem);
        assert_eq!(chip.accessibility.label.as_deref(), Some("Apple"));
        let remove = Combo::chip_remove_with("combo", 0, "Remove Apple", div());
        assert_eq!(remove.accessibility.role, AccessibilityRole::Button);
        assert!(remove.clickable);
        assert_eq!(remove.visual.background, None);

        let clear = Combo::clear_with("combo", "Clear", div());
        assert_eq!(clear.accessibility.role, AccessibilityRole::Button);
        let trigger = state.trigger_with("combo", "Fruit", div());
        assert_eq!(trigger.accessibility.expanded, Some(false));
        assert_eq!(
            trigger.accessibility.has_popover,
            Some(crate::AccessibilityPopover::ListBox)
        );

        let popup = state.popup_with("combo", div());
        assert_eq!(popup.accessibility.role, AccessibilityRole::ListBox);
        assert!(popup.accessibility.multiselectable);
        assert_eq!(
            state.portal_with("combo", div()).explicit_id,
            popup.explicit_id
        );
        assert_eq!(
            state.positioner_with("combo", div()).explicit_id,
            popup.explicit_id
        );

        let status = Combo::status_with("combo", div());
        assert_eq!(status.accessibility.role, AccessibilityRole::Status);
        assert_eq!(status.accessibility.live, Some(AccessibilityLive::Polite));
        assert!(Combo::empty_with("combo", div()).accessibility.hidden);
        assert_eq!(
            Combo::collection_with("combo", div()).explicit_id,
            Some(Combo::collection_id("combo"))
        );
        assert_eq!(
            Combo::row_with(div()).accessibility.role,
            AccessibilityRole::Group
        );
        assert_eq!(
            Combo::group_with(div()).accessibility.role,
            AccessibilityRole::Group
        );
        assert_eq!(
            Combo::group_label_with("group", div()).accessibility.role,
            AccessibilityRole::Label
        );
        assert_eq!(
            Combo::separator_with(div()).accessibility.role,
            AccessibilityRole::Separator
        );

        let item = state
            .item_with(
                "combo",
                0,
                ComboboxItemPartState {
                    highlighted: true,
                    selected: true,
                    disabled: false,
                },
                div(),
            )
            .unwrap();
        assert_eq!(item.accessibility.role, AccessibilityRole::ListBoxOption);
        assert!(item.accessibility.selected);
        assert_eq!(item.visual.background, None);
        assert!(
            state
                .item_with("combo", 99, ComboboxItemPartState::default(), div())
                .is_none()
        );
    }

    struct Owner {
        combobox: ComboboxState<&'static str>,
        queries: Vec<Arc<str>>,
        changes: Vec<&'static str>,
    }

    impl Owner {
        fn new() -> Self {
            Self {
                combobox: ComboboxState::new(options())
                    .unwrap()
                    .with_layout(ComboboxPopoverLayout::new(220.0, 32.0).max_visible_rows(2))
                    .with_selected_id("banana"),
                queries: Vec::new(),
                changes: Vec::new(),
            }
        }

        fn combobox(view: &mut Self) -> &mut ComboboxState<&'static str> {
            &mut view.combobox
        }
    }

    impl View for Owner {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl crate::IntoElement {
            let input = text_input(self.combobox.input_value().clone())
                .w(220.0)
                .h(36.0);
            let combobox = self.combobox.element(
                cx,
                "fruit",
                "Fruit",
                Self::combobox,
                input,
                popover_root,
                option_row,
                |view, query, _cx| view.queries.push(query),
                |view, value, _cx| view.changes.push(value),
            );
            div().children([combobox, button().id("after-combobox").child("After")])
        }
    }

    #[test]
    fn arbitrary_edits_never_commit_and_every_dismissal_restores_the_committed_label() {
        let (mut cx, owner) = Application::new()
            .bind_keys(combobox_key_bindings())
            .into_test_context(WindowOptions::default(), Owner::new())
            .unwrap();
        let window = owner.window_handle();
        assert_eq!(
            cx.read(owner, |view| view.combobox.input_value().clone())
                .unwrap(),
            Arc::from("Banana")
        );

        cx.focus(window, "fruit").unwrap();
        cx.simulate_keystrokes(window, "platform-a").unwrap();
        cx.simulate_input(window, "ap").unwrap();
        assert!(cx.read(owner, |view| view.combobox.is_open()).unwrap());
        assert_eq!(
            cx.read(owner, |view| view.combobox.query().clone())
                .unwrap(),
            Arc::from("ap")
        );
        cx.simulate_keystrokes(window, "escape").unwrap();
        assert!(!cx.read(owner, |view| view.combobox.is_open()).unwrap());
        assert_eq!(
            cx.read(owner, |view| view.combobox.input_value().clone())
                .unwrap(),
            Arc::from("Banana")
        );
        assert_eq!(
            cx.read(owner, |view| view.combobox.query().clone())
                .unwrap(),
            Arc::from("")
        );
        assert_eq!(
            cx.read(owner, |view| view.combobox.selected_value().copied())
                .unwrap(),
            Some("banana")
        );
        assert!(cx.read(owner, |view| view.changes.is_empty()).unwrap());

        cx.simulate_keystrokes(window, "platform-a").unwrap();
        cx.simulate_input(window, "no such option").unwrap();
        assert!(!cx.dispatch_action(window, crate::ComboboxConfirm).unwrap());
        assert!(cx.read(owner, |view| view.changes.is_empty()).unwrap());
        cx.simulate_keystrokes(window, "escape").unwrap();

        cx.simulate_keystrokes(window, "platform-a").unwrap();
        cx.simulate_input(window, "ap").unwrap();
        cx.simulate_keystrokes(window, "enter").unwrap();
        assert_eq!(
            cx.read(owner, |view| view.combobox.selected_value().copied())
                .unwrap(),
            Some("apple")
        );
        assert_eq!(
            cx.read(owner, |view| view.combobox.input_value().clone())
                .unwrap(),
            Arc::from("Apple")
        );
        assert_eq!(
            cx.read(owner, |view| view.changes.clone()).unwrap(),
            vec!["apple"]
        );
        assert_eq!(
            cx.read(owner, |view| view.queries.last().cloned()).unwrap(),
            Some(Arc::from(""))
        );
    }

    #[test]
    fn tab_and_owner_outside_press_restore_without_stealing_normal_focus_movement() {
        let (mut cx, owner) = Application::new()
            .bind_keys(combobox_key_bindings())
            .into_test_context(WindowOptions::default(), Owner::new())
            .unwrap();
        let window = owner.window_handle();
        cx.focus(window, "fruit").unwrap();
        cx.simulate_keystrokes(window, "platform-a").unwrap();
        cx.simulate_input(window, "ap").unwrap();
        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("after-combobox".into()));
        assert_eq!(
            cx.read(owner, |view| view.combobox.input_value().clone())
                .unwrap(),
            Arc::from("Banana")
        );

        cx.focus(window, "fruit").unwrap();
        cx.simulate_keystrokes(window, "platform-a").unwrap();
        cx.simulate_input(window, "ap").unwrap();
        let popover = cx
            .read(owner, |view| view.combobox.popover_window().unwrap())
            .unwrap();
        assert!(
            !cx.simulate_mouse_down(window, "after-combobox", MouseDownEvent::default(),)
                .unwrap()
        );
        assert!(!cx.is_window_open(popover));
        assert_eq!(
            cx.read(owner, |view| view.combobox.input_value().clone())
                .unwrap(),
            Arc::from("Banana")
        );

        cx.focus(window, "fruit").unwrap();
        cx.simulate_keystrokes(window, "platform-a").unwrap();
        cx.simulate_input(window, "ap").unwrap();
        let popover = cx
            .read(owner, |view| view.combobox.popover_window().unwrap())
            .unwrap();
        cx.update(owner, |_view, cx| cx.close_window_handle(popover))
            .unwrap();
        assert_eq!(
            cx.read(owner, |view| view.combobox.input_value().clone())
                .unwrap(),
            Arc::from("Banana")
        );
        assert_eq!(
            cx.read(owner, |view| view.combobox.query().clone())
                .unwrap(),
            Arc::from("")
        );
    }

    #[test]
    fn never_key_child_click_commits_a_declared_value_and_keeps_owner_input_focus() {
        let (mut cx, owner) = Application::new()
            .bind_keys(combobox_key_bindings())
            .into_test_context(WindowOptions::default(), Owner::new())
            .unwrap();
        let window = owner.window_handle();
        cx.focus(window, "fruit").unwrap();
        cx.click(window, "fruit").unwrap();
        let (popover, row) = cx
            .read(owner, |view| {
                (
                    view.combobox.popover_window().unwrap(),
                    view.combobox.option_id_for_source("fruit", 2).unwrap(),
                )
            })
            .unwrap();
        assert_eq!(
            cx.window_state(popover).unwrap().kind,
            crate::WindowKind::SystemPopover
        );
        assert_eq!(cx.focused(window).unwrap(), Some("fruit".into()));
        cx.click(popover, row).unwrap();
        assert!(!cx.is_window_open(popover));
        assert_eq!(cx.focused(window).unwrap(), Some("fruit".into()));
        assert_eq!(
            cx.read(owner, |view| view.combobox.selected_value().copied())
                .unwrap(),
            Some("apricot")
        );
        assert_eq!(
            cx.read(owner, |view| view.combobox.input_value().clone())
                .unwrap(),
            Arc::from("Apricot")
        );
    }

    #[test]
    fn open_source_replacement_reuses_child_and_stable_selection_survives_absence() {
        let (mut cx, owner) = Application::new()
            .bind_keys(combobox_key_bindings())
            .into_test_context(WindowOptions::default(), Owner::new())
            .unwrap();
        let window = owner.window_handle();
        cx.focus(window, "fruit").unwrap();
        cx.click(window, "fruit").unwrap();
        let popover = cx
            .read(owner, |view| view.combobox.popover_window().unwrap())
            .unwrap();

        cx.update(owner, |view, cx| {
            view.combobox
                .set_items(
                    [
                        PickerItem::new("Cherry", "cherry").id("cherry"),
                        PickerItem::new("Banana renamed", "banana-v2").id("banana"),
                    ],
                    cx,
                )
                .unwrap();
            cx.invalidate();
        })
        .unwrap();
        assert_eq!(
            cx.read(owner, |view| view.combobox.popover_window())
                .unwrap(),
            Some(popover)
        );
        assert_eq!(
            cx.read(owner, |view| view.combobox.selected_source_index())
                .unwrap(),
            Some(1)
        );
        assert_eq!(
            cx.read(owner, |view| view.combobox.selected_value().copied())
                .unwrap(),
            Some("banana-v2")
        );

        cx.update(owner, |view, cx| {
            view.combobox
                .set_items([PickerItem::new("Cherry", "cherry").id("cherry")], cx)
                .unwrap();
            cx.invalidate();
        })
        .unwrap();
        assert_eq!(
            cx.read(owner, |view| view.combobox.popover_window())
                .unwrap(),
            Some(popover)
        );
        assert_eq!(
            cx.read(owner, |view| view.combobox.selected_source_index())
                .unwrap(),
            None
        );
        assert_eq!(
            cx.read(owner, |view| view.combobox.selected_value().copied())
                .unwrap(),
            Some("banana-v2")
        );
        cx.simulate_keystrokes(window, "escape").unwrap();
        assert_eq!(
            cx.read(owner, |view| view.combobox.input_value().clone())
                .unwrap(),
            Arc::from("Banana renamed")
        );

        let error = cx
            .update(owner, |view, cx| {
                view.combobox.set_items(
                    [
                        PickerItem::new("One", "one").id("duplicate"),
                        PickerItem::new("Two", "two").id("duplicate"),
                    ],
                    cx,
                )
            })
            .unwrap()
            .unwrap_err();
        assert_eq!(
            error,
            PickerError::DuplicateId {
                id: "duplicate".into()
            }
        );
        assert_eq!(
            cx.read(owner, |view| view.combobox.items().len()).unwrap(),
            1
        );
        assert_eq!(
            cx.read(owner, |view| view.combobox.selected_value().copied())
                .unwrap(),
            Some("banana-v2")
        );
    }

    #[derive(Debug)]
    struct CloneProbe(Rc<Cell<usize>>);

    impl Clone for CloneProbe {
        fn clone(&self) -> Self {
            self.0.set(self.0.get() + 1);
            Self(Rc::clone(&self.0))
        }
    }

    struct ChipOwner {
        combobox: ComboboxState<&'static str>,
    }

    impl ChipOwner {
        fn new() -> Self {
            Self {
                combobox: ComboboxState::new(options())
                    .unwrap()
                    .with_layout(ComboboxPopoverLayout::new(220.0, 32.0).max_visible_rows(2))
                    .multiple(true),
            }
        }

        fn combobox(view: &mut Self) -> &mut ComboboxState<&'static str> {
            &mut view.combobox
        }
    }

    impl View for ChipOwner {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl crate::IntoElement {
            type Combo = ComboboxState<&'static str>;
            let input = text_input(self.combobox.input_value().clone())
                .w(220.0)
                .h(36.0);
            let input = self.combobox.input_with("fruit", input);
            let combobox = self.combobox.element(
                cx,
                "fruit",
                "Fruit",
                Self::combobox,
                input,
                popover_root,
                option_row,
                |_view, _query, _cx| {},
                |_view, _value, _cx| {},
            );
            let chips = self
                .combobox
                .chip_labels()
                .cloned()
                .enumerate()
                .map(|(index, label)| {
                    let remove = cx.listener(
                        Combo::chip_remove_id("fruit", index),
                        move |view: &mut Self, cx| {
                            if view.combobox.remove_chip(index) {
                                cx.invalidate();
                            }
                        },
                    );
                    Combo::chip_with("fruit", index, label.clone(), div()).child(
                        Combo::chip_remove_with("fruit", index, "Remove", div().h(12.0).w(12.0))
                            .on_click(remove),
                    )
                })
                .collect::<Vec<_>>();
            Combo::root_with(div()).children([
                Combo::label_with("fruit", div().child(text("Fruit"))),
                Combo::input_group_with("fruit", div()).child(combobox),
                Combo::chips_with("fruit", div()).children(chips),
                Combo::status_with("fruit", div().child(text(self.combobox.status_text()))),
            ])
        }
    }

    #[test]
    fn multiple_combobox_commits_into_chips_and_projects_list_semantics() {
        let (mut cx, owner) = Application::new()
            .bind_keys(combobox_key_bindings())
            .into_test_context(WindowOptions::default(), ChipOwner::new())
            .unwrap();
        let window = owner.window_handle();
        cx.focus(window, "fruit").unwrap();
        cx.simulate_input(window, "ban").unwrap();
        cx.simulate_keystrokes(window, "enter").unwrap();

        assert_eq!(
            cx.read(owner, |view| view.combobox.chip_count()).unwrap(),
            1
        );
        assert_eq!(
            cx.read(owner, |view| view
                .combobox
                .chip_labels()
                .map(Arc::as_ref)
                .map(str::to_owned)
                .collect::<Vec<_>>())
                .unwrap(),
            vec!["Banana".to_owned()]
        );
        // A multiple combobox keeps the input an editing query rather than writing the label back.
        assert_eq!(
            cx.read(owner, |view| view.combobox.input_value().clone())
                .unwrap(),
            Arc::from("")
        );

        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("combobox chip accessibility node")
        };
        type Combo = ComboboxState<&'static str>;
        assert_eq!(node(Combo::chips_id("fruit")).role(), accesskit::Role::List);
        let chip = node(Combo::chip_id("fruit", 0));
        assert_eq!(chip.role(), accesskit::Role::ListItem);
        assert_eq!(chip.label(), Some("Banana"));
        assert_eq!(
            node(Combo::chip_remove_id("fruit", 0)).role(),
            accesskit::Role::Button
        );
        assert_eq!(
            node(Combo::status_id("fruit")).role(),
            accesskit::Role::Status
        );

        cx.click(window, Combo::chip_remove_id("fruit", 0)).unwrap();
        assert_eq!(
            cx.read(owner, |view| view.combobox.chip_count()).unwrap(),
            0
        );
    }

    struct LargeOwner {
        combobox: ComboboxState<CloneProbe>,
        clones: Rc<Cell<usize>>,
    }

    impl LargeOwner {
        fn new() -> Self {
            let clones = Rc::new(Cell::new(0));
            let combobox = ComboboxState::new((0..20_000).map(|index| {
                PickerItem::new(format!("Item {index}"), CloneProbe(Rc::clone(&clones)))
                    .id(index as u64 + 1)
            }))
            .unwrap()
            .with_layout(ComboboxPopoverLayout::new(240.0, 30.0).max_visible_rows(5));
            Self { combobox, clones }
        }

        fn combobox(view: &mut Self) -> &mut ComboboxState<CloneProbe> {
            &mut view.combobox
        }
    }

    impl View for LargeOwner {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl crate::IntoElement {
            self.combobox.element(
                cx,
                "large-combobox",
                "Large combobox",
                Self::combobox,
                text_input(self.combobox.input_value().clone()).size(240.0, 36.0),
                |_state| div(),
                |_item, _state| div(),
                |_view, _query, _cx| {},
                |_view, _value, _cx| {},
            )
        }
    }

    #[test]
    fn large_sources_mount_visible_rows_without_value_clones_and_settled_windows_sleep() {
        let (mut cx, owner) = Application::new()
            .bind_keys(combobox_key_bindings())
            .into_test_context(WindowOptions::default(), LargeOwner::new())
            .unwrap();
        let window = owner.window_handle();
        cx.focus(window, "large-combobox").unwrap();
        cx.click(window, "large-combobox").unwrap();
        let popover = cx
            .read(owner, |view| view.combobox.popover_window().unwrap())
            .unwrap();
        assert_eq!(cx.read(owner, |view| view.clones.get()).unwrap(), 0);

        let mounted = (0..32)
            .filter(|source_index| {
                let id = cx
                    .read(owner, |view| {
                        view.combobox
                            .option_id_for_source("large-combobox", *source_index)
                            .unwrap()
                    })
                    .unwrap();
                cx.element_bounds(popover, id).is_ok()
            })
            .count();
        assert!((1..=7).contains(&mounted));

        let owner_renders = cx.render_count(window).unwrap();
        let child_renders = cx.render_count(popover).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), owner_renders);
        assert_eq!(cx.render_count(popover).unwrap(), child_renders);
        assert_eq!(cx.read(owner, |view| view.clones.get()).unwrap(), 0);
    }

    #[test]
    fn programmatic_selection_rejects_disabled_items_and_closed_state_has_no_popover() {
        let mut state = ComboboxState::new(options()).unwrap();
        let mut cx = EventContext::default();
        assert!(!state.set_selected_source(1, &mut cx));
        assert!(state.set_selected_id("banana", &mut cx));
        assert_eq!(state.selected_value(), Some(&"banana"));
        assert_eq!(state.input_value().as_ref(), "Banana");
        assert!(state.clear_selection(&mut cx));
        assert_eq!(state.selected_value(), None);
        assert_eq!(state.input_value().as_ref(), "");
        assert_eq!(state.popover_window(), None);
    }

    #[test]
    fn owner_accessibility_proxy_keeps_committed_selection_distinct_from_active_preview() {
        let (mut cx, owner) = Application::new()
            .bind_keys(combobox_key_bindings())
            .into_test_context(WindowOptions::default(), Owner::new())
            .unwrap();
        let window = owner.window_handle();
        cx.focus(window, "fruit").unwrap();
        cx.click(window, "fruit").unwrap();
        let update = cx.accessibility_update(window).unwrap();
        let node = |role: accesskit::Role, label: &str| {
            update
                .nodes
                .iter()
                .find(|(_node_id, node)| node.role() == role && node.label() == Some(label))
                .unwrap_or_else(|| {
                    panic!(
                        "missing accessibility node {role:?} {label:?}; available={:?}",
                        update
                            .nodes
                            .iter()
                            .map(|(node_id, node)| (*node_id, node.role(), node.label()))
                            .collect::<Vec<_>>()
                    )
                })
        };
        let (input_id, input) = node(accesskit::Role::EditableComboBox, "Fruit");
        let (list_id, _list) = node(accesskit::Role::ListBox, "Fruit");
        let (apple_id, apple) = node(accesskit::Role::ListBoxOption, "Apple");
        let (_banana_id, banana) = node(accesskit::Role::ListBoxOption, "Banana");
        assert_ne!(input_id, list_id);
        assert_eq!(input.controls(), &[*list_id]);
        assert_eq!(input.active_descendant(), Some(*apple_id));
        assert_eq!(apple.is_selected(), None);
        assert_eq!(banana.is_selected(), Some(true));
    }
}
