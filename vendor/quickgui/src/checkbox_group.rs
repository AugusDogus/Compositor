use crate::{
    AccessibilityRole, Checkbox, ClickListener, Element, ElementId, EventContext, StateAccessor,
    ToggleState, ViewContext, div,
};

/// Maximum declared values one checkbox group retains.
///
/// Both the declared universe and the checked subset are bounded by this constant, so one group
/// keeps a fixed worst-case size no matter what application data drives it.
pub const MAX_CHECKBOX_GROUP_VALUES: usize = 256;

const CHECKBOX_GROUP_ITEM_ID_TAG: u64 = 0x4c8b_2f17_ae60_93d5;
const CHECKBOX_GROUP_PARENT_ID_TAG: u64 = 0xd207_66be_1f39_c84a;

/// Controlled, bounded value state for one checkbox group.
///
/// The state owns the declared universe of values, the checked subset, and the group's disabled
/// flag. The application owns every visual declaration, the labels, and the listener that reacts
/// to a change. Nothing here observes, schedules, or animates.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CheckboxGroupState {
    all_values: Vec<ElementId>,
    values: Vec<ElementId>,
    disabled: bool,
}

impl CheckboxGroupState {
    /// Declare the group's complete, ordered universe of values.
    ///
    /// # Panics
    ///
    /// Panics with more than [`MAX_CHECKBOX_GROUP_VALUES`] values or with a duplicate value,
    /// because either makes the parent checkbox's derived state ambiguous.
    pub fn new<I>(all_values: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<ElementId>,
    {
        let all_values: Vec<ElementId> = all_values.into_iter().map(Into::into).collect();
        assert!(
            all_values.len() <= MAX_CHECKBOX_GROUP_VALUES,
            "a checkbox group retains at most {MAX_CHECKBOX_GROUP_VALUES} values"
        );
        for (index, value) in all_values.iter().enumerate() {
            assert!(
                !all_values[..index].contains(value),
                "checkbox group values must be distinct"
            );
        }
        Self {
            all_values,
            values: Vec::new(),
            disabled: false,
        }
    }

    /// Declare the initially checked subset, ignoring values outside the declared universe.
    #[must_use]
    pub fn checked<I>(mut self, values: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<ElementId>,
    {
        self.values.clear();
        for value in values.into_iter().map(Into::into) {
            if self.all_values.contains(&value) && !self.values.contains(&value) {
                self.values.push(value);
            }
        }
        self
    }

    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// The declared universe, in the order the application gave.
    pub fn all_values(&self) -> &[ElementId] {
        &self.all_values
    }

    /// The checked subset, in the order the application gave for the universe.
    pub fn values(&self) -> &[ElementId] {
        &self.values
    }

    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub fn is_checked(&self, value: impl Into<ElementId>) -> bool {
        self.values.contains(&value.into())
    }

    /// Replace one value's checked state, returning whether the group changed.
    ///
    /// A disabled group, or a value outside the declared universe, changes nothing.
    pub fn set_checked(&mut self, value: impl Into<ElementId>, checked: bool) -> bool {
        if self.disabled {
            return false;
        }
        let value = value.into();
        if !self.all_values.contains(&value) {
            return false;
        }
        let position = self.values.iter().position(|candidate| *candidate == value);
        match (checked, position) {
            (true, None) => {
                if self.values.len() == MAX_CHECKBOX_GROUP_VALUES {
                    return false;
                }
                self.values.push(value);
                self.sort_by_declaration();
                true
            }
            (false, Some(position)) => {
                self.values.remove(position);
                true
            }
            _ => false,
        }
    }

    /// Flip one value, returning whether the group changed.
    pub fn toggle(&mut self, value: impl Into<ElementId>) -> bool {
        let value = value.into();
        let checked = self.is_checked(value);
        self.set_checked(value, !checked)
    }

    /// The parent checkbox's derived state: on when every value is checked, mixed when some are.
    pub fn parent_state(&self) -> ToggleState {
        if self.all_values.is_empty() || self.values.is_empty() {
            ToggleState::Off
        } else if self.values.len() == self.all_values.len() {
            ToggleState::On
        } else {
            ToggleState::Mixed
        }
    }

    /// Apply the parent checkbox: check everything unless everything is already checked.
    ///
    /// Returns whether the group changed.
    pub fn toggle_parent(&mut self) -> bool {
        if self.disabled {
            return false;
        }
        if self.parent_state().is_on() {
            if self.values.is_empty() {
                return false;
            }
            self.values.clear();
            return true;
        }
        if self.values.len() == self.all_values.len() {
            return false;
        }
        self.values = self.all_values.clone();
        true
    }

    /// Uncheck every value, returning whether the group changed.
    pub fn clear(&mut self) -> bool {
        if self.disabled || self.values.is_empty() {
            return false;
        }
        self.values.clear();
        true
    }

    fn sort_by_declaration(&mut self) {
        let order = &self.all_values;
        self.values.sort_by_key(|value| {
            order
                .iter()
                .position(|candidate| candidate == value)
                .unwrap_or(usize::MAX)
        });
    }
}

/// A controlled, unstyled checkbox-group descriptor.
///
/// The application owns the layout, labels, indicators, and colors. QuickGUI supplies the Group
/// role, stable per-value part identities, exact checked/mixed semantics reused from [`Checkbox`],
/// explicit group-disabled propagation, and a parent checkbox whose state is derived from the
/// children rather than retained separately.
///
/// The descriptor borrows the caller's state and retains no registry, task, timer, observer, or
/// idle scheduler source.
#[derive(Clone, Copy, Debug)]
#[must_use = "a CheckboxGroup descriptor has no effect until its parts are mounted"]
pub struct CheckboxGroup<'a> {
    root_id: ElementId,
    state: &'a CheckboxGroupState,
}

impl<'a> CheckboxGroup<'a> {
    pub fn new(root_id: impl Into<ElementId>, state: &'a CheckboxGroupState) -> Self {
        Self {
            root_id: root_id.into(),
            state,
        }
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub const fn state(self) -> &'a CheckboxGroupState {
        self.state
    }

