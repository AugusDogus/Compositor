use std::{fmt, sync::Arc};
use web_time::{Duration, Instant};

use crate::{
    AccessibilityPopover, AccessibilityRole, AnchorPlacement, AnchorSide, AsyncViewContext,
    ComboboxConfirm, ComboboxFirst, ComboboxLast, ComboboxNext, ComboboxPageDown, ComboboxPageUp,
    ComboboxPrevious, Element, ElementId, EventContext, FocusHandle, Key,
    MAX_VALIDATION_MESSAGE_BYTES, Modifiers, PickerError, PickerItem, StateAccessor, Task, View,
    ViewContext, VirtualList, WindowHandle, div, picker::collect_picker_items,
};

/// Maximum option rows mounted by one standalone select popover before virtual scrolling takes over.
pub const MAX_SELECT_VISIBLE_ROWS: usize = 64;
/// Maximum UTF-8 bytes retained by one select popover's incremental typeahead buffer.
pub const MAX_SELECT_TYPEAHEAD_BYTES: usize = 256;
/// Select typeahead expires when the next key arrives after this interval; no timer is scheduled.
pub const SELECT_TYPEAHEAD_TIMEOUT: Duration = Duration::from_millis(500);

/// Maximum values one multiple select retains at once.
pub const MAX_SELECT_VALUES: usize = 256;
/// How often a hovered select scroll arrow advances the option list.
///
/// Each step is an exact one-shot deadline rescheduled by the previous step, so a select whose
/// arrows are not hovered owns no timer, animation, or idle scheduler source.
pub const SELECT_SCROLL_ARROW_INTERVAL: Duration = Duration::from_millis(50);
/// Default separator joining the labels of a multiple select's value text.
pub const DEFAULT_SELECT_VALUE_SEPARATOR: &str = ", ";

const SELECT_KEY_CONTEXT: &str = "Select";
const SELECT_SURFACE_ID_TAG: u64 = 0x99f6_4cc3_13aa_0c81;
const SELECT_OPTION_ID_TAG: u64 = 0x6fe3_f490_a3b8_b271;
const SELECT_LABEL_ID_TAG: u64 = 0x3f5c_9a10_de44_7b62;
const SELECT_VALUE_ID_TAG: u64 = 0xc10b_2e77_5a93_04df;
const SELECT_ICON_ID_TAG: u64 = 0x71a4_6d02_bb18_e5c3;
const SELECT_BACKDROP_ID_TAG: u64 = 0x52d8_0f4e_9c27_a6b1;
const SELECT_ARROW_ID_TAG: u64 = 0x8be1_37c5_2049_da7f;
const SELECT_LIST_ID_TAG: u64 = 0x0a96_c48d_e731_2f50;
const SELECT_SCROLL_UP_ID_TAG: u64 = 0xdd27_5b93_4ac6_1e08;
const SELECT_SCROLL_DOWN_ID_TAG: u64 = 0x64f0_9c81_37ba_d925;

/// Structural geometry for the separate native popover surface used by [`SelectState`].
///
/// It intentionally contains no color, typography, border, radius, shadow, or animation tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectPopoverLayout {
    pub width: f32,
    pub row_height: f32,
    pub max_visible_rows: usize,
    pub placement: AnchorPlacement,
    pub anchor_gap: f32,
    /// Height of the application-owned trigger, used only by `align_item_with_trigger`.
    ///
    /// Presentation stays application-owned, so QuickGUI cannot know how tall the trigger is; a
    /// select that aligns its selected row over the trigger declares that one measurement here.
    pub trigger_height: f32,
}

impl SelectPopoverLayout {
    pub fn new(width: f32, row_height: f32) -> Self {
        Self {
            width: finite_clamped(width, 1.0, 4_096.0, 240.0),
            row_height: finite_clamped(row_height, 20.0, 256.0, 36.0),
            max_visible_rows: 8,
            placement: AnchorPlacement::BottomStart,
            anchor_gap: 4.0,
            trigger_height: finite_clamped(row_height, 1.0, 256.0, 36.0),
        }
    }

    /// Declare the trigger's own height for `align_item_with_trigger`.
    pub fn trigger_height(mut self, height: f32) -> Self {
        self.trigger_height = finite_clamped(height, 1.0, 256.0, 36.0);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = finite_clamped(width, 1.0, 4_096.0, 240.0);
        self
    }

    pub fn row_height(mut self, row_height: f32) -> Self {
        self.row_height = finite_clamped(row_height, 20.0, 256.0, 36.0);
        self
    }

    pub fn max_visible_rows(mut self, rows: usize) -> Self {
        self.max_visible_rows = rows.clamp(1, MAX_SELECT_VISIBLE_ROWS);
        self
    }

    pub const fn placement(mut self, placement: AnchorPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub fn anchor_gap(mut self, gap: f32) -> Self {
        self.anchor_gap = finite_clamped(gap, 0.0, 512.0, 4.0);
        self
    }

    fn sanitized(mut self) -> Self {
        self.width = finite_clamped(self.width, 1.0, 4_096.0, 240.0);
        self.row_height = finite_clamped(self.row_height, 20.0, 256.0, 36.0);
        self.max_visible_rows = self.max_visible_rows.clamp(1, MAX_SELECT_VISIBLE_ROWS);
        self.anchor_gap = finite_clamped(self.anchor_gap, 0.0, 512.0, 4.0);
        self.trigger_height = finite_clamped(self.trigger_height, 1.0, 256.0, 36.0);
        self
    }

    fn popover_height(self, option_count: usize) -> f32 {
        option_count.min(self.max_visible_rows).max(1) as f32 * self.row_height
    }
}

impl Default for SelectPopoverLayout {
    fn default() -> Self {
        Self::new(240.0, 36.0)
    }
}

/// State supplied to the caller-owned popover-root renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectListState {
    pub option_count: usize,
    pub active_index: Option<usize>,
    pub selected_index: Option<usize>,
    /// Whether rows exist above the mounted window, Base UI's `Select.ScrollUpArrow` mounting rule.
    pub can_scroll_up: bool,
    /// Whether rows exist below the mounted window, Base UI's `Select.ScrollDownArrow` rule.
    pub can_scroll_down: bool,
}

/// Framework-owned popup decorators handed to a select's surface renderer.
///
/// Base UI's `Select.ScrollUpArrow` and `Select.ScrollDownArrow` scroll the option list while the
/// pointer rests on them. The list lives inside QuickGUI's separate native option surface, so the
/// behavior cannot be attached from the owner window; these decorators carry it instead. Reach
/// them through [`SelectState::element_with_trigger`]; every other part is decorated directly from
/// [`SelectState`].
pub struct SelectPopupParts<'a> {
    control: ElementId,
    scroll_up: &'a dyn Fn(Element) -> Element,
    scroll_down: &'a dyn Fn(Element) -> Element,
}

impl SelectPopupParts<'_> {
    /// The owner-window identity of the select these parts belong to.
    pub const fn control_id(&self) -> ElementId {
        self.control
    }

    /// Decorate a caller-owned upward scroll affordance, Base UI's `Select.ScrollUpArrow`.
    ///
    /// While the pointer rests on it the option window advances one row every
    /// [`SELECT_SCROLL_ARROW_INTERVAL`], each step an exact one-shot deadline armed by the
    /// previous one. Leaving the arrow, or reaching the end of the list, cancels it, so a settled
    /// select owns no timer. The arrow is hidden from assistive technology: the list it scrolls
    /// already reports its own position.
    pub fn scroll_up_arrow_with(&self, arrow: Element) -> Element {
        (self.scroll_up)(arrow)
    }
    /// Create the unstyled scroll up arrow part. Use [`Self::scroll_up_arrow_with`] to supply an existing element.
    pub fn scroll_up_arrow(&self) -> Element {
        self.scroll_up_arrow_with(crate::div())
    }

    /// Decorate a caller-owned downward scroll affordance, Base UI's `Select.ScrollDownArrow`.
    pub fn scroll_down_arrow_with(&self, arrow: Element) -> Element {
        (self.scroll_down)(arrow)
    }
    /// Create the unstyled scroll down arrow part. Use [`Self::scroll_down_arrow_with`] to supply an existing element.
    pub fn scroll_down_arrow(&self) -> Element {
        self.scroll_down_arrow_with(crate::div())
    }
}

/// State supplied to the caller-owned option renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectOptionState {
    pub source_index: usize,
    pub active: bool,
    pub selected: bool,
    pub disabled: bool,
}

/// A copyable render-state snapshot for one unstyled select.
///
/// Base UI publishes these as the trigger's `data-popup-open`, `data-pressed`,
/// `data-placeholder`, `data-valid`, `data-invalid`, `data-dirty`, `data-touched`, `data-filled`,
/// `data-focused`, `data-readonly`, and `data-required` attributes. QuickGUI has no style sheet,
/// so the same facts arrive as fields the application styles from. Build one with
/// [`SelectState::state`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SelectPartState {
    /// Whether the option surface is open.
    pub popup_open: bool,
    /// The side of the trigger the popup was declared to prefer.
    ///
    /// The option surface is a separate native child window, so the platform resolves the final
    /// side against the display work area; this reports the preference QuickGUI asked for.
    pub popup_side: AnchorSide,
    /// Whether the trigger is currently held down.
    pub pressed: bool,
    /// Whether no value is selected, so the trigger shows its placeholder.
    pub placeholder: bool,
    /// Whether the control currently satisfies its declared constraints.
    pub valid: bool,
    /// Whether the application marked the control invalid.
    pub invalid: bool,
    /// Whether the value changed at least once since the last [`SelectState::reset_dirty`].
    pub dirty: bool,
    /// Whether the control has been focused and left at least once.
    pub touched: bool,
    /// Whether the control holds at least one value.
    pub filled: bool,
    /// Whether the trigger currently owns keyboard focus.
    pub focused: bool,
    /// Whether the control refuses value changes while staying focusable.
    pub read_only: bool,
    /// Whether the control requires a value before submission.
    pub required: bool,
}

