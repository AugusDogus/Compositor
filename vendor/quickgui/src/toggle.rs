use std::sync::Arc;

use crate::{
    AccessibilityOrientation, AccessibilityRole, Element, ElementId, FocusHandle, KeyBinding,
    StateAccessor, ToggleState, ViewContext, div,
};

/// Maximum items retained in one toggle group.
///
/// Pressed values are retained inline so the state stays copyable and allocation-free.
pub const MAX_TOGGLE_GROUP_ITEMS: usize = 32;

const TOGGLE_GROUP_HORIZONTAL_KEY_CONTEXT: &str = "ToggleGroupHorizontal";
const TOGGLE_GROUP_VERTICAL_KEY_CONTEXT: &str = "ToggleGroupVertical";
const TOGGLE_GROUP_ITEM_ID_TAG: u64 = 0x3d5b_90ac_71e6_248f;

/// Move toggle-group focus to the next enabled item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToggleGroupNext;
/// Move toggle-group focus to the previous enabled item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToggleGroupPrevious;
/// Move toggle-group focus to the first enabled item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToggleGroupFirst;
/// Move toggle-group focus to the final enabled item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToggleGroupLast;

/// Contextual bindings used by [`ToggleGroupEntry::key_with`].
pub fn toggle_group_key_bindings() -> [KeyBinding; 8] {
    [
        KeyBinding::new(
            "right",
            ToggleGroupNext,
            Some(TOGGLE_GROUP_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "left",
            ToggleGroupPrevious,
            Some(TOGGLE_GROUP_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "home",
            ToggleGroupFirst,
            Some(TOGGLE_GROUP_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "end",
            ToggleGroupLast,
            Some(TOGGLE_GROUP_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "down",
            ToggleGroupNext,
            Some(TOGGLE_GROUP_VERTICAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "up",
            ToggleGroupPrevious,
            Some(TOGGLE_GROUP_VERTICAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "home",
            ToggleGroupFirst,
            Some(TOGGLE_GROUP_VERTICAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "end",
            ToggleGroupLast,
            Some(TOGGLE_GROUP_VERTICAL_KEY_CONTEXT),
        ),
    ]
}

/// Copyable declaration for one unstyled toggle button.
///
/// A toggle is a button that stays pressed, not a checkbox. Assistive technology reads it as a
/// button with a pressed state, so it belongs to commands such as *Bold* rather than to form data.
/// The application owns the icon, label, colors, and pressed styling.
///
/// The descriptor retains no allocation, task, timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a Toggle descriptor has no effect until its root part is mounted"]
pub struct Toggle {
    pressed: bool,
}

impl Toggle {
    pub const fn new(pressed: bool) -> Self {
        Self { pressed }
    }

    pub const fn is_pressed(self) -> bool {
        self.pressed
    }

    /// Decorate an application-owned root without adding layout or appearance.
    pub fn root_with(self, root: Element) -> Element {
        root.accessibility_role(AccessibilityRole::ToggleButton)
            .toggle_state(if self.pressed {
                ToggleState::On
            } else {
                ToggleState::Off
            })
            .clickable()
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Hide an application-owned decorative indicator from the accessible name.
    pub fn indicator_with(self, indicator: Element) -> Element {
        indicator.accessibility_hidden(true)
    }
    /// Create the unstyled indicator part. Use [`Self::indicator_with`] to supply an existing element.
    pub fn indicator(self) -> Element {
        self.indicator_with(crate::div())
    }
}

/// Create an unstyled controlled toggle-button root.
///
/// This shorthand is equivalent to `Toggle::new(pressed).root_with(div())`.
pub fn toggle(pressed: bool) -> Element {
    Toggle::new(pressed).root_with(div())
}

/// How many items of one toggle group can be pressed at once.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ToggleGroupSelection {
    /// At most one pressed item. Pressing the pressed item releases it.
    #[default]
    Single,
    /// Any number of pressed items.
    Multiple,
}

/// One caller-declared toggle-group item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToggleGroupItem {
    value: ElementId,
    disabled: bool,
}

impl ToggleGroupItem {
    pub fn new(value: impl Into<ElementId>) -> Self {
        Self {
            value: value.into(),
            disabled: false,
        }
    }

    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub const fn value(self) -> ElementId {
        self.value
    }

    pub const fn is_disabled(self) -> bool {
        self.disabled
    }
}

/// Controlled, allocation-free pressed values and roving focus for one toggle group.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToggleGroupState {
    selection: ToggleGroupSelection,
    pressed: [ElementId; MAX_TOGGLE_GROUP_ITEMS],
    count: usize,
    active: Option<ElementId>,
}

impl Default for ToggleGroupState {
    fn default() -> Self {
        Self::single()
    }
}

impl ToggleGroupState {
    /// Create a group where at most one item is pressed.
    pub const fn single() -> Self {
        Self {
            selection: ToggleGroupSelection::Single,
            pressed: [ElementId::new(0); MAX_TOGGLE_GROUP_ITEMS],
            count: 0,
            active: None,
        }
    }

    /// Create a group where any number of items can be pressed.
    pub const fn multiple() -> Self {
        Self {
            selection: ToggleGroupSelection::Multiple,
            pressed: [ElementId::new(0); MAX_TOGGLE_GROUP_ITEMS],
            count: 0,
            active: None,
        }
    }

    pub const fn selection(&self) -> ToggleGroupSelection {
        self.selection
    }

    pub fn pressed_values(&self) -> &[ElementId] {
        &self.pressed[..self.count]
    }

    pub fn is_pressed(&self, value: impl Into<ElementId>) -> bool {
        let value = value.into();
        self.pressed[..self.count].contains(&value)
    }

    /// The single pressed value, or the first pressed value of a multiple-selection group.
    pub fn pressed_value(&self) -> Option<ElementId> {
        (self.count > 0).then(|| self.pressed[0])
    }

    /// Press one value, returning whether the pressed set changed.
    ///
    /// A single-selection group replaces its existing value. A multiple-selection group at
    /// [`MAX_TOGGLE_GROUP_ITEMS`] rejects further values instead of growing.
    pub fn press(&mut self, value: impl Into<ElementId>) -> bool {
        let value = value.into();
        if self.is_pressed(value) {
            return false;
        }
        match self.selection {
            ToggleGroupSelection::Single => {
                self.pressed[0] = value;
                self.count = 1;
                true
            }
            ToggleGroupSelection::Multiple => {
                if self.count == MAX_TOGGLE_GROUP_ITEMS {
                    return false;
                }
                self.pressed[self.count] = value;
                self.count += 1;
                true
            }
        }
    }

    /// Release one value, returning whether the pressed set changed.
    pub fn release(&mut self, value: impl Into<ElementId>) -> bool {
        let value = value.into();
        let Some(position) = self.pressed[..self.count]
            .iter()
            .position(|entry| *entry == value)
        else {
            return false;
        };
        for index in position..self.count - 1 {
            self.pressed[index] = self.pressed[index + 1];
        }
        self.count -= 1;
        true
    }

    /// Toggle one value, returning whether the pressed set changed.
    pub fn toggle(&mut self, value: impl Into<ElementId>) -> bool {
        let value = value.into();
        if self.is_pressed(value) {
            self.release(value)
        } else {
            self.press(value)
        }
    }

    /// Release every value, returning whether anything changed.
    pub fn clear(&mut self) -> bool {
        if self.count == 0 {
            return false;
        }
        self.count = 0;
        true
    }

    pub const fn active(&self) -> Option<ElementId> {
        self.active
    }

    /// Replace the roving tab stop, returning whether it changed.
    pub fn set_active(&mut self, active: Option<ElementId>) -> bool {
        if self.active == active {
            false
        } else {
            self.active = active;
            true
        }
    }

    pub fn focus(&mut self, value: impl Into<ElementId>) -> bool {
        self.set_active(Some(value.into()))
    }
}

/// A controlled, unstyled toggle-group descriptor.
///
/// The application owns item content, layout, and pressed styling. QuickGUI supplies the group
/// role and orientation, pressed-button semantics per item, single or multiple selection, one
/// roving Tab stop, bounded arrow/Home/End navigation, and disabled-item skipping.
#[derive(Clone, Copy, Debug)]
#[must_use = "a ToggleGroup descriptor has no effect until its parts are mounted"]
pub struct ToggleGroup<'a> {
    root_id: ElementId,
    items: &'a [ToggleGroupItem],
    state: &'a ToggleGroupState,
    orientation: AccessibilityOrientation,
    loop_focus: bool,
}

impl<'a> ToggleGroup<'a> {
    /// Create a toggle group over the caller's ordered items.
    ///
    /// # Panics
    ///
    /// Panics with more than [`MAX_TOGGLE_GROUP_ITEMS`] items or duplicate item values.
    pub fn new(
        root_id: impl Into<ElementId>,
        state: &'a ToggleGroupState,
        items: &'a [ToggleGroupItem],
    ) -> Self {
        assert_toggle_group_items(items);
        Self {
            root_id: root_id.into(),
            items,
            state,
            orientation: AccessibilityOrientation::Horizontal,
            loop_focus: true,
        }
    }

    pub const fn vertical(mut self) -> Self {
        self.orientation = AccessibilityOrientation::Vertical;
        self
    }

    pub const fn loop_focus(mut self, loop_focus: bool) -> Self {
        self.loop_focus = loop_focus;
        self
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub const fn items(self) -> &'a [ToggleGroupItem] {
        self.items
    }

    pub fn item_id(self, value: impl Into<ElementId>) -> ElementId {
        derived_toggle_id(self.root_id, value.into())
    }

    /// The item that currently owns the group's single Tab stop.
    ///
    /// The controlled active value wins, then the first pressed enabled item, then the first
    /// enabled item.
    pub fn roving_value(self) -> Option<ElementId> {
        let enabled = |value: ElementId| {
            self.items
                .iter()
                .any(|item| item.value == value && !item.disabled)
        };
        self.state
            .active
            .filter(|active| enabled(*active))
            .or_else(|| {
                self.items
                    .iter()
                    .find(|item| !item.disabled && self.state.is_pressed(item.value))
                    .map(|item| item.value)
            })
            .or_else(|| {
                self.items
                    .iter()
                    .find(|item| !item.disabled)
                    .map(|item| item.value)
            })
    }

    /// Decorate an application-owned group root without adding layout or appearance.
    pub fn root_with(self, root: Element) -> Element {
        root.id(self.root_id)
            .accessibility_role(AccessibilityRole::Group)
            .accessibility_orientation(self.orientation)
            .app_region_no_drag()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Describe one declared item.
    pub fn item(self, value: impl Into<ElementId>) -> Option<ToggleGroupEntry<'a>> {
        let value = value.into();
        let item = self
            .items
            .iter()
            .copied()
            .find(|item| item.value == value)?;
        Some(ToggleGroupEntry {
            group: self,
            item,
            pressed: self.state.is_pressed(value),
            roving: self.roving_value() == Some(value),
        })
    }
}

/// A copyable declaration for one mounted toggle-group item.
#[derive(Clone, Copy, Debug)]
#[must_use = "a ToggleGroupEntry descriptor has no effect until its part is mounted"]
pub struct ToggleGroupEntry<'a> {
    group: ToggleGroup<'a>,
    item: ToggleGroupItem,
    pressed: bool,
    roving: bool,
}

impl<'a> ToggleGroupEntry<'a> {
    pub const fn value(self) -> ElementId {
        self.item.value
    }

    pub const fn is_pressed(self) -> bool {
        self.pressed
    }

    pub const fn is_disabled(self) -> bool {
        self.item.disabled
    }

    pub const fn is_roving_stop(self) -> bool {
        self.roving
    }

    pub fn item_id(self) -> ElementId {
        self.group.item_id(self.item.value)
    }

    /// Decorate an application-owned item as a pressed-state button.
    pub fn item_with(self, item: Element) -> Element {
        let disabled = self.item.disabled || item.accessibility.disabled;
        Toggle::new(self.pressed)
            .root_with(item)
            .id(self.item_id())
            .focusable()
            .tab_index(if self.roving { 0 } else { -1 })
            .key_context(match self.group.orientation {
                AccessibilityOrientation::Horizontal => TOGGLE_GROUP_HORIZONTAL_KEY_CONTEXT,
                AccessibilityOrientation::Vertical => TOGGLE_GROUP_VERTICAL_KEY_CONTEXT,
            })
            .disabled(disabled)
    }
    /// Create the unstyled item part. Use [`Self::item_with`] to supply an existing element.
    pub fn item(self) -> Element {
        self.item_with(crate::div())
    }

    /// Attach QuickGUI's typed toggle-group navigation to this item.
    ///
    /// Install [`toggle_group_key_bindings`] once on the application keymap.
    pub fn key_with<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        item: Element,
        access: fn(&mut V) -> &mut ToggleGroupState,
    ) -> Element {
        self.key_with_accessor(cx, item, StateAccessor::from(access))
    }
    /// Create the unstyled key part. Use [`Self::key_with`] to supply an existing element.
    pub fn key<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut ToggleGroupState,
    ) -> Element {
        self.key_with(cx, crate::div(), access)
    }

    /// Attach the typed toggle-group navigation against a per-instance state accessor.
    ///
    /// A host that renders many declared groups through one view passes an accessor that captures
    /// which [`ToggleGroupState`] this item belongs to.
    pub fn key_with_accessor<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        item: Element,
        access_source: StateAccessor<V, ToggleGroupState>,
    ) -> Element {
        let id = self.item_id();
        let root = self.group.root_id;
        let value = self.item.value;
        let loop_focus = self.group.loop_focus;
        let items: Arc<[ToggleGroupItem]> = Arc::from(self.group.items);

        let next_items = items.clone();
        let access = access_source.clone();
        let next = cx.action_listener(id, move |view, _: &ToggleGroupNext, cx| {
            move_focus(
                view,
                cx,
                &access,
                root,
                neighbor_value(&next_items, value, true, loop_focus),
            );
        });
        let previous_items = items.clone();
        let access = access_source.clone();
        let previous = cx.action_listener(id, move |view, _: &ToggleGroupPrevious, cx| {
            move_focus(
                view,
                cx,
                &access,
                root,
                neighbor_value(&previous_items, value, false, loop_focus),
            );
        });
        let first_items = items.clone();
        let access = access_source.clone();
        let first = cx.action_listener(id, move |view, _: &ToggleGroupFirst, cx| {
            move_focus(view, cx, &access, root, edge_value(&first_items, false));
        });
        let access = access_source;
        let last = cx.action_listener(id, move |view, _: &ToggleGroupLast, cx| {
            move_focus(view, cx, &access, root, edge_value(&items, true));
        });

        item.on_action(next)
            .on_action(previous)
            .on_action(first)
            .on_action(last)
    }
}