    pub fn checkbox_id(self, value: impl Into<ElementId>) -> ElementId {
        derived_group_id(self.root_id, CHECKBOX_GROUP_ITEM_ID_TAG, value.into())
    }

    pub fn parent_id(self) -> ElementId {
        derived_group_id(
            self.root_id,
            CHECKBOX_GROUP_PARENT_ID_TAG,
            ElementId::new(0),
        )
    }

    /// Decorate an application-owned group root without adding layout or appearance.
    ///
    /// Pair it with a [`crate::Field`] label, or with
    /// [`crate::Element::accessibility_labelled_by`], to name the group.
    pub fn root_with(self, root: Element) -> Element {
        root.id(self.root_id)
            .accessibility_role(AccessibilityRole::Group)
            .app_region_no_drag()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate one application-owned checkbox for `value`.
    ///
    /// The part reuses the existing [`Checkbox`] descriptor, so the checkbox role, exact
    /// on/off/mixed semantics, click behavior, and desktop pointer contract are identical to a
    /// standalone checkbox. A disabled group disables every item.
    pub fn checkbox_with(self, value: impl Into<ElementId>, checkbox: Element) -> Element {
        let value = value.into();
        let disabled = self.state.disabled || checkbox.accessibility.disabled;
        Checkbox::new(self.state.is_checked(value))
            .root_with(checkbox)
            .id(self.checkbox_id(value))
            .disabled(disabled)
    }
    /// Create the unstyled checkbox part. Use [`Self::checkbox_with`] to supply an existing element.
    pub fn checkbox(self, value: impl Into<ElementId>) -> Element {
        self.checkbox_with(value, crate::div())
    }

    /// Hide an application-owned indicator inside one item from the accessible name.
    pub fn indicator_with(self, indicator: Element) -> Element {
        indicator.accessibility_hidden(true)
    }
    /// Create the unstyled indicator part. Use [`Self::indicator_with`] to supply an existing element.
    pub fn indicator(self) -> Element {
        self.indicator_with(crate::div())
    }

    /// Decorate the application-owned parent checkbox.
    ///
    /// Its checked state is derived: on when every declared value is checked, mixed when only
    /// some are, and off when none are. QuickGUI retains no separate parent value, so the parent
    /// can never disagree with its children.
    pub fn parent_with(self, parent: Element) -> Element {
        let disabled = self.state.disabled || parent.accessibility.disabled;
        Checkbox::new(self.state.parent_state())
            .root_with(parent)
            .id(self.parent_id())
            .disabled(disabled)
    }
    /// Create the unstyled parent part. Use [`Self::parent_with`] to supply an existing element.
    pub fn parent(self) -> Element {
        self.parent_with(crate::div())
    }

    /// Build the click behavior for one item, reporting the whole new value set.
    ///
    /// Attach the returned handle with [`crate::Element::on_click`].
    pub fn on_checkbox_click<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        value: impl Into<ElementId>,
        access: fn(&mut V) -> &mut CheckboxGroupState,
        on_value_change: Change,
    ) -> ClickListener<V>
    where
        Change: Fn(&mut V, &[ElementId], &mut EventContext) + 'static,
    {
        self.on_checkbox_click_with(cx, value, StateAccessor::from(access), on_value_change)
    }