/// Bounded controlled state for an unstyled, non-editable single-value select.
///
/// The application owns this value and every visual element. QuickGUI owns keyboard navigation,
/// typeahead, native overflow placement, pointer selection, accessibility semantics, exact child
/// lifecycle synchronization, and visible-row mounting. Closed selects own no window, timer,
/// task, observer, renderer, or scheduler source.
pub struct SelectState<T> {
    items: Arc<[PickerItem<T>]>,
    selected: Vec<usize>,
    popover: Option<WindowHandle>,
    source_revision: u64,
    disabled: bool,
    invalid: bool,
    multiple: bool,
    required: bool,
    read_only: bool,
    modal: bool,
    align_item_with_trigger: bool,
    pressed: bool,
    focused: bool,
    dirty: bool,
    touched: bool,
    value_separator: Arc<str>,
    validation_message: Option<Arc<str>>,
    validation_message_truncated: bool,
    layout: SelectPopoverLayout,
    opaque_popover_background: Option<crate::Color>,
}

impl<T> fmt::Debug for SelectState<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SelectState")
            .field("items", &self.items.len())
            .field("selected", &self.selected)
            .field("multiple", &self.multiple)
            .field("popover", &self.popover)
            .field("source_revision", &self.source_revision)
            .field("disabled", &self.disabled)
            .field("invalid", &self.invalid)
            .field("validation_message", &self.validation_message)
            .field(
                "validation_message_truncated",
                &self.validation_message_truncated,
            )
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

impl<T> SelectState<T> {
    pub fn new(items: impl IntoIterator<Item = PickerItem<T>>) -> Result<Self, PickerError> {
        Ok(Self {
            items: Arc::from(collect_picker_items(items)?),
            selected: Vec::new(),
            popover: None,
            source_revision: 1,
            disabled: false,
            invalid: false,
            multiple: false,
            required: false,
            read_only: false,
            modal: false,
            align_item_with_trigger: false,
            pressed: false,
            focused: false,
            dirty: false,
            touched: false,
            value_separator: Arc::from(DEFAULT_SELECT_VALUE_SEPARATOR),
            validation_message: None,
            validation_message_truncated: false,
            layout: SelectPopoverLayout::default(),
            opaque_popover_background: None,
        })
    }

    /// Use an opaque native popup surface with this clear color.
    ///
    /// This avoids requiring an alpha-capable swapchain on window systems that do not
    /// provide one. The default remains transparent; option-row styling is caller-owned.
    pub fn with_opaque_popover_background(mut self, color: crate::Color) -> Self {
        self.opaque_popover_background = Some(color.with_alpha(1.0));
        self
    }

    pub fn with_layout(mut self, layout: SelectPopoverLayout) -> Self {
        self.layout = layout.sanitized();
        self
    }

    pub fn layout(&self) -> SelectPopoverLayout {
        self.layout
    }

    pub fn set_layout(&mut self, layout: SelectPopoverLayout) -> bool {
        let layout = layout.sanitized();
        if self.layout == layout {
            return false;
        }
        self.layout = layout;
        true
    }

    pub fn items(&self) -> &[PickerItem<T>] {
        &self.items
    }

    /// Create a select from Base UI's `items` map form, one entry per value and its label.
    ///
    /// This is the shortest declaration for a select whose options are a fixed value-to-label
    /// mapping; [`Self::new`] stays available for options that need stable IDs or disabled rows.
    pub fn from_labels(
        items: impl IntoIterator<Item = (T, impl Into<Arc<str>>)>,
    ) -> Result<Self, PickerError> {
        Self::new(
            items
                .into_iter()
                .map(|(value, label)| PickerItem::new(label, value)),
        )
    }

    /// The first selected source index, or `None` when nothing is selected.
    pub fn selected_source_index(&self) -> Option<usize> {
        self.selected.first().copied()
    }

    /// Every selected source index, ascending, Base UI's `multiple` value array.
    ///
    /// A single select holds at most one entry; a multiple select holds at most
    /// [`MAX_SELECT_VALUES`].
    pub fn selected_source_indices(&self) -> &[usize] {
        &self.selected
    }

    /// Every selected item, in source order.
    pub fn selected_items(&self) -> impl Iterator<Item = &PickerItem<T>> {
        self.selected
            .iter()
            .filter_map(|index| self.items.get(*index))
    }

    /// Every selected value, in source order.
    pub fn selected_values(&self) -> impl Iterator<Item = &T> {
        self.selected_items().map(PickerItem::value)
    }

    /// The joined label text a trigger shows, Base UI's `Select.Value`.
    ///
    /// Returns `None` when nothing is selected, which is when Base UI renders the placeholder.
    pub fn value_text(&self) -> Option<Arc<str>> {
        let mut labels = self.selected_items().map(|item| item.label().as_ref());
        let first = labels.next()?;
        let mut text = String::from(first);
        for label in labels {
            text.push_str(&self.value_separator);
            text.push_str(label);
        }
        Some(Arc::from(text))
    }

    /// Replace the separator joining a multiple select's value text.
    pub fn value_separator(mut self, separator: impl Into<Arc<str>>) -> Self {
        self.value_separator = separator.into();
        self
    }