fn move_focus<V: 'static>(
    view: &mut V,
    cx: &mut crate::EventContext,
    access: &StateAccessor<V, ToggleGroupState>,
    root: ElementId,
    target: Option<ElementId>,
) {
    let Some(target) = target else {
        return;
    };
    let changed = access.get(view).focus(target);
    cx.focus(FocusHandle::new(derived_toggle_id(root, target)));
    if changed {
        cx.invalidate();
    }
}

fn neighbor_value(
    items: &[ToggleGroupItem],
    from: ElementId,
    forward: bool,
    loop_focus: bool,
) -> Option<ElementId> {
    let position = items.iter().position(|item| item.value == from)?;
    let count = items.len();
    let mut index = position;
    for _ in 0..count {
        index = if forward {
            if index + 1 == count {
                if !loop_focus {
                    return None;
                }
                0
            } else {
                index + 1
            }
        } else if index == 0 {
            if !loop_focus {
                return None;
            }
            count - 1
        } else {
            index - 1
        };
        if !items[index].disabled {
            return Some(items[index].value);
        }
    }
    None
}

fn edge_value(items: &[ToggleGroupItem], last: bool) -> Option<ElementId> {
    let mut enabled = items.iter().filter(|item| !item.disabled);
    if last {
        enabled.next_back().map(|item| item.value)
    } else {
        enabled.next().map(|item| item.value)
    }
}

