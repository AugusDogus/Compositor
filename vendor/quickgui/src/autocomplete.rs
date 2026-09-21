use std::{fmt, ops::Range, sync::Arc};

use crate::{
    AccessibilityAutoComplete, AccessibilityPopover, AccessibilityRole, AnchorPlacement,
    ComboboxConfirm, ComboboxNext, ComboboxPageDown, ComboboxPageUp, ComboboxPrevious, Element,
    ElementId, Entity, EventContext, FocusHandle, Key, MAX_VALIDATION_MESSAGE_BYTES, PickerError,
    PickerFilter, PickerFilterMode, PickerItem, PickerState, StateAccessor, View, ViewContext,
    VirtualList, WindowHandle, div, element::ElementKind,
};

/// Maximum option rows mounted by one autocomplete popover before virtual scrolling takes over.
pub const MAX_AUTOCOMPLETE_VISIBLE_ROWS: usize = 64;
/// Maximum UTF-8 bytes retained for one free-form autocomplete value.
pub const MAX_AUTOCOMPLETE_VALUE_BYTES: usize = 64 * 1024;

const AUTOCOMPLETE_KEY_CONTEXT: &str = "Combobox";
const SURFACE_ID_TAG: u64 = 0xd12d_e762_27b8_d77d;
const OPTION_ID_TAG: u64 = 0x7673_faca_7f67_69de;

/// Structural geometry for the separate native suggestion surface.
///
/// No color, typography, border, radius, shadow, or animation token is retained here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutocompletePopoverLayout {
    pub width: f32,
    pub row_height: f32,
    pub max_visible_rows: usize,
    pub placement: AnchorPlacement,
    pub anchor_gap: f32,
}

impl AutocompletePopoverLayout {
    pub fn new(width: f32, row_height: f32) -> Self {
        Self {
            width: finite_clamped(width, 1.0, 4_096.0, 280.0),
            row_height: finite_clamped(row_height, 20.0, 256.0, 36.0),
            max_visible_rows: 8,
            placement: AnchorPlacement::BottomStart,
            anchor_gap: 4.0,
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = finite_clamped(width, 1.0, 4_096.0, 280.0);
        self
    }

    pub fn row_height(mut self, row_height: f32) -> Self {
        self.row_height = finite_clamped(row_height, 20.0, 256.0, 36.0);
        self
    }

    pub fn max_visible_rows(mut self, rows: usize) -> Self {
        self.max_visible_rows = rows.clamp(1, MAX_AUTOCOMPLETE_VISIBLE_ROWS);
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
        self.width = finite_clamped(self.width, 1.0, 4_096.0, 280.0);
        self.row_height = finite_clamped(self.row_height, 20.0, 256.0, 36.0);
        self.max_visible_rows = self
            .max_visible_rows
            .clamp(1, MAX_AUTOCOMPLETE_VISIBLE_ROWS);
        self.anchor_gap = finite_clamped(self.anchor_gap, 0.0, 512.0, 4.0);
        self
    }

    fn popover_height(self) -> f32 {
        self.max_visible_rows as f32 * self.row_height
    }
}

impl Default for AutocompletePopoverLayout {
    fn default() -> Self {
        Self::new(280.0, 36.0)
    }
}

/// State supplied to the caller-owned suggestion-surface renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AutocompleteListState {
    pub result_count: usize,
    pub total_match_count: usize,
    pub active_index: Option<usize>,
    pub results_truncated: bool,
}

/// State supplied to one caller-owned suggestion renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct AutocompleteOptionState {
    pub result_index: usize,
    pub source_index: usize,
    pub active: bool,
    pub disabled: bool,
    pub label_ranges: Arc<[Range<usize>]>,
}

/// What committing a suggestion does to the free-form input value.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AutocompleteSelectionBehavior {
    /// Replace the current free-form value with the committed suggestion label.
    #[default]
    CompleteInput,
    /// Report the suggestion while preserving the exact free-form value.
    DismissOnly,
}

#[derive(Clone, Debug, PartialEq)]
struct AutocompleteMatchSnapshot {
    result_index: usize,
    source_index: usize,
    label_ranges: Arc<[Range<usize>]>,
}

struct AutocompletePopoverSnapshot<T> {
    items: Arc<[PickerItem<T>]>,
    matches: Arc<[AutocompleteMatchSnapshot]>,
    total_match_count: usize,
    active_index: Option<usize>,
    source_revision: u64,
}

impl<T> Clone for AutocompletePopoverSnapshot<T> {
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
            matches: self.matches.clone(),
            total_match_count: self.total_match_count,
            active_index: self.active_index,
            source_revision: self.source_revision,
        }
    }
}

impl<T> AutocompletePopoverSnapshot<T> {
    fn list_state(&self) -> AutocompleteListState {
        AutocompleteListState {
            result_count: self.matches.len(),
            total_match_count: self.total_match_count,
            active_index: self.active_index,
            results_truncated: self.total_match_count > self.matches.len(),
        }
    }
}

/// A per-instance path from the owning view to one autocomplete's retained state.
///
/// The outer hop is a cloneable [`StateAccessor`] so a host that declares many autocompletes in
/// one view can address each one; the inner hop stays a plain projection because it depends only
/// on the wrapping state's type, never on which instance is being edited.
pub(crate) struct AutocompleteAccess<V: 'static, S: 'static, T> {
    outer: StateAccessor<V, S>,
    inner: fn(&mut S) -> &mut AutocompleteState<T>,
}

impl<V: 'static, S: 'static, T> Clone for AutocompleteAccess<V, S, T> {
    fn clone(&self) -> Self {
        Self {
            outer: self.outer.clone(),
            inner: self.inner,
        }
    }
}

impl<V: 'static, S: 'static, T> AutocompleteAccess<V, S, T> {
    pub(crate) fn new(
        outer: StateAccessor<V, S>,
        inner: fn(&mut S) -> &mut AutocompleteState<T>,
    ) -> Self {
        Self { outer, inner }
    }