    /// Accept more than one value, Base UI's `multiple`.
    ///
    /// Turning `multiple` off keeps only the first selected value, so the retained set can never
    /// disagree with the declared arity.
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
            self.selected.truncate(1);
        }
        true
    }

    pub const fn is_multiple(&self) -> bool {
        self.multiple
    }

    /// Require a value before submission, Base UI's `required`.
    pub const fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub const fn is_required(&self) -> bool {
        self.required
    }

    /// Refuse value changes while staying focusable, Base UI's `readOnly`.
    pub const fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Contain focus and expect a mounted backdrop while open, Base UI's `modal`.
    pub const fn modal(mut self, modal: bool) -> Self {
        self.modal = modal;
        self
    }

    pub const fn is_modal(&self) -> bool {
        self.modal
    }

    /// Open the popup so the selected row overlaps the trigger, Base UI's `alignItemWithTrigger`.
    ///
    /// QuickGUI offsets the anchored surface by the selected row's distance from the top of the
    /// popup plus [`SelectPopoverLayout::trigger_height`], so the row the user is already looking
    /// at does not move under the pointer. The native surface still resolves the final side
    /// against the display work area.
    pub const fn align_item_with_trigger(mut self, align: bool) -> Self {
        self.align_item_with_trigger = align;
        self
    }

    pub const fn aligns_item_with_trigger(&self) -> bool {
        self.align_item_with_trigger
    }

    /// Record that the trigger is held down, Base UI's `data-pressed`.
    pub const fn set_pressed(&mut self, pressed: bool) -> bool {
        if self.pressed == pressed {
            return false;
        }
        self.pressed = pressed;
        true
    }

    /// Record trigger focus, and mark the control touched when focus leaves it.
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

    /// Mark the control as having been interacted with, Base UI's `data-touched`.
    pub const fn mark_touched(&mut self) -> bool {
        if self.touched {
            return false;
        }
        self.touched = true;
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
    pub fn state(&self) -> SelectPartState {
        SelectPartState {
            popup_open: self.is_open(),
            popup_side: AnchorSide::of(self.layout.placement),
            pressed: self.pressed,
            placeholder: self.selected.is_empty(),
            valid: !self.invalid,
            invalid: self.invalid,
            dirty: self.dirty,
            touched: self.touched,
            filled: !self.selected.is_empty(),
            focused: self.focused,
            read_only: self.read_only,
            required: self.required,
        }
    }

    pub fn selected_item(&self) -> Option<&PickerItem<T>> {
        self.selected_source_index()
            .and_then(|index| self.items.get(index))
    }

    pub fn selected_value(&self) -> Option<&T> {
        self.selected_item().map(PickerItem::value)
    }

    pub const fn popover_window(&self) -> Option<WindowHandle> {
        self.popover
    }

    pub const fn is_open(&self) -> bool {
        self.popover.is_some()
    }

    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub const fn is_invalid(&self) -> bool {
        self.invalid
    }

    /// Replace the complete bounded source atomically and close an obsolete open snapshot.
    pub fn set_items(
        &mut self,
        items: impl IntoIterator<Item = PickerItem<T>>,
        cx: &mut EventContext,
    ) -> Result<(), PickerError> {
        let items = Arc::<[PickerItem<T>]>::from(collect_picker_items(items)?);
        let previous_index = self.selected_source_index();
        let previous_id = self.selected_item().and_then(PickerItem::stable_id);
        self.items = items;
        let restored = previous_id
            .and_then(|id| {
                self.items
                    .iter()
                    .position(|item| item.stable_id() == Some(id))
            })
            .or_else(|| {
                previous_id
                    .is_none()
                    .then_some(previous_index)
                    .flatten()
                    .filter(|index| *index < self.items.len())
            })
            .filter(|index| !self.items[*index].is_disabled());
        self.selected.clear();
        self.selected.extend(restored);
        self.source_revision = self.source_revision.wrapping_add(1).max(1);
        self.close(cx);
        Ok(())
    }

    /// Select one source row.
    ///
    /// A single select replaces its value. A multiple select adds the row, up to
    /// [`MAX_SELECT_VALUES`]. A read-only select refuses both.
    pub fn select_source(&mut self, source_index: usize) -> bool {
        if self.read_only
            || self
                .items
                .get(source_index)
                .is_none_or(PickerItem::is_disabled)
        {
            return false;
        }
        if self.multiple {
            if self.selected.contains(&source_index) || self.selected.len() >= MAX_SELECT_VALUES {
                return false;
            }
            let position = self.selected.partition_point(|index| *index < source_index);
            self.selected.insert(position, source_index);
        } else {
            if self.selected.first() == Some(&source_index) {
                return false;
            }
            self.selected.clear();
            self.selected.push(source_index);
        }
        self.dirty = true;
        self.touched = true;
        true
    }

    /// Toggle one source row, Base UI's `multiple` item behavior.
    ///
    /// A single select clears its value when the already-selected row is chosen again; a multiple
    /// select removes just that value.
    pub fn toggle_source(&mut self, source_index: usize) -> bool {
        if self.read_only
            || self
                .items
                .get(source_index)
                .is_none_or(PickerItem::is_disabled)
        {
            return false;
        }
        if let Some(position) = self
            .selected
            .iter()
            .position(|index| *index == source_index)
        {
            self.selected.remove(position);
            self.dirty = true;
            self.touched = true;
            return true;
        }
        self.select_source(source_index)
    }

    /// Whether one source row is currently selected.
    pub fn is_source_selected(&self, source_index: usize) -> bool {
        self.selected.contains(&source_index)
    }

    pub fn select_id(&mut self, id: impl Into<ElementId>) -> bool {
        let id = id.into();
        self.items
            .iter()
            .position(|item| item.stable_id() == Some(id))
            .is_some_and(|index| self.select_source(index))
    }

    pub fn clear_selection(&mut self) -> bool {
        if self.read_only || self.selected.is_empty() {
            return false;
        }
        self.selected.clear();
        self.dirty = true;
        true
    }

    /// Change disabled state and synchronously request closure of an open native popover.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut EventContext) -> bool {
        if self.disabled == disabled {
            return false;
        }
        self.disabled = disabled;
        if disabled {
            self.close(cx);
        }
        true
    }

    pub fn set_invalid(&mut self, invalid: bool) -> bool {
        if self.invalid == invalid {
            return false;
        }
        self.invalid = invalid;
        true
    }

    pub fn validation_message(&self) -> Option<&Arc<str>> {
        self.validation_message.as_ref()
    }

    pub fn set_validation_message(&mut self, message: impl Into<Arc<str>>) -> bool {
        let (message, truncated) = bounded_validation_message(message.into());
        if self.validation_message == message && self.validation_message_truncated == truncated {
            return false;
        }
        self.validation_message = message;
        self.validation_message_truncated = truncated;
        true
    }

    pub fn clear_validation_message(&mut self) -> bool {
        if self.validation_message.take().is_none() && !self.validation_message_truncated {
            return false;
        }
        self.validation_message_truncated = false;
        true
    }

    pub fn close(&mut self, cx: &mut EventContext) -> bool {
        let Some(popover) = self.popover.take() else {
            return false;
        };
        cx.close_window_handle(popover);
        true
    }

    pub fn surface_id(id: impl Into<ElementId>) -> ElementId {
        derived_select_id(id.into(), SELECT_SURFACE_ID_TAG, 0)
    }

    pub fn option_id_for_source(
        &self,
        id: impl Into<ElementId>,
        source_index: usize,
    ) -> Option<ElementId> {
        let item = self.items.get(source_index)?;
        Some(select_option_id(id.into(), item, source_index))
    }

    /// Stable identity of the mounted label part.
    pub fn label_id(id: impl Into<ElementId>) -> ElementId {
        derived_select_id(id.into(), SELECT_LABEL_ID_TAG, 0)
    }

    /// Stable identity of the mounted value part.
    pub fn value_id(id: impl Into<ElementId>) -> ElementId {
        derived_select_id(id.into(), SELECT_VALUE_ID_TAG, 0)
    }

    /// Stable identity of the mounted icon part.
    pub fn icon_id(id: impl Into<ElementId>) -> ElementId {
        derived_select_id(id.into(), SELECT_ICON_ID_TAG, 0)
    }

    /// Stable identity of the optional owner-window backdrop.
    pub fn backdrop_id(id: impl Into<ElementId>) -> ElementId {
        derived_select_id(id.into(), SELECT_BACKDROP_ID_TAG, 0)
    }

    /// Stable identity of the decorative popup arrow.
    pub fn arrow_id(id: impl Into<ElementId>) -> ElementId {
        derived_select_id(id.into(), SELECT_ARROW_ID_TAG, 0)
    }

    /// Stable identity of the scrolling option list inside the popup.
    pub fn list_id(id: impl Into<ElementId>) -> ElementId {
        derived_select_id(id.into(), SELECT_LIST_ID_TAG, 0)
    }

    /// Stable identity of the upward scroll affordance.
    pub fn scroll_up_arrow_id(id: impl Into<ElementId>) -> ElementId {
        derived_select_id(id.into(), SELECT_SCROLL_UP_ID_TAG, 0)
    }

    /// Stable identity of the downward scroll affordance.
    pub fn scroll_down_arrow_id(id: impl Into<ElementId>) -> ElementId {
        derived_select_id(id.into(), SELECT_SCROLL_DOWN_ID_TAG, 0)
    }

    /// Decorate the optional application-owned structural wrapper, Base UI's `Select.Root`.
    pub fn root_with(root: Element) -> Element {
        root.app_region_no_drag()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root() -> Element {
        Self::root_with(crate::div())
    }

    /// Decorate the caller-owned visible label, Base UI's `Select.Label`.
    ///
    /// [`Self::trigger_with`] points at this identity, so the label names the control without its
    /// text being copied into a second accessible string.
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

    /// Decorate the caller-owned value text inside the trigger, Base UI's `Select.Value`.
    ///
    /// The trigger already exposes the selected value, so this part is hidden from assistive
    /// technology and would otherwise be announced twice. Render
    /// [`Self::value_text`] inside it, or the placeholder when it returns `None`.
    pub fn value_with(id: impl Into<ElementId>, value: Element) -> Element {
        value
            .id(Self::value_id(id))
            .accessibility_hidden(true)
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled value part. Use [`Self::value_with`] to supply an existing element.
    pub fn value(id: impl Into<ElementId>) -> Element {
        Self::value_with(id, crate::div())
    }

    /// Decorate the caller-owned trigger affordance, Base UI's `Select.Icon`.
    pub fn icon_with(id: impl Into<ElementId>, icon: Element) -> Element {
        icon.id(Self::icon_id(id))
            .accessibility_hidden(true)
            .app_region_no_drag()
    }
    /// Create the unstyled icon part. Use [`Self::icon_with`] to supply an existing element.
    pub fn icon(id: impl Into<ElementId>) -> Element {
        Self::icon_with(id, crate::div())
    }

    /// Decorate an optional caller-painted owner-window backdrop, Base UI's `Select.Backdrop`.
    ///
    /// The option surface is a separate native child window that already takes the pointer grab,
    /// so this layer exists only for a caller-painted dimming pass. Mount it while
    /// [`Self::is_open`] is true, and declare [`Self::modal`] so the intent is inspectable.
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

    /// Decorate the popup boundary, Base UI's `Select.Portal` and `Select.Positioner`.
    ///
    /// QuickGUI's option surface is its own native window, so the portal, the positioner, and the
    /// popup are one element: all three names decorate it identically and QuickGUI resolves its
    /// placement against the display work area rather than a parent stacking context.
    pub fn portal_with(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        option_count: usize,
        multiple: bool,
        portal: Element,
    ) -> Element {
        Self::popup_with(id, label, option_count, multiple, portal)
    }
    /// Create the unstyled portal part. Use [`Self::portal_with`] to supply an existing element.
    pub fn portal(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        option_count: usize,
        multiple: bool,
    ) -> Element {
        Self::portal_with(id, label, option_count, multiple, crate::div())
    }

    /// Decorate the popup boundary, Base UI's `Select.Positioner`.
    pub fn positioner_with(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        option_count: usize,
        multiple: bool,
        positioner: Element,
    ) -> Element {
        Self::popup_with(id, label, option_count, multiple, positioner)
    }
    /// Create the unstyled positioner part. Use [`Self::positioner_with`] to supply an existing element.
    pub fn positioner(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        option_count: usize,
        multiple: bool,
    ) -> Element {
        Self::positioner_with(id, label, option_count, multiple, crate::div())
    }

    /// Decorate the caller-owned option surface, Base UI's `Select.Popup`.
    ///
    /// QuickGUI applies this to whatever the surface renderer returns, so an application that
    /// composes its own in-window list can reuse exactly the semantics the native surface gets.
    pub fn popup_with(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        option_count: usize,
        multiple: bool,
        popup: Element,
    ) -> Element {
        popup
            .id(Self::surface_id(id))
            .accessibility_role(AccessibilityRole::ListBox)
            .accessibility_label(label)
            .accessibility_size_of_set(option_count)
            .accessibility_multiselectable(multiple)
            .app_region_no_drag()
            .user_select_none()
            .cursor_default()
    }
    /// Create the unstyled popup part. Use [`Self::popup_with`] to supply an existing element.
    pub fn popup(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        option_count: usize,
        multiple: bool,
    ) -> Element {
        Self::popup_with(id, label, option_count, multiple, crate::div())
    }

    /// Position a caller-owned decorative arrow, Base UI's `Select.Arrow`.
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

    /// Decorate the caller-owned scrolling option list, Base UI's `Select.List`.
    ///
    /// The popup already carries the list-box role, so the inner list is a structural container:
    /// it keeps a stable identity and the declared set size without announcing a second list.
    pub fn list_with(id: impl Into<ElementId>, option_count: usize, list: Element) -> Element {
        list.id(Self::list_id(id))
            .accessibility_hidden(option_count == 0)
            .app_region_no_drag()
    }
    /// Create the unstyled list part. Use [`Self::list_with`] to supply an existing element.
    pub fn list(id: impl Into<ElementId>, option_count: usize) -> Element {
        Self::list_with(id, option_count, crate::div())
    }

    /// Decorate one caller-owned option row, Base UI's `Select.Item`.
    ///
    /// `row_id` comes from [`Self::option_id_for_source`], so the row keeps the identity the
    /// popup's active-descendant relationship points at.
    pub fn item_with(
        row_id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        state: SelectOptionState,
        row: Element,
    ) -> Element {
        row.id(row_id)
            .clickable()
            .tab_index(-1)
            .disabled(state.disabled)
            .selected(state.selected)
            .accessibility_role(AccessibilityRole::ListBoxOption)
            .accessibility_label(label)
            .app_region_no_drag()
            .user_select_none()
            .cursor_default()
    }
    /// Create the unstyled item part. Use [`Self::item_with`] to supply an existing element.
    pub fn item(
        row_id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        state: SelectOptionState,
    ) -> Element {
        Self::item_with(row_id, label, state, crate::div())
    }

    /// Decorate one option's visible text, Base UI's `Select.ItemText`.
    ///
    /// The row already carries the accessible name, so the text is decoration.
    pub fn item_text_with(text: Element) -> Element {
        text.accessibility_hidden(true).app_region_no_drag()
    }
    /// Create the unstyled item text part. Use [`Self::item_text_with`] to supply an existing element.
    pub fn item_text() -> Element {
        Self::item_text_with(crate::div())
    }

    /// Decorate one option's selected mark, Base UI's `Select.ItemIndicator`.
    ///
    /// The row already reports selection, so the indicator is hidden from assistive technology.
    /// Mount it only while the row is selected, exactly as Base UI does.
    pub fn item_indicator_with(indicator: Element) -> Element {
        indicator.accessibility_hidden(true).app_region_no_drag()
    }
    /// Create the unstyled item indicator part. Use [`Self::item_indicator_with`] to supply an existing element.
    pub fn item_indicator() -> Element {
        Self::item_indicator_with(crate::div())
    }

    /// Decorate a caller-composed option group, Base UI's `Select.Group`.
    pub fn group_with(group: Element) -> Element {
        group
            .accessibility_role(AccessibilityRole::Group)
            .app_region_no_drag()
    }
    /// Create the unstyled group part. Use [`Self::group_with`] to supply an existing element.
    pub fn group() -> Element {
        Self::group_with(crate::div())
    }

    /// Decorate a group's visible label, Base UI's `Select.GroupLabel`.
    ///
    /// Pass the same `label_id` to [`Element::accessibility_labelled_by`] on the group so the two
    /// are related without the text being copied.
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

    /// Decorate a caller-owned divider between option groups, Base UI's `Select.Separator`.
    pub fn separator_with(separator: Element) -> Element {
        separator
            .accessibility_role(AccessibilityRole::Separator)
            .app_region_no_drag()
    }
    /// Create the unstyled separator part. Use [`Self::separator_with`] to supply an existing element.
    pub fn separator() -> Element {
        Self::separator_with(crate::div())
    }

    fn scroll_arrow_part(id: ElementId, arrow: Element) -> Element {
        arrow
            .id(id)
            .accessibility_hidden(true)
            .app_region_no_drag()
            .user_select_none()
            .cursor_default()
    }

    /// Decorate an application-owned trigger without adding appearance, Base UI's `Select.Trigger`.
    pub fn trigger_with(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        trigger: Element,
    ) -> Element {
        let id = id.into();
        let mut trigger = trigger
            .id(id)
            .focusable()
            .accessibility_role(AccessibilityRole::ComboBox)
            .accessibility_label(label.into())
            .accessibility_has_popover(AccessibilityPopover::ListBox)
            .accessibility_expanded(self.is_open())
            .accessibility_labelled_by(Self::label_id(id))
            .accessibility_multiselectable(self.multiple)
            .accessibility_read_only(self.read_only)
            .required(self.required)
            .disabled(self.disabled)
            .invalid(self.invalid)
            .app_region_no_drag()
            .user_select_none()
            .cursor_default();
        if let Some(value) = self.value_text() {
            trigger = trigger.accessibility_value(value);
        }
        if let Some(message) = self.validation_message.clone() {
            trigger =
                trigger.validation_message_retained(message, self.validation_message_truncated);
        }
        trigger
    }
    /// Create the unstyled trigger part. Use [`Self::trigger_with`] to supply an existing element.
    pub fn trigger(&self, id: impl Into<ElementId>, label: impl Into<Arc<str>>) -> Element {
        self.trigger_with(id, label, crate::button())
    }

    /// Build the complete unstyled select interaction from caller-owned trigger, popover, and rows.
    ///
    /// This is the `fn`-pointer entry point: a view that owns one select per field passes
    /// `Self::field`. A host that renders many declared selects through one view passes a
    /// per-instance [`StateAccessor`] to [`SelectState::element_with`] instead.
    #[allow(clippy::too_many_arguments)]
    pub fn element<V, PopoverRoot, RenderOption, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        access: fn(&mut V) -> &mut SelectState<T>,
        trigger: Element,
        popover_root: PopoverRoot,
        render_option: RenderOption,
        change: Change,
    ) -> Element
    where
        V: 'static,
        T: Clone + 'static,
        PopoverRoot: Fn(SelectListState) -> Element + Clone + 'static,
        RenderOption: Fn(&PickerItem<T>, SelectOptionState) -> Element + Clone + 'static,
        Change: Fn(&mut V, T, &mut EventContext) + Clone + 'static,
    {
        self.element_with(
            cx,
            id,
            label,
            StateAccessor::from(access),
            trigger,
            popover_root,
            render_option,
            change,
        )
    }

    /// Build the select interaction against a per-instance retained-state accessor.
    ///
    /// Two selects declared by one view stay independent because the accessor, not the view type,
    /// decides which [`SelectState`] each registered listener resolves.
    #[allow(clippy::too_many_arguments)]
    pub fn element_with<V, PopoverRoot, RenderOption, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        access: StateAccessor<V, SelectState<T>>,
        trigger: Element,
        popover_root: PopoverRoot,
        render_option: RenderOption,
        change: Change,
    ) -> Element
    where
        V: 'static,
        T: Clone + 'static,
        PopoverRoot: Fn(SelectListState) -> Element + Clone + 'static,
        RenderOption: Fn(&PickerItem<T>, SelectOptionState) -> Element + Clone + 'static,
        Change: Fn(&mut V, T, &mut EventContext) + Clone + 'static,
    {
        self.element_with_trigger_accessor(
            cx,
            id,
            label,
            access,
            trigger,
            move |list_state, _parts: &SelectPopupParts<'_>| popover_root(list_state),
            render_option,
            change,
        )
    }

    /// Build the select interaction with access to the framework-owned popup parts.
    ///
    /// The surface renderer additionally receives [`SelectPopupParts`], whose
    /// `scroll_up_arrow_with` and `scroll_down_arrow_with` decorators carry Base UI's hovered
    /// scrolling behavior. Every other entry point is this one with a renderer that ignores them.
    #[allow(clippy::too_many_arguments)]
    pub fn element_with_trigger<V, PopoverRoot, RenderOption, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        access: fn(&mut V) -> &mut SelectState<T>,
        trigger: Element,
        popover_root: PopoverRoot,
        render_option: RenderOption,
        change: Change,
    ) -> Element
    where
        V: 'static,
        T: Clone + 'static,
        PopoverRoot: Fn(SelectListState, &SelectPopupParts<'_>) -> Element + Clone + 'static,
        RenderOption: Fn(&PickerItem<T>, SelectOptionState) -> Element + Clone + 'static,
        Change: Fn(&mut V, T, &mut EventContext) + Clone + 'static,
    {
        self.element_with_trigger_accessor(
            cx,
            id,
            label,
            StateAccessor::from(access),
            trigger,
            popover_root,
            render_option,
            change,
        )
    }

    /// Build the popup-parts select interaction against a per-instance retained-state accessor.
    #[allow(clippy::too_many_arguments)]
    pub fn element_with_trigger_accessor<V, PopoverRoot, RenderOption, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        access: StateAccessor<V, SelectState<T>>,
        trigger: Element,
        popover_root: PopoverRoot,
        render_option: RenderOption,
        change: Change,
    ) -> Element
    where
        V: 'static,
        T: Clone + 'static,
        PopoverRoot: Fn(SelectListState, &SelectPopupParts<'_>) -> Element + Clone + 'static,
        RenderOption: Fn(&PickerItem<T>, SelectOptionState) -> Element + Clone + 'static,
        Change: Fn(&mut V, T, &mut EventContext) + Clone + 'static,
    {
        let id = id.into();
        let label = label.into();
        let renderers = SelectRenderers {
            popover_root,
            render_option,
        };

        let closed_access = access.clone();
        cx.on_any_child_window_closed(move |view, closed, cx| {
            let state = closed_access.get(view);
            if state.popover == Some(closed) {
                state.popover = None;
                cx.focus(FocusHandle::new(id));
                cx.invalidate();
            }
        });

        let commit_change = change.clone();
        let commit_access = access.clone();
        let commit = cx.action_listener(id, move |view, action: &SelectCommit, cx| {
            if action.control != id {
                cx.propagate();
                return;
            }
            let value = {
                let state = commit_access.get(view);
                if state.popover != Some(action.popover)
                    || state.source_revision != action.source_revision
                {
                    return;
                }
                let value = state
                    .items
                    .get(action.source_index)
                    .filter(|item| !item.is_disabled())
                    .map(|item| item.value().clone());
                if value.is_some() {
                    if state.multiple {
                        state.toggle_source(action.source_index);
                    } else {
                        state.select_source(action.source_index);
                    }
                    state.popover = None;
                }
                value
            };
            if let Some(value) = value {
                commit_change(view, value, cx);
                cx.focus(FocusHandle::new(id));
                cx.invalidate();
            }
        });

        let click_renderers = renderers.clone();
        let click_label = label.clone();
        let click_access = access.clone();
        let click = cx.listener(id, move |view, cx| {
            open_select_popover(
                view,
                cx,
                id,
                click_label.clone(),
                &click_access,
                click_renderers.clone(),
            );
        });

        let previous_renderers = renderers.clone();
        let previous_label = label.clone();
        let previous_access = access.clone();
        let previous = cx.action_listener(id, move |view, _: &ComboboxPrevious, cx| {
            open_select_popover(
                view,
                cx,
                id,
                previous_label.clone(),
                &previous_access,
                previous_renderers.clone(),
            );
        });
        let next_renderers = renderers.clone();
        let next_label = label.clone();
        let next_access = access.clone();
        let next = cx.action_listener(id, move |view, _: &ComboboxNext, cx| {
            open_select_popover(
                view,
                cx,
                id,
                next_label.clone(),
                &next_access,
                next_renderers.clone(),
            );
        });
        let page_up_renderers = renderers.clone();
        let page_up_label = label.clone();
        let page_up_access = access.clone();
        let page_up = cx.action_listener(id, move |view, _: &ComboboxPageUp, cx| {
            open_select_popover(
                view,
                cx,
                id,
                page_up_label.clone(),
                &page_up_access,
                page_up_renderers.clone(),
            );
        });
        let page_down_renderers = renderers.clone();
        let page_down_label = label.clone();
        let page_down_access = access.clone();
        let page_down = cx.action_listener(id, move |view, _: &ComboboxPageDown, cx| {
            open_select_popover(
                view,
                cx,
                id,
                page_down_label.clone(),
                &page_down_access,
                page_down_renderers.clone(),
            );
        });
        let first_renderers = renderers.clone();
        let first_label = label.clone();
        let first_access = access.clone();
        let first = cx.action_listener(id, move |view, _: &ComboboxFirst, cx| {
            open_select_popover(
                view,
                cx,
                id,
                first_label.clone(),
                &first_access,
                first_renderers.clone(),
            );
        });
        let last_renderers = renderers.clone();
        let last_label = label.clone();
        let last_access = access.clone();
        let last = cx.action_listener(id, move |view, _: &ComboboxLast, cx| {
            open_select_popover(
                view,
                cx,
                id,
                last_label.clone(),
                &last_access,
                last_renderers.clone(),
            );
        });
        let confirm_renderers = renderers;
        let confirm_label = label.clone();
        let confirm_access = access.clone();
        let confirm = cx.action_listener(id, move |view, _: &ComboboxConfirm, cx| {
            open_select_popover(
                view,
                cx,
                id,
                confirm_label.clone(),
                &confirm_access,
                confirm_renderers.clone(),
            );
        });

        self.trigger_with(id, label, trigger)
            .on_click(click)
            .key_context(SELECT_KEY_CONTEXT)
            .on_action(commit)
            .on_action(previous)
            .on_action(next)
            .on_action(page_up)
            .on_action(page_down)
            .on_action(first)
            .on_action(last)
            .on_action(confirm)
    }
}