    /// Build one item's click behavior against a per-instance state accessor.
    pub fn on_checkbox_click_with<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        value: impl Into<ElementId>,
        access: StateAccessor<V, CheckboxGroupState>,
        on_value_change: Change,
    ) -> ClickListener<V>
    where
        Change: Fn(&mut V, &[ElementId], &mut EventContext) + 'static,
    {
        let value = value.into();
        cx.listener(self.checkbox_id(value), move |view, cx| {
            if access.get(view).toggle(value) {
                let values = access.get(view).values().to_vec();
                on_value_change(view, &values, cx);
                cx.invalidate();
            }
        })
    }

    /// Build the parent checkbox's click behavior, reporting the whole new value set.
    pub fn on_parent_click<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut CheckboxGroupState,
        on_value_change: Change,
    ) -> ClickListener<V>
    where
        Change: Fn(&mut V, &[ElementId], &mut EventContext) + 'static,
    {
        self.on_parent_click_with(cx, StateAccessor::from(access), on_value_change)
    }

    /// Build the parent checkbox's click behavior against a per-instance state accessor.
    pub fn on_parent_click_with<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, CheckboxGroupState>,
        on_value_change: Change,
    ) -> ClickListener<V>
    where
        Change: Fn(&mut V, &[ElementId], &mut EventContext) + 'static,
    {
        cx.listener(self.parent_id(), move |view, cx| {
            if access.get(view).toggle_parent() {
                let values = access.get(view).values().to_vec();
                on_value_change(view, &values, cx);
                cx.invalidate();
            }
        })
    }
}

/// Create an unstyled semantic checkbox-group root.
///
/// This shorthand is equivalent to `CheckboxGroup::new(id, state).root_with(div())`.
pub fn checkbox_group(id: impl Into<ElementId>, state: &CheckboxGroupState) -> Element {
    CheckboxGroup::new(id, state).root_with(div())
}