    fn get<'a>(&self, view: &'a mut V) -> &'a mut AutocompleteState<T> {
        (self.inner)(self.outer.get(view))
    }
}

fn autocomplete_identity<T>(state: &mut AutocompleteState<T>) -> &mut AutocompleteState<T> {
    state
}

/// Controlled, free-form autocomplete state with an overflow-capable native suggestion panel.
///
/// The owner input remains the native key and IME target. The visual child panel is never key,
/// while a bounded semantic listbox proxy remains in the owner's AccessKit tree. Closed state owns
/// no native window, entity, timer, task, observer, renderer, or scheduler source.
pub struct AutocompleteState<T> {
    picker: PickerState<T>,
    value: Arc<str>,
    popover: Option<WindowHandle>,
    popover_snapshot: Option<Entity<AutocompletePopoverSnapshot<T>>>,
    source_revision: u64,
    disabled: bool,
    invalid: bool,
    validation_message: Option<Arc<str>>,
    validation_message_truncated: bool,
    layout: AutocompletePopoverLayout,
    selection_behavior: AutocompleteSelectionBehavior,
}

impl<T> fmt::Debug for AutocompleteState<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AutocompleteState")
            .field("picker", &self.picker)
            .field("value", &self.value)
            .field("popover", &self.popover)
            .field("popover_snapshot", &self.popover_snapshot)
            .field("source_revision", &self.source_revision)
            .field("disabled", &self.disabled)
            .field("invalid", &self.invalid)
            .field(
                "validation_message_truncated",
                &self.validation_message_truncated,
            )
            .field("layout", &self.layout)
            .field("selection_behavior", &self.selection_behavior)
            .finish_non_exhaustive()
    }
}

impl<T> AutocompleteState<T> {
    pub fn new(items: impl IntoIterator<Item = PickerItem<T>>) -> Result<Self, PickerError> {
        Ok(Self {
            picker: PickerState::new(items)?,
            value: Arc::from(""),
            popover: None,
            popover_snapshot: None,
            source_revision: 1,
            disabled: false,
            invalid: false,
            validation_message: None,
            validation_message_truncated: false,
            layout: AutocompletePopoverLayout::default(),
            selection_behavior: AutocompleteSelectionBehavior::CompleteInput,
        })
    }

    pub fn with_layout(mut self, layout: AutocompletePopoverLayout) -> Self {
        self.layout = layout.sanitized();
        self
    }

    pub fn with_filter_mode(mut self, filter_mode: PickerFilterMode) -> Self {
        self.picker.set_filter_mode(filter_mode);
        self
    }

    /// Install an application-supplied filter predicate before a runtime context exists.
    ///
    /// This is Base UI's `filter` prop. The predicate replaces [`Self::filter_mode`] entirely;
    /// [`Self::set_filter`] passing `None` restores it.
    pub fn with_filter(mut self, filter: PickerFilter) -> Self {
        self.picker.set_filter(Some(filter));
        self
    }

    /// Set the initial free-form value before a runtime context exists.
    ///
    /// This is the constructor-time equivalent of [`Self::set_value`]: a state that has never been
    /// mounted owns no popover to synchronize, so no [`EventContext`] is needed. The value is
    /// bounded by [`MAX_AUTOCOMPLETE_VALUE_BYTES`] exactly as it is at runtime.
    pub fn with_value(mut self, value: impl Into<Arc<str>>) -> Self {
        self.value = bounded_value(value.into());
        self.picker.set_query(&self.value);
        self
    }

    pub const fn layout(&self) -> AutocompletePopoverLayout {
        self.layout
    }

    /// Replace native popover geometry, closing an open fixed-size child first.
    pub fn set_layout(&mut self, layout: AutocompletePopoverLayout, cx: &mut EventContext) -> bool {
        let layout = layout.sanitized();
        if self.layout == layout {
            return false;
        }
        self.layout = layout;
        self.close(cx);
        true
    }

    pub const fn selection_behavior(&self) -> AutocompleteSelectionBehavior {
        self.selection_behavior
    }

    pub fn set_selection_behavior(&mut self, behavior: AutocompleteSelectionBehavior) -> bool {
        if self.selection_behavior == behavior {
            return false;
        }
        self.selection_behavior = behavior;
        true
    }

    pub fn value(&self) -> &Arc<str> {
        &self.value
    }

    pub(crate) fn picker_query(&self) -> &Arc<str> {
        self.picker.query()
    }

    /// Replace the free-form value and recompute or forward the bounded suggestion query.
    pub fn set_value(&mut self, value: impl Into<Arc<str>>, cx: &mut EventContext) -> bool {
        let value = bounded_value(value.into());
        if self.value == value {
            return false;
        }
        self.value = value;
        self.picker.set_query(&self.value);
        self.sync_popover(cx);
        true
    }

    pub(crate) fn set_display_value(&mut self, value: impl Into<Arc<str>>) -> bool {
        let value = bounded_value(value.into());
        if self.value == value {
            return false;
        }
        self.value = value;
        true
    }

    pub(crate) fn set_query_only(&mut self, query: &str, cx: &mut EventContext) -> bool {
        if !self.picker.set_query(query) {
            return false;
        }
        self.sync_popover(cx);
        true
    }

    pub fn items(&self) -> &[PickerItem<T>] {
        self.picker.items()
    }

    /// Atomically replace suggestions and update an open child without reopening its surface.
    pub fn set_items(
        &mut self,
        items: impl IntoIterator<Item = PickerItem<T>>,
        cx: &mut EventContext,
    ) -> Result<(), PickerError> {
        self.picker.set_items(items)?;
        self.source_revision = self.source_revision.wrapping_add(1).max(1);
        self.sync_popover(cx);
        Ok(())
    }

    pub const fn filter_mode(&self) -> PickerFilterMode {
        self.picker.filter_mode()
    }

    pub fn set_filter_mode(
        &mut self,
        filter_mode: PickerFilterMode,
        cx: &mut EventContext,
    ) -> bool {
        if !self.picker.set_filter_mode(filter_mode) {
            return false;
        }
        self.sync_popover(cx);
        true
    }