#[derive(Clone)]
struct SelectRenderers<PopoverRoot, RenderOption> {
    popover_root: PopoverRoot,
    render_option: RenderOption,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SelectCommit {
    control: ElementId,
    popover: WindowHandle,
    source_revision: u64,
    source_index: usize,
}

#[allow(clippy::too_many_arguments)]
fn open_select_popover<V, T, PopoverRoot, RenderOption>(
    view: &mut V,
    cx: &mut EventContext,
    id: ElementId,
    label: Arc<str>,
    access: &StateAccessor<V, SelectState<T>>,
    renderers: SelectRenderers<PopoverRoot, RenderOption>,
) where
    V: 'static,
    T: Clone + 'static,
    PopoverRoot: Fn(SelectListState, &SelectPopupParts<'_>) -> Element + Clone + 'static,
    RenderOption: Fn(&PickerItem<T>, SelectOptionState) -> Element + Clone + 'static,
{
    let (items, selected, selected_source, source_revision, layout, align_item, multiple, background) = {
        let state = access.get(view);
        if state.disabled || state.popover.is_some() {
            return;
        }
        (
            Arc::clone(&state.items),
            state.selected.clone(),
            state.selected_source_index(),
            state.source_revision,
            state.layout,
            state.align_item_with_trigger,
            state.multiple,
            state.opaque_popover_background,
        )
    };
    let popover = SelectPopoverView::new(
        id,
        label,
        items,
        selected,
        selected_source,
        source_revision,
        layout,
        multiple,
        renderers,
    );
    let option_count = popover.items.len();
    let surface = crate::SystemPopover::new(layout.width, layout.popover_height(option_count))
        .placement(layout.placement)
        .gap(if align_item { 0.0 } else { layout.anchor_gap });
    let surface = if align_item {
        let row_top = selected_source.map_or(0.0, |index| {
            let offset = index as f32 * layout.row_height;
            let max_scroll = (option_count as f32 * layout.row_height
                - layout.popover_height(option_count))
            .max(0.0);
            offset - offset.min(max_scroll)
        });
        surface.offset(0.0, -(layout.trigger_height + row_top))
    } else {
        surface
    };
    let mut options = surface.window_options("Select");
    if let Some(background) = background {
        options = options.background(background)
            .window_background(crate::WindowBackgroundAppearance::Opaque);
    }
    let result = cx.open_system_popover(id, options, popover);
    if let Ok(handle) = result {
        access.get(view).popover = Some(handle);
        cx.invalidate();
    }
}

struct SelectPopoverView<T, PopoverRoot, RenderOption> {
    control: ElementId,
    label: Arc<str>,
    items: Arc<[PickerItem<T>]>,
    selected: Vec<usize>,
    selected_source: Option<usize>,
    source_revision: u64,
    multiple: bool,
    active: Option<usize>,
    list: VirtualList,
    layout: SelectPopoverLayout,
    renderers: SelectRenderers<PopoverRoot, RenderOption>,
    typeahead: String,
    typeahead_at: Option<Instant>,
    scroll_direction: Option<bool>,
    scroll_generation: u64,
    scroll_task: Option<Task<()>>,
}

impl<T, PopoverRoot, RenderOption> SelectPopoverView<T, PopoverRoot, RenderOption> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        control: ElementId,
        label: Arc<str>,
        items: Arc<[PickerItem<T>]>,
        selected: Vec<usize>,
        selected_source: Option<usize>,
        source_revision: u64,
        layout: SelectPopoverLayout,
        multiple: bool,
        renderers: SelectRenderers<PopoverRoot, RenderOption>,
    ) -> Self {
        let active = selected_source
            .filter(|index| items.get(*index).is_some_and(|item| !item.is_disabled()))
            .or_else(|| items.iter().position(|item| !item.is_disabled()));
        let mut list = VirtualList::new(items.len(), layout.row_height).with_overscan(1);
        list.set_viewport_height(layout.popover_height(items.len()));
        if let Some(active) = active {
            list.scroll_to_reveal(active);
        }
        Self {
            control,
            label,
            items,
            selected,
            selected_source,
            source_revision,
            multiple,
            active,
            list,
            layout,
            renderers,
            typeahead: String::new(),
            typeahead_at: None,
            scroll_direction: None,
            scroll_generation: 0,
            scroll_task: None,
        }
    }