fn assert_toggle_group_items(items: &[ToggleGroupItem]) {
    assert!(
        items.len() <= MAX_TOGGLE_GROUP_ITEMS,
        "a toggle group retains at most {MAX_TOGGLE_GROUP_ITEMS} items"
    );
    for (index, item) in items.iter().enumerate() {
        assert!(
            !items[..index]
                .iter()
                .any(|earlier| earlier.value == item.value),
            "toggle group item values must be unique"
        );
    }
}

fn derived_toggle_id(scope: ElementId, value: ElementId) -> ElementId {
    let mut hash = scope
        .as_u64()
        .rotate_left(7)
        .wrapping_add(value.as_u64().rotate_right(23))
        ^ TOGGLE_GROUP_ITEM_ID_TAG;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == u64::MAX || hash == scope.as_u64() || hash == value.as_u64() {
        hash ^= TOGGLE_GROUP_ITEM_ID_TAG.rotate_left(11);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppRegion, Application, Color, CursorStyle, IntoElement, UserSelect, View, WindowOptions,
        button, text,
    };

    fn items() -> [ToggleGroupItem; 4] {
        [
            ToggleGroupItem::new("left"),
            ToggleGroupItem::new("center").disabled(true),
            ToggleGroupItem::new("right"),
            ToggleGroupItem::new("justify"),
        ]
    }

    #[test]
    fn toggle_is_a_pressed_button_not_a_checkbox() {
        let pressed = Toggle::new(true);
        assert!(pressed.is_pressed());
        let root = pressed.root_with(div().px_2().bg(Color::rgb8(1, 2, 3)));
        assert_eq!(root.accessibility.role, AccessibilityRole::ToggleButton);
        assert_eq!(root.accessibility.toggled, Some(ToggleState::On));
        assert!(root.clickable);
        assert!(root.focusable);
        assert_eq!(root.cursor_style, Some(CursorStyle::Arrow));
        assert_eq!(root.app_region, Some(AppRegion::NoDrag));
        assert_eq!(root.user_select, UserSelect::None);
        assert_eq!(root.visual.background, Some(Color::rgb8(1, 2, 3)));

        let released = Toggle::new(false).root_with(div());
        assert_eq!(released.accessibility.toggled, Some(ToggleState::Off));
        assert!(pressed.indicator_with(div()).accessibility.hidden);

        let shorthand = toggle(true);
        assert_eq!(
            shorthand.accessibility.role,
            AccessibilityRole::ToggleButton
        );
        assert!(shorthand.children.is_empty());
        assert_eq!(shorthand.visual.background, None);
    }

    #[test]
    fn group_selection_and_roving_stop_are_bounded() {
        let mut single = ToggleGroupState::single();
        assert_eq!(single.selection(), ToggleGroupSelection::Single);
        assert!(single.pressed_values().is_empty());
        assert!(single.press("left"));
        assert!(!single.press("left"));
        assert!(single.is_pressed("left"));
        assert!(single.press("right"));
        assert_eq!(single.pressed_values(), &[ElementId::from("right")]);
        assert!(single.toggle("right"));
        assert!(single.pressed_values().is_empty());
        assert!(!single.release("right"));
        assert!(!single.clear());

        let mut multiple = ToggleGroupState::multiple();
        assert!(multiple.press("left"));
        assert!(multiple.press("right"));
        assert!(multiple.press("justify"));
        assert_eq!(multiple.pressed_values().len(), 3);
        assert!(multiple.release("right"));
        assert_eq!(
            multiple.pressed_values(),
            &[ElementId::from("left"), ElementId::from("justify")]
        );
        assert_eq!(multiple.pressed_value(), Some(ElementId::from("left")));
        assert!(multiple.clear());

        let mut full = ToggleGroupState::multiple();
        for index in 0..MAX_TOGGLE_GROUP_ITEMS {
            assert!(full.press(ElementId::new(index as u64 + 1)));
        }
        assert!(!full.press(ElementId::new(9_999)));
        assert_eq!(full.pressed_values().len(), MAX_TOGGLE_GROUP_ITEMS);

        let items = items();
        let mut state = ToggleGroupState::single();
        assert!(state.press("right"));
        let group = ToggleGroup::new("align", &state, &items);
        // With no active value the first pressed enabled item owns the Tab stop.
        assert_eq!(group.roving_value(), Some("right".into()));

        let mut focused = state;
        assert!(focused.focus("justify"));
        let group = ToggleGroup::new("align", &focused, &items);
        assert_eq!(group.roving_value(), Some("justify".into()));

        let mut disabled_active = state;
        assert!(disabled_active.focus("center"));
        assert_eq!(
            ToggleGroup::new("align", &disabled_active, &items).roving_value(),
            Some("right".into())
        );

        let empty = ToggleGroupState::single();
        assert_eq!(
            ToggleGroup::new("align", &empty, &items).roving_value(),
            Some("left".into())
        );
    }

    #[test]
    fn parts_add_exact_semantics_without_appearance() {
        let items = items();
        let mut state = ToggleGroupState::multiple();
        assert!(state.press("right"));
        let group = ToggleGroup::new("align", &state, &items);

        let root = group.root_with(div().gap_1().bg(Color::rgb8(4, 5, 6)));
        assert_eq!(root.explicit_id, Some("align".into()));
        assert_eq!(root.accessibility.role, AccessibilityRole::Group);
        assert_eq!(
            root.accessibility.orientation,
            Some(AccessibilityOrientation::Horizontal)
        );
        assert_eq!(root.visual.background, Some(Color::rgb8(4, 5, 6)));
        assert!(!root.focusable);

        let right = group.item("right").expect("right entry");
        assert!(right.is_pressed());
        assert!(right.is_roving_stop());
        let element = right.item_with(button().child("Right"));
        assert_eq!(element.explicit_id, Some(group.item_id("right")));
        assert_eq!(element.accessibility.role, AccessibilityRole::ToggleButton);
        assert_eq!(element.accessibility.toggled, Some(ToggleState::On));
        assert_eq!(element.tab_index, 0);

        let left = group.item("left").expect("left entry");
        assert!(!left.is_pressed());
        assert_eq!(left.item_with(div()).tab_index, -1);
        assert_eq!(
            left.item_with(div()).accessibility.toggled,
            Some(ToggleState::Off)
        );

        let center = group.item("center").expect("center entry");
        assert!(center.is_disabled());
        assert!(center.item_with(div()).accessibility.disabled);
        assert!(group.item("absent").is_none());

        let vertical = ToggleGroup::new("align", &state, &items).vertical();
        assert_eq!(
            vertical.root_with(div()).accessibility.orientation,
            Some(AccessibilityOrientation::Vertical)
        );

        let ids = [
            group.root_id(),
            group.item_id("left"),
            group.item_id("center"),
            group.item_id("right"),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert!(!ids[..index].contains(id));
        }
    }

    struct ToggleView {
        align: ToggleGroupState,
        marks: ToggleGroupState,
    }

    impl Default for ToggleView {
        fn default() -> Self {
            Self {
                align: ToggleGroupState::single(),
                marks: ToggleGroupState::multiple(),
            }
        }
    }

    impl ToggleView {
        fn align(view: &mut Self) -> &mut ToggleGroupState {
            &mut view.align
        }
    }

    impl View for ToggleView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let items = items();
            let group = ToggleGroup::new("align", &self.align, &items);
            let mut root = group.root_with(div());
            for item in items {
                let entry = group.item(item.value()).expect("declared entry");
                let value = item.value();
                let clicked = cx.listener(entry.item_id(), move |view: &mut Self, cx| {
                    view.align.toggle(value);
                    view.align.focus(value);
                    cx.invalidate();
                });
                root = root.child(entry.key_with(
                    cx,
                    entry.item_with(div().child(text("Item")).on_click(clicked)),
                    Self::align,
                ));
            }

            let bold = Toggle::new(self.marks.is_pressed("bold"));
            let bold_click = cx.listener("bold", |view: &mut Self, cx| {
                view.marks.toggle("bold");
                cx.invalidate();
            });

            div()
                .child(button().id("before").child("Before"))
                .child(root)
                .child(
                    bold.root_with(div().child(text("Bold")))
                        .id("bold")
                        .on_click(bold_click),
                )
        }
    }

    #[test]
    fn keyboard_selection_and_accessibility_paths_stay_deterministic() {
        let (mut cx, view) = Application::new()
            .bind_keys(toggle_group_key_bindings())
            .into_test_context(WindowOptions::default(), ToggleView::default())
            .unwrap();
        let window = view.window_handle();
        let items = items();
        let empty = ToggleGroupState::single();
        let group = ToggleGroup::new("align", &empty, &items);

        cx.simulate_keystrokes(window, "tab tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(group.item_id("left")));
        cx.simulate_keystrokes(window, "space").unwrap();
        assert!(cx.read(view, |view| view.align.is_pressed("left")).unwrap());
        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(group.item_id("right")));
        cx.simulate_keystrokes(window, "space").unwrap();
        assert!(!cx.read(view, |view| view.align.is_pressed("left")).unwrap());
        assert!(
            cx.read(view, |view| view.align.is_pressed("right"))
                .unwrap()
        );
        cx.simulate_keystrokes(window, "end").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(group.item_id("justify")));
        cx.simulate_keystrokes(window, "home").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(group.item_id("left")));
        cx.simulate_keystrokes(window, "left").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(group.item_id("justify")));
        assert!(cx.click(window, group.item_id("center")).is_err());

        cx.click(window, "bold").unwrap();
        assert!(cx.read(view, |view| view.marks.is_pressed("bold")).unwrap());
        cx.click(window, "bold").unwrap();
        assert!(!cx.read(view, |view| view.marks.is_pressed("bold")).unwrap());

        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("toggle accessibility node")
        };
        assert_eq!(node("align".into()).role(), accesskit::Role::Group);
        let right = node(group.item_id("right"));
        assert_eq!(right.role(), accesskit::Role::Button);
        assert_eq!(right.toggled(), Some(accesskit::Toggled::True));
        assert_eq!(
            node(group.item_id("left")).toggled(),
            Some(accesskit::Toggled::False)
        );
        assert!(node(group.item_id("center")).is_disabled());

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn bindings_are_contextual_and_complete() {
        let bindings = toggle_group_key_bindings();
        assert_eq!(bindings.len(), 8);
        assert!(bindings.iter().all(|binding| {
            binding.context_predicate().is_some_and(|context| {
                context
                    .depth_of(&[
                        crate::KeyContext::parse(TOGGLE_GROUP_HORIZONTAL_KEY_CONTEXT).unwrap(),
                    ])
                    .is_some()
                    || context
                        .depth_of(&[
                            crate::KeyContext::parse(TOGGLE_GROUP_VERTICAL_KEY_CONTEXT).unwrap()
                        ])
                        .is_some()
            })
        }));
    }

    #[test]
    #[should_panic(expected = "toggle group item values must be unique")]
    fn duplicate_item_values_are_rejected() {
        let items = [ToggleGroupItem::new("same"), ToggleGroupItem::new("same")];
        let _ = ToggleGroup::new("align", &ToggleGroupState::single(), &items);
    }
}