    /// The application-supplied filter predicate, when one is installed.
    pub fn filter(&self) -> Option<&PickerFilter> {
        self.picker.filter()
    }

    /// Replace the built-in filter policy with an application-supplied predicate.
    ///
    /// Passing `None` restores [`Self::filter_mode`]. The open suggestion surface, if any, is
    /// resynchronized exactly once.
    pub fn set_filter(&mut self, filter: Option<PickerFilter>, cx: &mut EventContext) -> bool {
        if !self.picker.set_filter(filter) {
            return false;
        }
        self.sync_popover(cx);
        true
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

    pub const fn is_invalid(&self) -> bool {
        self.invalid
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

    pub fn result_count(&self) -> usize {
        self.picker.result_count()
    }

    pub fn total_match_count(&self) -> usize {
        self.picker.total_match_count()
    }

    pub fn active_result_index(&self) -> Option<usize> {
        self.picker.selected_result_index()
    }

    pub fn active_source_index(&self) -> Option<usize> {
        self.picker.selected_source_index()
    }

    pub fn close(&mut self, cx: &mut EventContext) -> bool {
        self.popover_snapshot = None;
        let Some(popover) = self.popover.take() else {
            return false;
        };
        cx.close_window_handle(popover);
        true
    }

    pub fn surface_id(id: impl Into<ElementId>) -> ElementId {
        derived_autocomplete_id(id.into(), SURFACE_ID_TAG, 0)
    }

    pub fn option_id_for_source(
        &self,
        id: impl Into<ElementId>,
        source_index: usize,
    ) -> Option<ElementId> {
        let item = self.picker.item_at(source_index)?;
        Some(autocomplete_option_id(id.into(), item, source_index))
    }

    fn snapshot(&self) -> AutocompletePopoverSnapshot<T> {
        let mut matches = Vec::with_capacity(self.picker.result_count());
        for result_index in 0..self.picker.result_count() {
            let matched = self
                .picker
                .match_at(result_index)
                .expect("bounded autocomplete match exists");
            matches.push(AutocompleteMatchSnapshot {
                result_index,
                source_index: matched.source_index(),
                label_ranges: matched.shared_label_ranges(),
            });
        }
        AutocompletePopoverSnapshot {
            items: self.picker.shared_items(),
            matches: Arc::from(matches),
            total_match_count: self.picker.total_match_count(),
            active_index: self.picker.selected_result_index(),
            source_revision: self.source_revision,
        }
    }

    fn sync_popover(&mut self, cx: &mut EventContext) {
        let Some(entity) = self.popover_snapshot.clone() else {
            return;
        };
        let snapshot = self.snapshot();
        entity.update(cx, |current, _cx| *current = snapshot);
    }

    /// Build the complete unstyled interaction from caller-owned input, popover, and row elements.
    #[allow(clippy::too_many_arguments)]
    pub fn element<V, PopoverRoot, RenderOption, ValueChanged, Select>(
        &mut self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        access: fn(&mut V) -> &mut AutocompleteState<T>,
        input: Element,
        popover_root: PopoverRoot,
        render_option: RenderOption,
        value_changed: ValueChanged,
        select: Select,
    ) -> Element
    where
        V: 'static,
        T: Clone + 'static,
        PopoverRoot: Fn(AutocompleteListState) -> Element + Clone + 'static,
        RenderOption: Fn(&PickerItem<T>, AutocompleteOptionState) -> Element + Clone + 'static,
        ValueChanged: Fn(&mut V, Arc<str>, &mut EventContext) + Clone + 'static,
        Select: Fn(&mut V, T, &mut EventContext) + Clone + 'static,
    {
        self.element_with(
            cx,
            id,
            label,
            StateAccessor::from(access),
            input,
            popover_root,
            render_option,
            value_changed,
            select,
        )
    }

    /// Build the autocomplete against a per-instance retained-state accessor.
    ///
    /// A host that renders many declared autocompletes through one view passes an accessor that
    /// captures which [`AutocompleteState`] each registered listener resolves.
    #[allow(clippy::too_many_arguments)]
    pub fn element_with<V, PopoverRoot, RenderOption, ValueChanged, Select>(
        &mut self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        access: StateAccessor<V, AutocompleteState<T>>,
        input: Element,
        popover_root: PopoverRoot,
        render_option: RenderOption,
        value_changed: ValueChanged,
        select: Select,
    ) -> Element
    where
        V: 'static,
        T: Clone + 'static,
        PopoverRoot: Fn(AutocompleteListState) -> Element + Clone + 'static,
        RenderOption: Fn(&PickerItem<T>, AutocompleteOptionState) -> Element + Clone + 'static,
        ValueChanged: Fn(&mut V, Arc<str>, &mut EventContext) + Clone + 'static,
        Select: Fn(&mut V, T, &mut EventContext) + Clone + 'static,
    {
        let select_with_source =
            move |view: &mut V, _source_index: usize, value: T, cx: &mut EventContext| {
                select(view, value, cx);
            };
        self.element_with_source(
            cx,
            id,
            label,
            AutocompleteAccess::new(access, autocomplete_identity::<T>),
            input,
            popover_root,
            render_option,
            value_changed,
            select_with_source,
            |_view, _cx| {},
            None,
        )
    }

    /// Internal composition hook shared with the constrained combobox.
    ///
    /// The public free-form API intentionally reports only the application value. A constrained
    /// combobox additionally needs the exact source identity and every dismissal path so it can
    /// restore its last committed label without duplicating the native popover engine.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn element_with_source<
        V,
        S,
        PopoverRoot,
        RenderOption,
        ValueChanged,
        Select,
        Dismiss,
    >(
        &mut self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        access_source: AutocompleteAccess<V, S, T>,
        input: Element,
        popover_root: PopoverRoot,
        render_option: RenderOption,
        value_changed: ValueChanged,
        select: Select,
        dismiss: Dismiss,
        accessibility_selection: Option<usize>,
    ) -> Element
    where
        V: 'static,
        S: 'static,
        T: Clone + 'static,
        PopoverRoot: Fn(AutocompleteListState) -> Element + Clone + 'static,
        RenderOption: Fn(&PickerItem<T>, AutocompleteOptionState) -> Element + Clone + 'static,
        ValueChanged: Fn(&mut V, Arc<str>, &mut EventContext) + Clone + 'static,
        Select: Fn(&mut V, usize, T, &mut EventContext) + Clone + 'static,
        Dismiss: Fn(&mut V, &mut EventContext) + Clone + 'static,
    {
        let mut input = input;
        match &mut input.kind {
            ElementKind::TextInput(input) if !input.multiline => {
                if input.value != self.value {
                    input.value = self.value.clone();
                    input.highlights = Arc::from([]);
                }
            }
            _ => panic!("AutocompleteState::element requires a single-line text_input element"),
        }
        let id = id.into();
        let label = label.into();
        let focus = FocusHandle::new(id);
        let surface_id = Self::surface_id(id);
        let renderers = AutocompleteRenderers {
            popover_root,
            render_option,
        };
        let layout = self.layout;
        self.picker
            .set_result_viewport_height(layout.popover_height());
        if let Some(active) = self.picker.selected_result_index() {
            self.picker.select_result(active);
        }

        let child_dismiss = dismiss.clone();
        let access = access_source.clone();
        cx.on_any_child_window_closed(move |view, closed, cx| {
            let state = access.get(view);
            if state.popover == Some(closed) {
                state.popover = None;
                state.popover_snapshot = None;
                child_dismiss(view, cx);
                cx.invalidate();
            }
        });

        let access = access_source.clone();
        let preview = cx.action_listener(id, move |view, action: &AutocompletePreview, cx| {
            if action.control != id {
                cx.propagate();
                return;
            }
            let changed = {
                let state = access.get(view);
                if state.popover != Some(action.popover)
                    || state.source_revision != action.source_revision
                {
                    return;
                }
                state.picker.select_result(action.result_index)
            };
            if changed {
                access.get(view).sync_popover(cx);
                cx.invalidate();
            }
        });

        let commit_value_changed = value_changed.clone();
        let commit_select = select.clone();
        let access = access_source.clone();
        let commit = cx.action_listener(id, move |view, action: &AutocompleteCommit, cx| {
            if action.control != id {
                cx.propagate();
                return;
            }
            commit_autocomplete_source(
                view,
                cx,
                &access,
                Some((action.popover, action.source_revision)),
                action.source_index,
                &commit_value_changed,
                &commit_select,
            );
        });

        let input_renderers = renderers.clone();
        let input_label = label.clone();
        let input_value_changed = value_changed.clone();
        let access = access_source.clone();
        let input_listener = cx.input_listener(id, move |view, value, cx| {
            let (changed, value) = {
                let state = access.get(view);
                let changed = state.set_value(value, cx);
                (changed, state.value.clone())
            };
            if changed {
                input_value_changed(view, value, cx);
            }
            open_autocomplete_popover(
                view,
                cx,
                id,
                input_label.clone(),
                &access,
                input_renderers.clone(),
            );
            if changed {
                cx.invalidate();
            }
        });

        let click_renderers = renderers.clone();
        let click_label = label.clone();
        let access = access_source.clone();
        let click = cx.listener(id, move |view, cx| {
            open_autocomplete_popover(
                view,
                cx,
                id,
                click_label.clone(),
                &access,
                click_renderers.clone(),
            );
            cx.focus(focus);
        });

        let previous_renderers = renderers.clone();
        let previous_label = label.clone();
        let access = access_source.clone();
        let previous = cx.action_listener(id, move |view, _: &ComboboxPrevious, cx| {
            if access.get(view).is_open() {
                if access.get(view).picker.select_previous() {
                    access.get(view).sync_popover(cx);
                    cx.invalidate();
                }
            } else {
                open_autocomplete_popover(
                    view,
                    cx,
                    id,
                    previous_label.clone(),
                    &access,
                    previous_renderers.clone(),
                );
            }
        });
        let next_renderers = renderers.clone();
        let next_label = label.clone();
        let access = access_source.clone();
        let next = cx.action_listener(id, move |view, _: &ComboboxNext, cx| {
            if access.get(view).is_open() {
                if access.get(view).picker.select_next() {
                    access.get(view).sync_popover(cx);
                    cx.invalidate();
                }
            } else {
                open_autocomplete_popover(
                    view,
                    cx,
                    id,
                    next_label.clone(),
                    &access,
                    next_renderers.clone(),
                );
            }
        });
        let page_up_renderers = renderers.clone();
        let page_up_label = label.clone();
        let access = access_source.clone();
        let page_up = cx.action_listener(id, move |view, _: &ComboboxPageUp, cx| {
            if access.get(view).is_open() {
                if access.get(view).picker.select_page_up() {
                    access.get(view).sync_popover(cx);
                    cx.invalidate();
                }
            } else {
                open_autocomplete_popover(
                    view,
                    cx,
                    id,
                    page_up_label.clone(),
                    &access,
                    page_up_renderers.clone(),
                );
            }
        });
        let page_down_renderers = renderers;
        let page_down_label = label.clone();
        let access = access_source.clone();
        let page_down = cx.action_listener(id, move |view, _: &ComboboxPageDown, cx| {
            if access.get(view).is_open() {
                if access.get(view).picker.select_page_down() {
                    access.get(view).sync_popover(cx);
                    cx.invalidate();
                }
            } else {
                open_autocomplete_popover(
                    view,
                    cx,
                    id,
                    page_down_label.clone(),
                    &access,
                    page_down_renderers.clone(),
                );
            }
        });

        let confirm_value_changed = value_changed;
        let confirm_select = select;
        let access = access_source.clone();
        let confirm = cx.action_listener(id, move |view, _: &ComboboxConfirm, cx| {
            let source_index = access.get(view).active_source_index();
            let Some(source_index) = source_index else {
                cx.propagate();
                return;
            };
            if !commit_autocomplete_source(
                view,
                cx,
                &access,
                None,
                source_index,
                &confirm_value_changed,
                &confirm_select,
            ) {
                cx.propagate();
            }
        });

        let key_dismiss = dismiss.clone();
        let access = access_source.clone();
        let key_down = cx.key_down_listener(id, move |view, event, cx| {
            if event.key == Key::Escape {
                if access.get(view).close(cx) {
                    key_dismiss(view, cx);
                    cx.invalidate();
                    cx.prevent_default();
                    cx.stop_propagation();
                }
            } else if event.key == Key::Tab && access.get(view).close(cx) {
                key_dismiss(view, cx);
                cx.invalidate();
            }
        });

        let mut input = input
            .id(id)
            .track_focus(focus)
            .on_input(input_listener)
            .on_click(click)
            .on_key_down(key_down)
            .key_context(AUTOCOMPLETE_KEY_CONTEXT)
            .on_action(preview)
            .on_action(commit)
            .on_action(previous)
            .on_action(next)
            .on_action(page_up)
            .on_action(page_down)
            .on_action(confirm)
            .accessibility_role(AccessibilityRole::EditableComboBox)
            .accessibility_label(label.clone())
            .accessibility_auto_complete(AccessibilityAutoComplete::List)
            .accessibility_has_popover(AccessibilityPopover::ListBox)
            .accessibility_expanded(self.is_open())
            .disabled(self.disabled)
            .invalid(self.invalid)
            .app_region_no_drag()
            .cursor_text();
        if let Some(message) = self.validation_message.clone() {
            input = input.validation_message_retained(message, self.validation_message_truncated);
        }

        let mut proxy = None;
        if self.is_open() {
            let outside_dismiss = dismiss;
            let access = access_source.clone();
            let outside = cx.mouse_down_listener(id, move |view, _event, cx| {
                if access.get(view).close(cx) {
                    outside_dismiss(view, cx);
                    cx.invalidate();
                }
            });
            input = input.on_mouse_down_out(outside);
            input = input.accessibility_controls(surface_id);
            if let Some(active) = self.picker.selected_result_index()
                && let Some(matched) = self.picker.match_at(active)
            {
                input = input.accessibility_active_descendant(autocomplete_option_id(
                    id,
                    matched.item(),
                    matched.source_index(),
                ));
            }
            proxy = Some(self.accessibility_proxy(id, label, surface_id, accessibility_selection));
        }

        let mut root = div().relative().child(input);
        if let Some(proxy) = proxy {
            root = root.child(proxy);
        }
        root
    }

    fn accessibility_proxy(
        &self,
        id: ElementId,
        label: Arc<str>,
        surface_id: ElementId,
        committed_source: Option<usize>,
    ) -> Element {
        let visible = self.picker.virtual_list().visible_rows().range;
        let active = self.picker.selected_result_index();
        let mut result_indices = visible.collect::<Vec<_>>();
        if let Some(active) = active
            && !result_indices.contains(&active)
        {
            result_indices.push(active);
        }
        if let Some(committed_source) = committed_source
            && let Some(committed_result) = (0..self.picker.result_count()).find(|result_index| {
                self.picker
                    .match_at(*result_index)
                    .is_some_and(|matched| matched.source_index() == committed_source)
            })
            && !result_indices.contains(&committed_result)
        {
            result_indices.push(committed_result);
        }
        result_indices.sort_unstable();
        result_indices.dedup();
        result_indices.truncate(MAX_AUTOCOMPLETE_VISIBLE_ROWS.saturating_add(4));

        let mut options = Vec::with_capacity(result_indices.len());
        for result_index in result_indices {
            let Some(matched) = self.picker.match_at(result_index) else {
                continue;
            };
            let item = matched.item();
            options.push(
                div()
                    .id(autocomplete_option_id(id, item, matched.source_index()))
                    .accessibility_role(AccessibilityRole::ListBoxOption)
                    .accessibility_label(item.label().clone())
                    .accessibility_position_in_set(result_index)
                    .accessibility_size_of_set(self.picker.result_count())
                    .selected(
                        committed_source.map_or(active == Some(result_index), |source| {
                            source == matched.source_index()
                        }),
                    )
                    .disabled(item.is_disabled())
                    .absolute()
                    .size(1.0, 1.0),
            );
        }
        div()
            .id(surface_id)
            .absolute()
            .size(1.0, 1.0)
            .overflow_hidden()
            .accessibility_role(AccessibilityRole::ListBox)
            .accessibility_label(label)
            .accessibility_size_of_set(self.picker.result_count())
            .children(options)
    }
}

#[derive(Clone)]
struct AutocompleteRenderers<PopoverRoot, RenderOption> {
    popover_root: PopoverRoot,
    render_option: RenderOption,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AutocompletePreview {
    control: ElementId,
    popover: WindowHandle,
    source_revision: u64,
    result_index: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AutocompleteCommit {
    control: ElementId,
    popover: WindowHandle,
    source_revision: u64,
    source_index: usize,
}

fn open_autocomplete_popover<V, S, T, PopoverRoot, RenderOption>(
    view: &mut V,
    cx: &mut EventContext,
    id: ElementId,
    label: Arc<str>,
    access: &AutocompleteAccess<V, S, T>,
    renderers: AutocompleteRenderers<PopoverRoot, RenderOption>,
) where
    V: 'static,
    S: 'static,
    T: Clone + 'static,
    PopoverRoot: Fn(AutocompleteListState) -> Element + Clone + 'static,
    RenderOption: Fn(&PickerItem<T>, AutocompleteOptionState) -> Element + Clone + 'static,
{
    let (snapshot, layout) = {
        let state = access.get(view);
        if state.disabled || state.popover.is_some() {
            return;
        }
        (state.snapshot(), state.layout)
    };
    let entity = Entity::new(snapshot);
    let popover = AutocompletePopoverView::new(id, entity.clone(), layout, renderers);
    let result = crate::SystemPopover::new(layout.width, layout.popover_height())
        .placement(layout.placement)
        .gap(layout.anchor_gap)
        .grab(false)
        .accepts_key_focus(false)
        .open(cx, id, label.to_string(), popover);
    if let Ok(handle) = result {
        let state = access.get(view);
        state.popover = Some(handle);
        state.popover_snapshot = Some(entity);
        cx.invalidate();
    }
}

fn commit_autocomplete_source<V, S, T, ValueChanged, Select>(
    view: &mut V,
    cx: &mut EventContext,
    access: &AutocompleteAccess<V, S, T>,
    expected: Option<(WindowHandle, u64)>,
    source_index: usize,
    value_changed: &ValueChanged,
    select: &Select,
) -> bool
where
    S: 'static,
    T: Clone,
    ValueChanged: Fn(&mut V, Arc<str>, &mut EventContext),
    Select: Fn(&mut V, usize, T, &mut EventContext),
{
    let (value, completed_value) = {
        let state = access.get(view);
        let Some(popover) = state.popover else {
            return false;
        };
        if let Some((expected_popover, expected_revision)) = expected
            && (popover != expected_popover || state.source_revision != expected_revision)
        {
            return false;
        }
        let Some(item) = state
            .picker
            .item_at(source_index)
            .filter(|item| !item.is_disabled())
        else {
            return false;
        };
        let value = item.value().clone();
        let label = item.label().clone();
        let completed_value = if state.selection_behavior
            == AutocompleteSelectionBehavior::CompleteInput
            && state.value != label
        {
            state.value = label.clone();
            state.picker.set_query(&label);
            Some(label)
        } else {
            None
        };
        state.close(cx);
        (value, completed_value)
    };
    if let Some(value) = completed_value {
        value_changed(view, value, cx);
    }
    select(view, source_index, value, cx);
    cx.invalidate();
    true
}

struct AutocompletePopoverView<T, PopoverRoot, RenderOption> {
    control: ElementId,
    snapshot: Entity<AutocompletePopoverSnapshot<T>>,
    list: VirtualList,
    layout: AutocompletePopoverLayout,
    renderers: AutocompleteRenderers<PopoverRoot, RenderOption>,
}

impl<T, PopoverRoot, RenderOption> AutocompletePopoverView<T, PopoverRoot, RenderOption> {
    fn new(
        control: ElementId,
        snapshot: Entity<AutocompletePopoverSnapshot<T>>,
        layout: AutocompletePopoverLayout,
        renderers: AutocompleteRenderers<PopoverRoot, RenderOption>,
    ) -> Self {
        let (len, active) =
            snapshot.read(|snapshot| (snapshot.matches.len(), snapshot.active_index));
        let mut list = VirtualList::new(len, layout.row_height).with_overscan(1);
        list.set_viewport_height(layout.popover_height());
        if let Some(active) = active {
            list.scroll_to_reveal(active);
        }
        Self {
            control,
            snapshot,
            list,
            layout,
            renderers,
        }
    }
}

impl<T, PopoverRoot, RenderOption> View for AutocompletePopoverView<T, PopoverRoot, RenderOption>
where
    T: Clone + 'static,
    PopoverRoot: Fn(AutocompleteListState) -> Element + Clone + 'static,
    RenderOption: Fn(&PickerItem<T>, AutocompleteOptionState) -> Element + Clone + 'static,
{
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl crate::IntoElement {
        let snapshot = cx.observe(&self.snapshot, Clone::clone);
        self.list.set_len(snapshot.matches.len());
        self.list.set_viewport_height(self.layout.popover_height());
        if let Some(active) = snapshot.active_index {
            self.list.scroll_to_reveal(active);
        }

        let popover = cx.window_handle();
        let mut rows = Vec::with_capacity(self.list.visible_rows().len());
        for result_index in self.list.visible_rows().range {
            let Some(matched) = snapshot.matches.get(result_index) else {
                continue;
            };
            let Some(item) = snapshot.items.get(matched.source_index) else {
                continue;
            };
            let option_state = AutocompleteOptionState {
                result_index: matched.result_index,
                source_index: matched.source_index,
                active: snapshot.active_index == Some(result_index),
                disabled: item.is_disabled(),
                label_ranges: matched.label_ranges.clone(),
            };
            let row_id = autocomplete_option_id(self.control, item, matched.source_index);
            let mut row = (self.renderers.render_option)(item, option_state.clone())
                .id(row_id)
                .clickable()
                .tab_index(-1)
                .disabled(option_state.disabled)
                .selected(option_state.active)
                .absolute()
                .top(result_index as f32 * self.layout.row_height - self.list.scroll_offset())
                .left(0.0)
                .w_full()
                .h(self.layout.row_height)
                .app_region_no_drag()
                .user_select_none()
                .cursor_default();
            if !option_state.disabled {
                let control = self.control;
                let source_revision = snapshot.source_revision;
                let hover = cx.hover_listener(row_id, move |_view, hovered, cx| {
                    if *hovered {
                        cx.dispatch_action_to_popover_owner(AutocompletePreview {
                            control,
                            popover,
                            source_revision,
                            result_index,
                        });
                    }
                });
                let source_index = matched.source_index;
                let click = cx.listener(row_id, move |_view, cx| {
                    if cx.dispatch_action_to_popover_owner(AutocompleteCommit {
                        control,
                        popover,
                        source_revision,
                        source_index,
                    }) {
                        cx.close_window();
                    }
                });
                row = row.on_hover(hover).on_click(click);
            }
            rows.push(row);
        }

        let options = div()
            .relative()
            .size_full()
            .overflow_hidden()
            .virtual_scroll(&self.list)
            .children(rows);
        (self.renderers.popover_root)(snapshot.list_state())
            .id(AutocompleteState::<T>::surface_id(self.control))
            .size_full()
            .overflow_hidden()
            .app_region_no_drag()
            .user_select_none()
            .cursor_default()
            .accessibility_hidden(true)
            .child(options)
    }
}

fn autocomplete_option_id<T>(
    control: ElementId,
    item: &PickerItem<T>,
    source_index: usize,
) -> ElementId {
    derived_autocomplete_id(
        control,
        OPTION_ID_TAG,
        item.stable_id()
            .map_or(source_index as u64, ElementId::as_u64),
    )
}

fn derived_autocomplete_id(parent: ElementId, tag: u64, value: u64) -> ElementId {
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

fn bounded_value(value: Arc<str>) -> Arc<str> {
    if value.len() <= MAX_AUTOCOMPLETE_VALUE_BYTES {
        return value;
    }
    let mut end = MAX_AUTOCOMPLETE_VALUE_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    Arc::from(&value[..end])
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
    use crate::{
        Application, Color, IntoElement, MouseDownEvent, WindowOptions, button,
        combobox_key_bindings, text, text_input,
    };

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

    fn popover_root(state: AutocompleteListState) -> Element {
        div()
            .bg(Color::BLACK)
            .child(text(format!("{} results", state.result_count)))
    }

    fn option_row(item: &PickerItem<&'static str>, state: AutocompleteOptionState) -> Element {
        text(item.label().clone())
            .opacity(if state.active { 1.0 } else { 0.8 })
            .into_element()
    }

    struct AutocompleteOwner {
        autocomplete: AutocompleteState<&'static str>,
        values: Vec<Arc<str>>,
        selected: Vec<&'static str>,
    }

    impl AutocompleteOwner {
        fn new(behavior: AutocompleteSelectionBehavior) -> Self {
            let mut autocomplete = AutocompleteState::new(options())
                .unwrap()
                .with_layout(AutocompletePopoverLayout::new(220.0, 32.0).max_visible_rows(2));
            autocomplete.set_selection_behavior(behavior);
            Self {
                autocomplete,
                values: Vec::new(),
                selected: Vec::new(),
            }
        }

        fn autocomplete(view: &mut Self) -> &mut AutocompleteState<&'static str> {
            &mut view.autocomplete
        }
    }

    impl View for AutocompleteOwner {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl crate::IntoElement {
            let input = text_input(self.autocomplete.value().clone())
                .w(220.0)
                .h(36.0);
            let autocomplete = self.autocomplete.element(
                cx,
                "fruit",
                "Fruit",
                Self::autocomplete,
                input,
                popover_root,
                option_row,
                |view, value, _cx| view.values.push(value),
                |view, value, _cx| view.selected.push(value),
            );
            div().children([
                autocomplete,
                button().id("after-autocomplete").child("After"),
            ])
        }
    }

    #[test]
    fn with_value_seeds_the_query_and_stays_bounded_before_a_context_exists() {
        let state = AutocompleteState::new(options()).unwrap().with_value("ap");
        assert_eq!(state.value().as_ref(), "ap");
        assert_eq!(state.picker_query().as_ref(), "ap");
        assert_eq!(state.result_count(), 2);

        let long = "é".repeat(MAX_AUTOCOMPLETE_VALUE_BYTES);
        let state = AutocompleteState::new(options()).unwrap().with_value(long);
        let retained = state.value();
        assert!(retained.len() <= MAX_AUTOCOMPLETE_VALUE_BYTES);
        assert!(retained.is_char_boundary(retained.len()));
    }

    #[test]
    fn free_form_value_survives_escape_and_unmatched_return_propagates() {
        let (mut cx, owner) = Application::new()
            .bind_keys(combobox_key_bindings())
            .into_test_context(
                WindowOptions::default(),
                AutocompleteOwner::new(AutocompleteSelectionBehavior::CompleteInput),
            )
            .unwrap();
        let window = owner.window_handle();
        cx.focus(window, "fruit").unwrap();
        cx.simulate_input(window, "custom value").unwrap();
        assert!(cx.read(owner, |view| view.autocomplete.is_open()).unwrap());
        assert_eq!(
            cx.read(owner, |view| view.autocomplete.result_count())
                .unwrap(),
            0
        );
        cx.simulate_keystrokes(window, "escape").unwrap();
        assert_eq!(
            cx.read(owner, |view| view.autocomplete.value().clone())
                .unwrap(),
            Arc::from("custom value")
        );
        assert!(!cx.read(owner, |view| view.autocomplete.is_open()).unwrap());
        assert_eq!(cx.focused(window).unwrap(), Some("fruit".into()));
        assert!(!cx.dispatch_action(window, ComboboxConfirm).unwrap());
        assert!(cx.read(owner, |view| view.selected.is_empty()).unwrap());

        cx.click(window, "fruit").unwrap();
        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(
            cx.focused(window).unwrap(),
            Some("after-autocomplete".into())
        );
        assert_eq!(
            cx.read(owner, |view| view.autocomplete.value().clone())
                .unwrap(),
            Arc::from("custom value")
        );
    }

    #[test]
    fn keyboard_commit_completes_by_default_and_dismiss_only_preserves_text() {
        for (behavior, expected_value) in [
            (
                AutocompleteSelectionBehavior::CompleteInput,
                Arc::<str>::from("Apple"),
            ),
            (
                AutocompleteSelectionBehavior::DismissOnly,
                Arc::<str>::from("ap"),
            ),
        ] {
            let (mut cx, owner) = Application::new()
                .bind_keys(combobox_key_bindings())
                .into_test_context(WindowOptions::default(), AutocompleteOwner::new(behavior))
                .unwrap();
            let window = owner.window_handle();
            cx.focus(window, "fruit").unwrap();
            cx.simulate_input(window, "ap").unwrap();
            cx.simulate_keystrokes(window, "enter").unwrap();

            assert_eq!(
                cx.read(owner, |view| view.autocomplete.value().clone())
                    .unwrap(),
                expected_value
            );
            assert_eq!(
                cx.read(owner, |view| view.selected.clone()).unwrap(),
                vec!["apple"]
            );
            assert!(!cx.read(owner, |view| view.autocomplete.is_open()).unwrap());
            assert_eq!(cx.focused(window).unwrap(), Some("fruit".into()));
        }
    }

    #[test]
    fn never_key_child_row_click_commits_without_moving_owner_input_focus() {
        let (mut cx, owner) = Application::new()
            .bind_keys(combobox_key_bindings())
            .into_test_context(
                WindowOptions::default(),
                AutocompleteOwner::new(AutocompleteSelectionBehavior::CompleteInput),
            )
            .unwrap();
        let window = owner.window_handle();
        cx.focus(window, "fruit").unwrap();
        cx.click(window, "fruit").unwrap();
        let (popover, row) = cx
            .read(owner, |view| {
                (
                    view.autocomplete.popover_window().unwrap(),
                    view.autocomplete.option_id_for_source("fruit", 0).unwrap(),
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
            cx.read(owner, |view| view.selected.clone()).unwrap(),
            vec!["apple"]
        );
        assert_eq!(
            cx.read(owner, |view| view.autocomplete.popover_window())
                .unwrap(),
            None
        );
    }

    #[test]
    fn owner_outside_press_closes_without_rewriting_free_form_text() {
        let (mut cx, owner) = Application::new()
            .bind_keys(combobox_key_bindings())
            .into_test_context(
                WindowOptions::default(),
                AutocompleteOwner::new(AutocompleteSelectionBehavior::CompleteInput),
            )
            .unwrap();
        let window = owner.window_handle();
        cx.focus(window, "fruit").unwrap();
        cx.simulate_input(window, "custom").unwrap();
        let popover = cx
            .read(owner, |view| view.autocomplete.popover_window().unwrap())
            .unwrap();
        assert!(
            !cx.simulate_mouse_down(window, "after-autocomplete", MouseDownEvent::default())
                .unwrap()
        );
        assert!(!cx.is_window_open(popover));
        assert_eq!(
            cx.read(owner, |view| view.autocomplete.value().clone())
                .unwrap(),
            Arc::from("custom")
        );
    }

    #[test]
    fn open_source_replacement_updates_the_same_child_and_rejects_invalid_input() {
        let (mut cx, owner) = Application::new()
            .bind_keys(combobox_key_bindings())
            .into_test_context(
                WindowOptions::default(),
                AutocompleteOwner::new(AutocompleteSelectionBehavior::CompleteInput),
            )
            .unwrap();
        let window = owner.window_handle();
        cx.focus(window, "fruit").unwrap();
        cx.click(window, "fruit").unwrap();
        let popover = cx
            .read(owner, |view| view.autocomplete.popover_window().unwrap())
            .unwrap();
        cx.update(owner, |view, cx| {
            view.autocomplete
                .set_items(
                    [
                        PickerItem::new("Cherry", "cherry").id("cherry"),
                        PickerItem::new("Citrus", "citrus").id("citrus"),
                    ],
                    cx,
                )
                .unwrap();
            cx.invalidate();
        })
        .unwrap();
        assert_eq!(
            cx.read(owner, |view| view.autocomplete.popover_window())
                .unwrap(),
            Some(popover)
        );
        let row = cx
            .read(owner, |view| {
                view.autocomplete.option_id_for_source("fruit", 0).unwrap()
            })
            .unwrap();
        cx.click(popover, row).unwrap();
        assert_eq!(
            cx.read(owner, |view| view.selected.clone()).unwrap(),
            vec!["cherry"]
        );

        let mut state = AutocompleteState::new(options()).unwrap();
        let mut event = EventContext::default();
        let error = state
            .set_items(
                [
                    PickerItem::new("One", "one").id("duplicate"),
                    PickerItem::new("Two", "two").id("duplicate"),
                ],
                &mut event,
            )
            .unwrap_err();
        assert_eq!(
            error,
            PickerError::DuplicateId {
                id: "duplicate".into()
            }
        );
        assert_eq!(state.items().len(), 4);
    }

    #[test]
    fn async_filter_none_value_bounds_and_child_policy_are_exact() {
        let mut state = AutocompleteState::new(options())
            .unwrap()
            .with_filter_mode(PickerFilterMode::None);
        let mut cx = EventContext::default();
        assert!(state.set_value("not locally matched", &mut cx));
        assert_eq!(state.result_count(), options().len());

        let oversized = Arc::<str>::from("é".repeat(MAX_AUTOCOMPLETE_VALUE_BYTES));
        assert!(state.set_value(oversized, &mut cx));
        assert!(state.value().len() <= MAX_AUTOCOMPLETE_VALUE_BYTES);
        assert!(state.value().is_char_boundary(state.value().len()));

        let options = crate::SystemPopover::new(240.0, 120.0)
            .grab(false)
            .accepts_key_focus(false)
            .window_options("Autocomplete");
        let popover = options.popover.expect("system popover options");
        assert!(!options.focus);
        assert!(!popover.grab);
        assert!(!popover.accepts_key_focus);
    }

    #[test]
    fn large_sources_mount_only_visible_child_rows_and_settled_windows_sleep() {
        let items = (0..20_000).map(|index| PickerItem::new(format!("Item {index}"), index));
        let mut state = AutocompleteState::new(items)
            .unwrap()
            .with_layout(AutocompletePopoverLayout::new(240.0, 30.0).max_visible_rows(5));
        state
            .picker
            .set_result_viewport_height(state.layout.popover_height());
        let snapshot = Entity::new(state.snapshot());
        let popover = AutocompletePopoverView::new(
            "large-autocomplete".into(),
            snapshot,
            state.layout,
            AutocompleteRenderers {
                popover_root: |_state: AutocompleteListState| div(),
                render_option: |_item: &PickerItem<usize>, _state: AutocompleteOptionState| div(),
            },
        );
        assert!(popover.list.visible_rows().len() <= 7);

        let (mut cx, owner) = Application::new()
            .bind_keys(combobox_key_bindings())
            .into_test_context(
                WindowOptions::default(),
                AutocompleteOwner::new(AutocompleteSelectionBehavior::CompleteInput),
            )
            .unwrap();
        let window = owner.window_handle();
        cx.focus(window, "fruit").unwrap();
        cx.click(window, "fruit").unwrap();
        let child = cx
            .read(owner, |view| view.autocomplete.popover_window().unwrap())
            .unwrap();
        let owner_renders = cx.render_count(window).unwrap();
        let child_renders = cx.render_count(child).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), owner_renders);
        assert_eq!(cx.render_count(child).unwrap(), child_renders);
    }
}