    fn can_scroll_up(&self) -> bool {
        self.list.scroll_offset() > 0.0
    }

    fn can_scroll_down(&self) -> bool {
        self.list.scroll_offset() < self.list.max_scroll_offset()
    }

    /// Advance the option window one row, reporting whether the offset actually moved.
    fn scroll_step(&mut self, down: bool) -> bool {
        let delta = if down {
            self.layout.row_height
        } else {
            -self.layout.row_height
        };
        self.list.scroll_by(delta)
    }

    fn cancel_scroll(&mut self) {
        self.scroll_generation = self.scroll_generation.wrapping_add(1);
        self.scroll_direction = None;
        if let Some(task) = self.scroll_task.take() {
            task.cancel();
        }
    }

    fn select_previous(&mut self) -> bool {
        self.move_active(false)
    }

    fn select_next(&mut self) -> bool {
        self.move_active(true)
    }

    fn select_first(&mut self) -> bool {
        self.select_boundary(false)
    }

    fn select_last(&mut self) -> bool {
        self.select_boundary(true)
    }

    fn select_page(&mut self, forward: bool) -> bool {
        if self.items.is_empty() {
            return false;
        }
        let page = self.layout.max_visible_rows.max(1);
        let current = self
            .active
            .unwrap_or(if forward { 0 } else { self.items.len() - 1 });
        let target = if forward {
            current.saturating_add(page).min(self.items.len() - 1)
        } else {
            current.saturating_sub(page)
        };
        let candidate = if forward {
            (target..self.items.len())
                .chain(0..target)
                .find(|index| !self.items[*index].is_disabled())
        } else {
            (0..=target)
                .rev()
                .chain((target + 1..self.items.len()).rev())
                .find(|index| !self.items[*index].is_disabled())
        };
        candidate.is_some_and(|index| self.set_active(index))
    }