fn derived_group_id(scope: ElementId, tag: u64, value: ElementId) -> ElementId {
    let mut hash = scope
        .as_u64()
        .rotate_left(29)
        .wrapping_add(value.as_u64().rotate_right(13))
        ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == u64::MAX || hash == scope.as_u64() {
        hash ^= tag.rotate_left(7);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, IntoElement, TestAppContext, View, text};

    fn ids(values: &[&str]) -> Vec<ElementId> {
        values.iter().map(|value| (*value).into()).collect()
    }

    #[test]
    fn bounded_values_derive_the_parent_state() {
        let mut state = CheckboxGroupState::new(["red", "green", "blue"]);
        assert_eq!(state.all_values(), ids(&["red", "green", "blue"]));
        assert_eq!(state.values(), Vec::<ElementId>::new());
        assert_eq!(state.parent_state(), ToggleState::Off);

        assert!(state.set_checked("green", true));
        assert!(!state.set_checked("green", true));
        assert_eq!(state.parent_state(), ToggleState::Mixed);
        assert!(state.toggle("red"));
        // Checked values keep the declared order regardless of click order.
        assert_eq!(state.values(), ids(&["red", "green"]));

        assert!(state.set_checked("blue", true));
        assert_eq!(state.parent_state(), ToggleState::On);
        assert!(state.toggle_parent());
        assert_eq!(state.values(), Vec::<ElementId>::new());
        assert!(state.toggle_parent());
        assert_eq!(state.values(), ids(&["red", "green", "blue"]));
        assert!(!state.set_checked("teal", true), "undeclared value ignored");
        assert!(state.clear());
        assert!(!state.clear());

        let disabled = &mut CheckboxGroupState::new(["a"]).disabled(true);
        assert!(!disabled.set_checked("a", true));
        assert!(!disabled.toggle_parent());
        assert!(!disabled.clear());
        assert!(disabled.is_disabled());

        let empty = CheckboxGroupState::new(Vec::<ElementId>::new());
        assert_eq!(empty.parent_state(), ToggleState::Off);

        let preset = CheckboxGroupState::new(["a", "b"]).checked(["b", "b", "z"]);
        assert_eq!(preset.values(), ids(&["b"]));
    }

    #[test]
    #[should_panic(expected = "at most")]
    fn oversized_groups_are_rejected() {
        let values: Vec<ElementId> = (0..=MAX_CHECKBOX_GROUP_VALUES)
            .map(|index| ElementId::new(index as u64 + 1))
            .collect();
        let _ = CheckboxGroupState::new(values);
    }

    #[test]
    #[should_panic(expected = "distinct")]
    fn duplicate_values_are_rejected() {
        let _ = CheckboxGroupState::new(["a", "a"]);
    }

    #[test]
    fn parts_add_exact_semantics_without_appearance() {
        let state = CheckboxGroupState::new(["red", "green"]).checked(["red"]);
        let group = CheckboxGroup::new("colors", &state);
        let root = group.root_with(div().bg(Color::rgb8(1, 2, 3)));
        assert_eq!(root.explicit_id, Some("colors".into()));
        assert_eq!(root.accessibility.role, AccessibilityRole::Group);
        assert_eq!(root.visual.background, Some(Color::rgb8(1, 2, 3)));

        let checked = group.checkbox_with("red", div());
        assert_eq!(checked.explicit_id, Some(group.checkbox_id("red")));
        assert_eq!(checked.accessibility.role, AccessibilityRole::CheckBox);
        assert_eq!(checked.accessibility.toggled, Some(ToggleState::On));
        assert!(checked.clickable);
        assert_eq!(checked.visual.background, None);

        let unchecked = group.checkbox_with("green", div());
        assert_eq!(unchecked.accessibility.toggled, Some(ToggleState::Off));

        let parent = group.parent_with(div());
        assert_eq!(parent.explicit_id, Some(group.parent_id()));
        assert_eq!(parent.accessibility.toggled, Some(ToggleState::Mixed));

        let indicator = group.indicator_with(div());
        assert!(indicator.accessibility.hidden);

        let disabled_state = CheckboxGroupState::new(["red"]).disabled(true);
        let disabled = CheckboxGroup::new("colors", &disabled_state);
        assert!(disabled.checkbox_with("red", div()).accessibility.disabled);
        assert!(disabled.parent_with(div()).accessibility.disabled);

        let all = [
            group.root_id(),
            group.parent_id(),
            group.checkbox_id("red"),
            group.checkbox_id("green"),
        ];
        for (index, id) in all.iter().enumerate() {
            assert_ne!(*id, ElementId::new(0));
            assert_ne!(*id, ElementId::new(u64::MAX));
            assert!(!all[..index].contains(id));
        }

        let shorthand = checkbox_group("colors", &state);
        assert_eq!(shorthand.accessibility.role, AccessibilityRole::Group);
        assert!(shorthand.children.is_empty());
    }

    struct GroupView {
        colors: CheckboxGroupState,
        reported: Vec<Vec<ElementId>>,
    }

    impl GroupView {
        fn colors(view: &mut Self) -> &mut CheckboxGroupState {
            &mut view.colors
        }
    }

    impl View for GroupView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let group = CheckboxGroup::new("colors", &self.colors);
            let parent = group.on_parent_click(cx, Self::colors, |view, values, _| {
                view.reported.push(values.to_vec());
            });
            let mut root = group
                .root_with(div())
                .child(group.parent_with(div().child(text("All"))).on_click(parent));
            for value in ["red", "green", "blue"] {
                let click = group.on_checkbox_click(cx, value, Self::colors, |view, values, _| {
                    view.reported.push(values.to_vec());
                });
                root = root.child(
                    group
                        .checkbox_with(value, div().child(text(value)))
                        .on_click(click),
                );
            }
            root
        }
    }

    #[test]
    fn clicks_toggle_children_and_the_derived_parent() {
        let (mut cx, view) = TestAppContext::new(GroupView {
            colors: CheckboxGroupState::new(["red", "green", "blue"]),
            reported: Vec::new(),
        })
        .unwrap();
        let window = view.window_handle();
        let state = CheckboxGroupState::new(["red", "green", "blue"]);
        let group = CheckboxGroup::new("colors", &state);

        cx.click(window, group.checkbox_id("green")).unwrap();
        assert_eq!(
            cx.read(view, |view| view.colors.values().to_vec()).unwrap(),
            ids(&["green"])
        );
        assert_eq!(
            cx.read(view, |view| view.colors.parent_state()).unwrap(),
            ToggleState::Mixed
        );

        cx.click(window, group.parent_id()).unwrap();
        assert_eq!(
            cx.read(view, |view| view.colors.values().to_vec()).unwrap(),
            ids(&["red", "green", "blue"])
        );
        cx.click(window, group.parent_id()).unwrap();
        assert_eq!(
            cx.read(view, |view| view.colors.values().to_vec()).unwrap(),
            Vec::<ElementId>::new()
        );
        assert_eq!(cx.read(view, |view| view.reported.len()).unwrap(), 3);

        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("checkbox group accessibility node")
        };
        assert_eq!(node(group.root_id()).role(), accesskit::Role::Group);
        let parent = node(group.parent_id());
        assert_eq!(parent.role(), accesskit::Role::CheckBox);
        assert_eq!(parent.toggled(), Some(accesskit::Toggled::False));
        assert_eq!(
            node(group.checkbox_id("red")).role(),
            accesskit::Role::CheckBox
        );

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