    fn select_boundary(&mut self, last: bool) -> bool {
        let candidate = if last {
            self.items.iter().rposition(|item| !item.is_disabled())
        } else {
            self.items.iter().position(|item| !item.is_disabled())
        };
        candidate.is_some_and(|index| self.set_active(index))
    }

    fn move_active(&mut self, forward: bool) -> bool {
        let len = self.items.len();
        if len == 0 {
            return false;
        }
        let start = match (self.active, forward) {
            (Some(index), true) => (index + 1) % len,
            (Some(index), false) => (index + len - 1) % len,
            (None, true) => 0,
            (None, false) => len - 1,
        };
        (0..len)
            .map(|offset| {
                if forward {
                    (start + offset) % len
                } else {
                    (start + len - offset) % len
                }
            })
            .find(|index| !self.items[*index].is_disabled())
            .is_some_and(|index| self.set_active(index))
    }

    fn set_active(&mut self, index: usize) -> bool {
        if self.items.get(index).is_none_or(PickerItem::is_disabled) || self.active == Some(index) {
            return false;
        }
        self.active = Some(index);
        self.list.scroll_to_reveal(index);
        self.typeahead.clear();
        self.typeahead_at = None;
        true
    }

    fn typeahead(&mut self, value: &str, now: Instant) -> bool {
        let input = normalized_typeahead_input(value);
        if input.is_empty() {
            return false;
        }
        if self.typeahead_at.is_none_or(|previous| {
            now.saturating_duration_since(previous) > SELECT_TYPEAHEAD_TIMEOUT
        }) {
            self.typeahead.clear();
        }
        self.typeahead_at = Some(now);
        push_bounded(&mut self.typeahead, &input, MAX_SELECT_TYPEAHEAD_BYTES);
        let first = self.typeahead.chars().next();
        let repeated = first.is_some()
            && self
                .typeahead
                .chars()
                .all(|character| Some(character) == first);
        let repeated_prefix;
        let prefix = if repeated && self.typeahead.chars().count() > 1 {
            repeated_prefix = first.expect("non-empty typeahead").to_string();
            repeated_prefix.as_str()
        } else {
            self.typeahead.as_str()
        };
        let len = self.items.len();
        let start = self.active.map_or(0, |active| (active + 1) % len.max(1));
        for offset in 0..len {
            let index = (start + offset) % len;
            let item = &self.items[index];
            if !item.is_disabled() && label_starts_with(item.label(), prefix) {
                let changed = self.active != Some(index);
                self.active = Some(index);
                self.list.scroll_to_reveal(index);
                return changed;
            }
        }
        false
    }

    fn commit(&self, cx: &mut EventContext) -> bool {
        let Some(source_index) = self.active else {
            return false;
        };
        let Some(popover) = cx.window_handle() else {
            return false;
        };
        if !cx.dispatch_action_to_popover_owner(SelectCommit {
            control: self.control,
            popover,
            source_revision: self.source_revision,
            source_index,
        }) {
            return false;
        }
        cx.close_popover_chain()
    }

    fn surface_id(&self) -> ElementId {
        SelectState::<T>::surface_id(self.control)
    }

    fn option_id(&self, source_index: usize) -> ElementId {
        select_option_id(self.control, &self.items[source_index], source_index)
    }
}

impl<T, PopoverRoot, RenderOption> SelectPopoverView<T, PopoverRoot, RenderOption>
where
    T: Clone + 'static,
    PopoverRoot: Fn(SelectListState, &SelectPopupParts<'_>) -> Element + Clone + 'static,
    RenderOption: Fn(&PickerItem<T>, SelectOptionState) -> Element + Clone + 'static,
{
    /// Start or stop the hovered scroll repeat for one arrow.
    fn arrow_hover(&mut self, down: bool, hovered: bool, cx: &mut EventContext) {
        if !hovered {
            if self.scroll_direction == Some(down) {
                self.cancel_scroll();
            }
            return;
        }
        self.cancel_scroll();
        if !self.scroll_step(down) {
            return;
        }
        cx.invalidate();
        self.arm_scroll(down, cx);
    }

    /// Arm the single exact deadline that performs the next scroll step.
    fn arm_scroll(&mut self, down: bool, cx: &mut EventContext) {
        self.scroll_generation = self.scroll_generation.wrapping_add(1);
        self.scroll_direction = Some(down);
        let generation = self.scroll_generation;
        let spawned = cx.spawn(|task_cx: AsyncViewContext<Self>| async move {
            if task_cx.sleep(SELECT_SCROLL_ARROW_INTERVAL).await.is_err() {
                return;
            }
            let _ = task_cx
                .update(move |view, cx| {
                    if view.scroll_generation != generation || view.scroll_direction != Some(down) {
                        return;
                    }
                    view.scroll_task = None;
                    if view.scroll_step(down) {
                        cx.invalidate();
                        view.arm_scroll(down, cx);
                    } else {
                        view.scroll_direction = None;
                    }
                })
                .await;
        });
        match spawned {
            Ok(task) => self.scroll_task = Some(task),
            Err(_) => self.scroll_direction = None,
        }
    }
}

impl<T, PopoverRoot, RenderOption> View for SelectPopoverView<T, PopoverRoot, RenderOption>
where
    T: Clone + 'static,
    PopoverRoot: Fn(SelectListState, &SelectPopupParts<'_>) -> Element + Clone + 'static,
    RenderOption: Fn(&PickerItem<T>, SelectOptionState) -> Element + Clone + 'static,
{
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl crate::IntoElement {
        let surface_id = self.surface_id();
        let previous = cx.action_listener(surface_id, |view, _: &ComboboxPrevious, cx| {
            if view.select_previous() {
                cx.invalidate();
            }
        });
        let next = cx.action_listener(surface_id, |view, _: &ComboboxNext, cx| {
            if view.select_next() {
                cx.invalidate();
            }
        });
        let page_up = cx.action_listener(surface_id, |view, _: &ComboboxPageUp, cx| {
            if view.select_page(false) {
                cx.invalidate();
            }
        });
        let page_down = cx.action_listener(surface_id, |view, _: &ComboboxPageDown, cx| {
            if view.select_page(true) {
                cx.invalidate();
            }
        });
        let first = cx.action_listener(surface_id, |view, _: &ComboboxFirst, cx| {
            if view.select_first() {
                cx.invalidate();
            }
        });
        let last = cx.action_listener(surface_id, |view, _: &ComboboxLast, cx| {
            if view.select_last() {
                cx.invalidate();
            }
        });
        let confirm = cx.action_listener(surface_id, |view, _: &ComboboxConfirm, cx| {
            view.commit(cx);
        });
        let key_down = cx.key_down_listener(surface_id, |view, event, cx| {
            if event.key == Key::Escape {
                cx.close_popover_chain();
                cx.prevent_default();
                cx.stop_propagation();
                return;
            }
            if event
                .modifiers
                .intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER)
            {
                return;
            }
            let Key::Character(value) = event.key_char.as_ref().unwrap_or(&event.key) else {
                return;
            };
            if view.typeahead(value, Instant::now()) {
                cx.invalidate();
            }
            cx.prevent_default();
            cx.stop_propagation();
        });

        // Revealing the active row on every frame would undo a pointer scroll, so it runs only
        // when the mounted window itself changed size.
        let viewport = self.layout.popover_height(self.items.len());
        if self.list.viewport_height() != viewport {
            self.list.set_viewport_height(viewport);
            if let Some(active) = self.active {
                self.list.scroll_to_reveal(active);
            }
        }
        let mut rows = Vec::with_capacity(self.list.visible_rows().len());
        for source_index in self.list.visible_rows().range {
            let item = &self.items[source_index];
            let option_state = SelectOptionState {
                source_index,
                active: self.active == Some(source_index),
                selected: self.selected.contains(&source_index),
                disabled: item.is_disabled(),
            };
            let row_id = self.option_id(source_index);
            let mut row = SelectState::<T>::item_with(
                row_id,
                item.label().clone(),
                option_state,
                (self.renderers.render_option)(item, option_state),
            )
            .accessibility_position_in_set(source_index)
            .absolute()
            .top(source_index as f32 * self.layout.row_height - self.list.scroll_offset())
            .left(0.0)
            .w_full()
            .h(self.layout.row_height)
            .app_region_no_drag()
            .user_select_none()
            .cursor_default();
            if !option_state.disabled {
                let hover = cx.hover_listener(row_id, move |view, hovered, cx| {
                    if *hovered && view.set_active(source_index) {
                        cx.invalidate();
                    }
                });
                let click = cx.listener(row_id, move |view, cx| {
                    if view.set_active(source_index) {
                        cx.invalidate();
                    }
                    view.commit(cx);
                });
                row = row.on_hover(hover).on_click(click);
            }
            rows.push(row);
        }

        let list_state = SelectListState {
            option_count: self.items.len(),
            active_index: self.active,
            selected_index: self.selected_source,
            can_scroll_up: self.can_scroll_up(),
            can_scroll_down: self.can_scroll_down(),
        };
        let options = SelectState::<T>::list_with(
            self.control,
            self.items.len(),
            div()
                .relative()
                .size_full()
                .overflow_hidden()
                .virtual_scroll(&self.list)
                .children(rows),
        );

        let up_id = SelectState::<T>::scroll_up_arrow_id(self.control);
        let down_id = SelectState::<T>::scroll_down_arrow_id(self.control);
        let up_listener = cx.hover_listener(up_id, |view, hovered, cx| {
            view.arrow_hover(false, *hovered, cx);
        });
        let down_listener = cx.hover_listener(down_id, |view, hovered, cx| {
            view.arrow_hover(true, *hovered, cx);
        });
        let scroll_up = move |arrow: Element| {
            SelectState::<T>::scroll_arrow_part(up_id, arrow).on_hover(up_listener)
        };
        let scroll_down = move |arrow: Element| {
            SelectState::<T>::scroll_arrow_part(down_id, arrow).on_hover(down_listener)
        };
        let parts = SelectPopupParts {
            control: self.control,
            scroll_up: &scroll_up,
            scroll_down: &scroll_down,
        };

        let mut root = SelectState::<T>::popup_with(
            self.control,
            self.label.clone(),
            self.items.len(),
            self.multiple,
            (self.renderers.popover_root)(list_state, &parts),
        )
        .track_focus(FocusHandle::new(surface_id))
        .auto_focus()
        .tab_index(-1)
        .key_context(SELECT_KEY_CONTEXT)
        .size_full()
        .overflow_hidden()
        .app_region_no_drag()
        .user_select_none()
        .cursor_default()
        .on_action(previous)
        .on_action(next)
        .on_action(page_up)
        .on_action(page_down)
        .on_action(first)
        .on_action(last)
        .on_action(confirm)
        .on_key_down(key_down)
        .child(options);
        if let Some(active) = self.active {
            root = root.accessibility_active_descendant(self.option_id(active));
        }
        root
    }
}

fn select_option_id<T>(control: ElementId, item: &PickerItem<T>, source_index: usize) -> ElementId {
    derived_select_id(
        control,
        SELECT_OPTION_ID_TAG,
        item.stable_id()
            .map_or(source_index as u64, ElementId::as_u64),
    )
}

fn derived_select_id(parent: ElementId, tag: u64, value: u64) -> ElementId {
    let mut hash = parent.as_u64() ^ tag ^ value.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == parent.as_u64() || hash == u64::MAX {
        hash ^= tag.rotate_left(17);
    }
    ElementId::new(hash)
}

fn normalized_typeahead_input(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .flat_map(char::to_lowercase)
        .collect()
}

fn label_starts_with(label: &str, prefix: &str) -> bool {
    let mut label = label.chars().flat_map(char::to_lowercase);
    prefix
        .chars()
        .all(|expected| label.next() == Some(expected))
}

fn push_bounded(target: &mut String, value: &str, maximum: usize) {
    if target.len() >= maximum {
        return;
    }
    let remaining = maximum - target.len();
    if value.len() <= remaining {
        target.push_str(value);
        return;
    }
    let mut end = remaining;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    target.push_str(&value[..end]);
}

fn finite_clamped(value: f32, minimum: f32, maximum: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        fallback
    }
}

fn bounded_validation_message(message: Arc<str>) -> (Option<Arc<str>>, bool) {
    if message.is_empty() {
        return (None, false);
    }
    if message.len() <= MAX_VALIDATION_MESSAGE_BYTES {
        return (Some(message), false);
    }
    let mut end = MAX_VALIDATION_MESSAGE_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    (Some(Arc::from(&message[..end])), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Application, Color, Point, WindowOptions, select_key_bindings, text};

    fn options() -> [PickerItem<&'static str>; 4] {
        [
            PickerItem::new("Alpha", "alpha").id("alpha"),
            PickerItem::new("Disabled", "disabled")
                .id("disabled")
                .disabled(true),
            PickerItem::new("Beta", "beta").id("beta"),
            PickerItem::new("Bravo", "bravo").id("bravo"),
        ]
    }

    fn popover_root(state: SelectListState, parts: &SelectPopupParts<'_>) -> Element {
        let mut root = div().bg(Color::BLACK).relative();
        if state.can_scroll_up {
            root = root.child(
                parts.scroll_up_arrow_with(div().overlay().top(0.0).left(0.0).w(220.0).h(8.0)),
            );
        }
        if state.can_scroll_down {
            root = root.child(
                parts.scroll_down_arrow_with(div().overlay().top(56.0).left(0.0).w(220.0).h(8.0)),
            );
        }
        root
    }

    fn option_row(item: &PickerItem<&'static str>, state: SelectOptionState) -> Element {
        div()
            .child(text(item.label().clone()))
            .opacity(if state.active { 1.0 } else { 0.8 })
    }

    #[test]
    fn state_replacement_is_atomic_and_preserves_stable_selection() {
        let mut state = SelectState::new(options()).unwrap();
        assert!(state.select_id("beta"));
        let stable_option = state.option_id_for_source("select", 2).unwrap();
        let mut cx = EventContext::default();
        state
            .set_items(
                [
                    PickerItem::new("Beta renamed", "new beta").id("beta"),
                    PickerItem::new("Alpha", "new alpha").id("alpha"),
                ],
                &mut cx,
            )
            .unwrap();
        assert_eq!(state.selected_source_index(), Some(0));
        assert_eq!(state.selected_value(), Some(&"new beta"));
        assert_eq!(state.option_id_for_source("select", 0), Some(stable_option));

        let error = state
            .set_items(
                [
                    PickerItem::new("One", "one").id("same"),
                    PickerItem::new("Two", "two").id("same"),
                ],
                &mut cx,
            )
            .unwrap_err();
        assert_eq!(error, PickerError::DuplicateId { id: "same".into() });
        assert_eq!(state.selected_value(), Some(&"new beta"));
    }

    #[test]
    fn trigger_part_adds_behavior_without_appearance() {
        let state = SelectState::new(options()).unwrap();
        let trigger = state.trigger_with("select", "Theme", div());
        assert_eq!(trigger.accessibility.role, AccessibilityRole::ComboBox);
        assert_eq!(
            trigger.accessibility.has_popover,
            Some(AccessibilityPopover::ListBox)
        );
        assert!(trigger.focusable);
        assert_eq!(trigger.visual.background, None);
        assert_eq!(trigger.visual.border_color, None);
    }

    #[test]
    fn validation_message_bound_preserves_utf8() {
        let mut state = SelectState::new(options()).unwrap();
        let message = "é".repeat(MAX_VALIDATION_MESSAGE_BYTES);
        assert!(state.set_validation_message(message));
        let retained = state.validation_message().unwrap();
        assert!(retained.len() <= MAX_VALIDATION_MESSAGE_BYTES);
        assert!(retained.is_char_boundary(retained.len()));
        assert!(state.validation_message_truncated);
        assert!(state.clear_validation_message());
        assert_eq!(state.validation_message(), None);
        assert!(!state.validation_message_truncated);
    }

    struct SelectOwner {
        select: SelectState<&'static str>,
        value: Option<&'static str>,
    }

    impl Default for SelectOwner {
        fn default() -> Self {
            Self {
                select: SelectState::new(options())
                    .unwrap()
                    .with_layout(SelectPopoverLayout::new(220.0, 32.0).max_visible_rows(2)),
                value: None,
            }
        }
    }

    impl SelectOwner {
        fn select(view: &mut Self) -> &mut SelectState<&'static str> {
            &mut view.select
        }
    }

    impl View for SelectOwner {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl crate::IntoElement {
            self.select.element_with_trigger(
                cx,
                "select",
                "Theme",
                Self::select,
                div().child("Choose"),
                popover_root,
                option_row,
                |view, value, _cx| view.value = Some(value),
            )
        }
    }

    #[test]
    fn native_popover_commits_through_owner_and_close_lifecycle_clears_state() {
        let (mut cx, owner) = Application::new()
            .bind_keys(select_key_bindings())
            .into_test_context(WindowOptions::default(), SelectOwner::default())
            .unwrap();
        cx.click(owner.window_handle(), "select").unwrap();
        let popover = cx
            .read(owner, |view| view.select.popover_window().unwrap())
            .unwrap();
        assert_eq!(
            cx.window_state(popover).unwrap().kind,
            crate::WindowKind::SystemPopover
        );

        cx.simulate_keystrokes(popover, "down enter").unwrap();
        assert!(!cx.is_window_open(popover));
        assert_eq!(cx.read(owner, |view| view.value).unwrap(), Some("beta"));
        assert_eq!(
            cx.read(owner, |view| view.select.popover_window()).unwrap(),
            None
        );

        cx.click(owner.window_handle(), "select").unwrap();
        let popover = cx
            .read(owner, |view| view.select.popover_window().unwrap())
            .unwrap();
        cx.update(owner, |_view, cx| cx.close_window_handle(popover))
            .unwrap();
        assert_eq!(
            cx.read(owner, |view| view.select.popover_window()).unwrap(),
            None
        );
        let renders = cx.render_count(owner.window_handle()).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(owner.window_handle()).unwrap(), renders);
    }

    #[test]
    fn popover_typeahead_is_bounded_cycles_and_skips_disabled_options() {
        let renderers = SelectRenderers {
            popover_root,
            render_option: option_row,
        };
        let mut popover = SelectPopoverView::new(
            "select".into(),
            Arc::from("Theme"),
            Arc::from(options()),
            Vec::new(),
            None,
            1,
            SelectPopoverLayout::default(),
            false,
            renderers,
        );
        let now = Instant::now();
        assert!(popover.typeahead("b", now));
        assert_eq!(popover.active, Some(2));
        assert!(popover.typeahead("b", now + Duration::from_millis(10)));
        assert_eq!(popover.active, Some(3));
        popover.typeahead(&"z".repeat(MAX_SELECT_TYPEAHEAD_BYTES * 2), now);
        assert!(popover.typeahead.len() <= MAX_SELECT_TYPEAHEAD_BYTES);
    }

    #[test]
    fn child_popover_options_remain_visible_only_for_large_sources() {
        let items = Arc::from(
            collect_picker_items(
                (0..20_000).map(|index| PickerItem::new(format!("Option {index}"), index)),
            )
            .unwrap(),
        );
        let popover = SelectPopoverView::new(
            "large-select".into(),
            Arc::from("Large"),
            items,
            vec![19_999],
            Some(19_999),
            1,
            SelectPopoverLayout::new(240.0, 32.0).max_visible_rows(5),
            false,
            SelectRenderers {
                popover_root,
                render_option: |_item: &PickerItem<usize>, _state: SelectOptionState| div(),
            },
        );
        assert!(popover.list.visible_rows().len() <= 7);
        assert_eq!(popover.active, Some(19_999));
    }

    #[test]
    fn multiple_selection_toggles_within_its_bound_and_joins_value_text() {
        let mut state = SelectState::new(options()).unwrap().multiple(true);
        assert!(state.is_multiple());
        assert_eq!(state.value_text(), None);
        assert!(state.select_source(0));
        assert!(state.select_source(3));
        assert!(!state.select_source(1), "a disabled row cannot be selected");
        assert_eq!(state.selected_source_indices(), &[0, 3]);
        assert_eq!(state.value_text().as_deref(), Some("Alpha, Bravo"));
        assert_eq!(
            state.selected_values().copied().collect::<Vec<_>>(),
            vec!["alpha", "bravo"]
        );
        assert!(state.is_source_selected(3));

        assert!(state.toggle_source(0));
        assert_eq!(state.selected_source_indices(), &[3]);
        assert_eq!(state.value_text().as_deref(), Some("Bravo"));

        // Turning multiple off keeps exactly one value.
        assert!(state.select_source(2));
        assert_eq!(state.selected_source_indices(), &[2, 3]);
        assert!(state.set_multiple(false));
        assert_eq!(state.selected_source_indices(), &[2]);
        assert!(!state.set_multiple(false));

        let mut bounded = SelectState::new(
            (0..MAX_SELECT_VALUES + 8)
                .map(|index| PickerItem::new(format!("Option {index}"), index)),
        )
        .unwrap()
        .multiple(true);
        for index in 0..MAX_SELECT_VALUES + 8 {
            bounded.select_source(index);
        }
        assert_eq!(bounded.selected_source_indices().len(), MAX_SELECT_VALUES);
    }

    #[test]
    fn read_only_refuses_changes_and_part_state_mirrors_base_ui_attributes() {
        let mut state = SelectState::new(options()).unwrap();
        assert_eq!(
            state.state(),
            SelectPartState {
                popup_open: false,
                popup_side: AnchorSide::Bottom,
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

        assert!(state.select_source(0));
        assert!(state.is_dirty());
        assert!(state.is_touched());
        assert!(state.set_pressed(true));
        assert!(state.set_focused(true));
        let filled = state.state();
        assert!(filled.filled && !filled.placeholder && filled.dirty && filled.pressed);
        assert!(filled.focused);
        assert!(state.reset_dirty());
        assert!(!state.is_dirty());
        assert!(state.set_focused(false));
        assert!(state.is_touched());

        let mut locked = SelectState::new(options())
            .unwrap()
            .read_only(true)
            .required(true)
            .modal(true)
            .align_item_with_trigger(true);
        assert!(locked.is_read_only());
        assert!(locked.is_required());
        assert!(locked.is_modal());
        assert!(locked.aligns_item_with_trigger());
        assert!(!locked.select_source(0));
        assert!(!locked.toggle_source(0));
        assert!(!locked.clear_selection());
        let locked_state = locked.state();
        assert!(locked_state.read_only && locked_state.required && locked_state.valid);

        let trigger = locked.trigger_with("select", "Theme", div());
        assert!(trigger.accessibility.read_only);
        assert!(trigger.accessibility.required);
        assert_eq!(
            trigger.accessibility.relations.labelled_by(),
            Some(SelectState::<&str>::label_id("select")),
            "the trigger points at the label part's stable identity"
        );
    }

    #[test]
    fn items_map_form_and_unstyled_parts_add_semantics_without_appearance() {
        let state = SelectState::from_labels([("light", "Light"), ("dark", "Dark")]).unwrap();
        assert_eq!(state.items().len(), 2);
        assert_eq!(state.items()[1].label().as_ref(), "Dark");

        assert_eq!(
            SelectState::<&str>::root_with(div().bg(Color::BLACK))
                .visual
                .background,
            Some(Color::BLACK)
        );
        let label = SelectState::<&str>::label_with("select", div());
        assert_eq!(
            label.explicit_id,
            Some(SelectState::<&str>::label_id("select"))
        );
        assert_eq!(label.accessibility.role, AccessibilityRole::Label);

        let value = SelectState::<&str>::value_with("select", div());
        assert!(value.accessibility.hidden);
        assert!(
            SelectState::<&str>::icon_with("select", div())
                .accessibility
                .hidden
        );
        assert!(
            SelectState::<&str>::arrow_with("select", div())
                .accessibility
                .hidden
        );
        assert!(
            SelectState::<&str>::backdrop_with("select", div())
                .accessibility
                .hidden
        );
        assert!(
            SelectState::<&str>::item_text_with(div())
                .accessibility
                .hidden
        );
        assert!(
            SelectState::<&str>::item_indicator_with(div())
                .accessibility
                .hidden
        );

        let popup = SelectState::<&str>::popup_with("select", "Theme", 4, true, div());
        assert_eq!(popup.accessibility.role, AccessibilityRole::ListBox);
        assert!(popup.accessibility.multiselectable);
        assert_eq!(popup.visual.background, None);
        assert_eq!(
            SelectState::<&str>::portal_with("select", "Theme", 4, false, div()).explicit_id,
            popup.explicit_id
        );
        assert_eq!(
            SelectState::<&str>::positioner_with("select", "Theme", 4, false, div()).explicit_id,
            popup.explicit_id
        );

        let item = SelectState::<&str>::item_with(
            "row",
            "Alpha",
            SelectOptionState {
                source_index: 0,
                active: true,
                selected: true,
                disabled: false,
            },
            div(),
        );
        assert_eq!(item.accessibility.role, AccessibilityRole::ListBoxOption);
        assert!(item.accessibility.selected);
        assert_eq!(item.visual.background, None);

        assert_eq!(
            SelectState::<&str>::group_with(div()).accessibility.role,
            AccessibilityRole::Group
        );
        assert_eq!(
            SelectState::<&str>::group_label_with("group-label", div())
                .accessibility
                .role,
            AccessibilityRole::Label
        );
        assert_eq!(
            SelectState::<&str>::separator_with(div())
                .accessibility
                .role,
            AccessibilityRole::Separator
        );
    }

    #[test]
    fn hovered_scroll_arrows_advance_the_option_window_on_exact_deadlines() {
        let (mut cx, owner) = Application::new()
            .bind_keys(select_key_bindings())
            .into_test_context(WindowOptions::default(), SelectOwner::default())
            .unwrap();
        cx.click(owner.window_handle(), "select").unwrap();
        let popover = cx
            .read(owner, |view| view.select.popover_window().unwrap())
            .unwrap();
        let first = SelectState::<&'static str>::option_id_for_source(
            &SelectOwner::default().select,
            "select",
            0,
        )
        .unwrap();
        let last = SelectState::<&'static str>::option_id_for_source(
            &SelectOwner::default().select,
            "select",
            3,
        )
        .unwrap();
        assert!(cx.contains_element(popover, first).unwrap());
        assert!(!cx.contains_element(popover, last).unwrap());
        let down_arrow = SelectState::<&'static str>::scroll_down_arrow_id("select");
        assert!(cx.contains_element(popover, down_arrow).unwrap());

        cx.visual(popover)
            .unwrap()
            .move_pointer(Point::new(110.0, 60.0))
            .unwrap();
        // The first step lands with the pointer event; the rest are exact one-shot deadlines.
        cx.advance_time(SELECT_SCROLL_ARROW_INTERVAL).unwrap();
        cx.advance_time(SELECT_SCROLL_ARROW_INTERVAL).unwrap();
        assert!(cx.contains_element(popover, last).unwrap());
        assert!(!cx.contains_element(popover, first).unwrap());

        // The list is at its end, so the repeat disarms itself and the window settles.
        cx.run_until_idle().unwrap();
        let renders = cx.render_count(popover).unwrap();
        cx.advance_time(SELECT_SCROLL_ARROW_INTERVAL * 4).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(popover).unwrap(), renders);
    }

    #[test]
    fn child_window_config_is_a_real_overflow_capable_popover() {
        let mut state = SelectState::new(options()).unwrap();
        let mut cx = EventContext::default();
        state.popover = Some(WindowHandle::next());
        assert!(state.close(&mut cx));
        assert_eq!(cx.close_windows.len(), 1);
        let options = crate::SystemPopover::new(240.0, 120.0).window_options("Select");
        assert_eq!(options.kind, crate::WindowKind::SystemPopover);
    }
}
