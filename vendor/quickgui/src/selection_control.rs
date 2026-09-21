use crate::{AccessibilityRole, Element, ToggleState, div};

/// Copyable declaration for one controlled, unstyled checkbox.
///
/// The application owns the checked value, listener, layout, indicator, label, colors, and motion.
/// QuickGUI supplies the checkbox role, exact on/off/mixed state, focus/click behavior, native
/// window-drag exclusion, desktop arrow cursor, and an accessibility-hidden indicator part.
///
/// The descriptor retains no allocation, task, timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a Checkbox descriptor has no effect until one of its parts is mounted"]
pub struct Checkbox {
    state: ToggleState,
    read_only: bool,
}

impl Checkbox {
    pub fn new(state: impl Into<ToggleState>) -> Self {
        Self {
            state: state.into(),
            read_only: false,
        }
    }

    /// Derive a parent checkbox from the checked values of the boxes it governs.
    ///
    /// This is Base UI's `parent` checkbox: every child checked reports on, none reports off, and
    /// a mix reports mixed. QuickGUI keeps no child registry — the application passes the values it
    /// is already rendering — so the derivation stays a pure function of the caller's own data.
    ///
    /// ```
    /// use quickgui::{Checkbox, ToggleState};
    ///
    /// assert_eq!(Checkbox::parent([true, true]).state(), ToggleState::On);
    /// assert_eq!(Checkbox::parent([false, false]).state(), ToggleState::Off);
    /// assert_eq!(Checkbox::parent([true, false]).state(), ToggleState::Mixed);
    /// // An empty group is unchecked rather than mixed.
    /// assert_eq!(Checkbox::parent([]).state(), ToggleState::Off);
    /// ```
    pub fn parent(children: impl IntoIterator<Item = bool>) -> Self {
        let mut any = false;
        let mut all = true;
        for checked in children {
            any |= checked;
            all &= checked;
        }
        Self::new(match (any, all) {
            (false, _) => ToggleState::Off,
            (true, true) => ToggleState::On,
            (true, false) => ToggleState::Mixed,
        })
    }

    /// Show a value the user may read but not change, Base UI's `readOnly`.
    ///
    /// Unlike a disabled checkbox, a read-only one stays focusable and stays in the Tab sequence.
    /// QuickGUI projects the state and refuses the transition through [`Self::next_state`]; the
    /// application's own listener asks for that transition rather than toggling blindly.
    pub const fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub const fn state(self) -> ToggleState {
        self.state
    }

    pub const fn is_read_only(self) -> bool {
        self.read_only
    }

    /// The state an activation moves this checkbox to, or `None` when it refuses.
    ///
    /// Mixed and off both move to on, and on moves to off, which is the web's tri-state contract.
    /// A read-only checkbox returns `None`, so a click listener that asks before writing cannot
    /// change a value the control is refusing.
    pub const fn next_state(self) -> Option<ToggleState> {
        if self.read_only {
            return None;
        }
        Some(match self.state {
            ToggleState::On => ToggleState::Off,
            ToggleState::Off | ToggleState::Mixed => ToggleState::On,
        })
    }

    /// The checked value activating a parent checkbox moves its whole group to.
    ///
    /// A partially or fully unchecked parent checks everything; a fully checked one clears it.
    /// Returns `None` when the parent is read-only.
    pub const fn parent_next_checked(self) -> Option<bool> {
        match self.next_state() {
            Some(ToggleState::On) => Some(true),
            Some(_) => Some(false),
            None => None,
        }
    }

    /// Decorate an application-owned root without adding layout or appearance.
    pub fn root_with(self, root: Element) -> Element {
        selection_root(root, AccessibilityRole::CheckBox, self.state)
            .accessibility_read_only(self.read_only)
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::button())
    }

    /// Hide an application-owned visual indicator from the accessible name.
    pub fn indicator_with(self, indicator: Element) -> Element {
        indicator.accessibility_hidden(true)
    }
    /// Create the unstyled indicator part. Use [`Self::indicator_with`] to supply an existing element.
    pub fn indicator(self) -> Element {
        self.indicator_with(crate::div())
    }
}

/// Copyable declaration for one controlled, unstyled radio button.
///
/// Put related roots inside [`RadioGroup::root_with`]. QuickGUI supplies roving Tab/arrow
/// behavior from the mounted semantic tree; the application owns every visual declaration.
/// The descriptor retains no allocation, task, timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a Radio descriptor has no effect until one of its parts is mounted"]
pub struct Radio {
    selected: bool,
    read_only: bool,
}

impl Radio {
    pub const fn new(selected: bool) -> Self {
        Self {
            selected,
            read_only: false,
        }
    }

    /// Show a value the user may read but not change, Base UI's `readOnly`.
    ///
    /// A read-only radio stays focusable and keeps taking part in arrow navigation; only the
    /// selection it would make is refused.
    pub const fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub const fn is_selected(self) -> bool {
        self.selected
    }

    pub const fn is_read_only(self) -> bool {
        self.read_only
    }

    /// Whether activating this radio may select it.
    pub const fn accepts_selection(self) -> bool {
        !self.read_only && !self.selected
    }

    /// Decorate an application-owned root without adding layout or appearance.
    pub fn root_with(self, root: Element) -> Element {
        selection_root(
            root,
            AccessibilityRole::RadioButton,
            ToggleState::from(self.selected),
        )
        .accessibility_read_only(self.read_only)
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::button())
    }

    /// Hide an application-owned visual indicator from the accessible name.
    pub fn indicator_with(self, indicator: Element) -> Element {
        indicator.accessibility_hidden(true)
    }
    /// Create the unstyled indicator part. Use [`Self::indicator_with`] to supply an existing element.
    pub fn indicator(self) -> Element {
        self.indicator_with(crate::div())
    }
}

/// Copyable declaration for one semantic radio group.
///
/// The group adds only the accessibility relationship used by Tab and arrow-key navigation. It
/// retains no item registry, allocation, task, timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "a RadioGroup descriptor has no effect until its root part is mounted"]
pub struct RadioGroup {
    read_only: bool,
    required: bool,
}

impl RadioGroup {
    pub const fn new() -> Self {
        Self {
            read_only: false,
            required: false,
        }
    }

    /// Show values the user may read but not change, Base UI's `readOnly`.
    pub const fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Require a selection before submission, Base UI's `required`.
    pub const fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub const fn is_read_only(self) -> bool {
        self.read_only
    }

    pub const fn is_required(self) -> bool {
        self.required
    }

    /// Decorate an application-owned group root without adding layout or appearance.
    pub fn root_with(self, root: Element) -> Element {
        root.accessibility_role(AccessibilityRole::RadioGroup)
            .accessibility_read_only(self.read_only)
            .required(self.required)
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }
}

/// Copyable declaration for one controlled, unstyled switch.
///
/// The application owns the checked value, listener, track/root presentation, thumb presentation,
/// label, and motion. QuickGUI supplies the switch role and ordinary control interaction contract.
/// The descriptor retains no allocation, task, timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a Switch descriptor has no effect until one of its parts is mounted"]
pub struct Switch {
    checked: bool,
    read_only: bool,
}

impl Switch {
    pub const fn new(checked: bool) -> Self {
        Self {
            checked,
            read_only: false,
        }
    }

    /// Show a value the user may read but not change, Base UI's `readOnly`.
    ///
    /// A read-only switch stays focusable and stays in the Tab sequence; only the toggle it would
    /// perform is refused, through [`Self::next_checked`].
    pub const fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub const fn is_checked(self) -> bool {
        self.checked
    }

    pub const fn is_read_only(self) -> bool {
        self.read_only
    }

    /// The value an activation moves this switch to, or `None` when it refuses.
    pub const fn next_checked(self) -> Option<bool> {
        if self.read_only {
            None
        } else {
            Some(!self.checked)
        }
    }

    /// Decorate an application-owned root/track without adding layout or appearance.
    pub fn root_with(self, root: Element) -> Element {
        selection_root(
            root,
            AccessibilityRole::Switch,
            ToggleState::from(self.checked),
        )
        .accessibility_read_only(self.read_only)
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::button())
    }

    /// Hide an application-owned visual thumb from the accessible name.
    pub fn thumb_with(self, thumb: Element) -> Element {
        thumb.accessibility_hidden(true)
    }
    /// Create the unstyled thumb part. Use [`Self::thumb_with`] to supply an existing element.
    pub fn thumb(self) -> Element {
        self.thumb_with(crate::div())
    }
}

/// Create an unstyled controlled checkbox root.
///
/// This shorthand is equivalent to `Checkbox::new(state).root_with(div())`. Use [`Checkbox`]
/// directly when composing a separate indicator part.
pub fn checkbox(state: impl Into<ToggleState>) -> Element {
    Checkbox::new(state).root_with(div())
}

/// Create an unstyled controlled radio root.
///
/// This shorthand is equivalent to `Radio::new(selected).root_with(div())`.
pub fn radio(selected: bool) -> Element {
    Radio::new(selected).root_with(div())
}

/// Create an unstyled semantic radio-group root.
///
/// This shorthand is equivalent to `RadioGroup::new().root_with(div())`.
pub fn radio_group() -> Element {
    RadioGroup::new().root_with(div())
}

/// Create an unstyled controlled switch root.
///
/// This shorthand is equivalent to `Switch::new(checked).root_with(div())`. Use [`Switch`]
/// directly when composing a separate thumb part.
pub fn switch(checked: bool) -> Element {
    Switch::new(checked).root_with(div())
}

fn selection_root(root: Element, role: AccessibilityRole, state: ToggleState) -> Element {
    root.accessibility_role(role)
        .toggle_state(state)
        .clickable()
        .cursor_default()
        .app_region_no_drag()
        .user_select_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppRegion, Color, CursorStyle, EventContext, Insets, IntoElement, TestAppContext,
        UserSelect, View, ViewContext, text,
    };

    #[test]
    fn parts_add_exact_behavior_without_layout_or_appearance() {
        let checkbox = Checkbox::new(ToggleState::Mixed);
        assert_eq!(checkbox.state(), ToggleState::Mixed);
        let checkbox_root = checkbox.root_with(
            div()
                .w(137.0)
                .bg(Color::rgb8(4, 5, 6))
                .border(3.0, Color::rgb8(7, 8, 9))
                .child("Visible label"),
        );
        assert_eq!(
            checkbox_root.accessibility.role,
            AccessibilityRole::CheckBox
        );
        assert_eq!(
            checkbox_root.accessibility.toggled,
            Some(ToggleState::Mixed)
        );
        assert!(checkbox_root.clickable);
        assert!(checkbox_root.focusable);
        assert_eq!(checkbox_root.cursor_style, Some(CursorStyle::Arrow));
        assert!(checkbox_root.cursor_style_explicit);
        assert_eq!(checkbox_root.app_region, Some(AppRegion::NoDrag));
        assert_eq!(checkbox_root.user_select, UserSelect::None);
        assert_eq!(checkbox_root.visual.background, Some(Color::rgb8(4, 5, 6)));
        assert_eq!(
            checkbox_root.visual.border_color,
            Some(Color::rgb8(7, 8, 9))
        );
        assert_eq!(checkbox_root.visual.border_widths, Insets::all(3.0));
        assert_eq!(checkbox_root.children.len(), 1);
        assert!(checkbox_root.transition.is_none());

        let indicator = checkbox.indicator_with(
            div()
                .size(19.0, 17.0)
                .bg(Color::rgb8(10, 11, 12))
                .child("decorative check"),
        );
        assert!(indicator.accessibility.hidden);
        assert_eq!(indicator.visual.background, Some(Color::rgb8(10, 11, 12)));
        assert_eq!(indicator.children.len(), 1);

        let radio = Radio::new(true);
        assert!(radio.is_selected());
        let radio_root = radio.root_with(div());
        assert_eq!(
            radio_root.accessibility.role,
            AccessibilityRole::RadioButton
        );
        assert_eq!(radio_root.accessibility.toggled, Some(ToggleState::On));
        assert!(radio.indicator_with(div()).accessibility.hidden);

        let switch = Switch::new(false);
        assert!(!switch.is_checked());
        let switch_root = switch.root_with(div());
        assert_eq!(switch_root.accessibility.role, AccessibilityRole::Switch);
        assert_eq!(switch_root.accessibility.toggled, Some(ToggleState::Off));
        assert!(switch.thumb_with(div()).accessibility.hidden);

        let group = RadioGroup::new().root_with(
            div()
                .bg(Color::rgb8(13, 14, 15))
                .child("Application-owned group"),
        );
        assert_eq!(group.accessibility.role, AccessibilityRole::RadioGroup);
        assert!(!group.clickable);
        assert!(!group.focusable);
        assert_eq!(group.visual.background, Some(Color::rgb8(13, 14, 15)));
        assert_eq!(group.children.len(), 1);
    }

    #[test]
    fn shorthands_are_semantic_empty_unstyled_roots() {
        let checked = checkbox(true);
        assert_eq!(checked.accessibility.role, AccessibilityRole::CheckBox);
        assert_eq!(checked.accessibility.toggled, Some(ToggleState::On));
        assert!(checked.children.is_empty());
        assert_eq!(checked.visual.background, None);
        assert_eq!(checked.visual.border_color, None);

        let mixed = checkbox(ToggleState::Mixed);
        assert_eq!(mixed.accessibility.toggled, Some(ToggleState::Mixed));

        let radio = radio(false);
        assert_eq!(radio.accessibility.role, AccessibilityRole::RadioButton);
        assert_eq!(radio.accessibility.toggled, Some(ToggleState::Off));

        let switch = switch(true);
        assert_eq!(switch.accessibility.role, AccessibilityRole::Switch);
        assert_eq!(switch.accessibility.toggled, Some(ToggleState::On));
        assert_eq!(
            radio_group().accessibility.role,
            AccessibilityRole::RadioGroup
        );
    }

    #[derive(Default)]
    struct ControlView {
        checked: bool,
        radio: usize,
        switched: bool,
    }

    impl View for ControlView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let check = cx.listener("check", |view, cx: &mut EventContext| {
                view.checked = !view.checked;
                cx.invalidate();
            });
            let radio_one = cx.listener("radio-one", |view, cx| {
                view.radio = 1;
                cx.invalidate();
            });
            let radio_zero = cx.listener("radio-zero", |view, cx| {
                view.radio = 0;
                cx.invalidate();
            });
            let toggle = cx.listener("switch", |view, cx| {
                view.switched = !view.switched;
                cx.invalidate();
            });

            let checkbox_control = Checkbox::new(self.checked);
            let first_radio = Radio::new(self.radio == 0);
            let second_radio = Radio::new(self.radio == 1);
            let switch = Switch::new(self.switched);

            div().children([
                checkbox_control
                    .root_with(
                        div().child(checkbox_control.indicator_with(text("decorative mark"))),
                    )
                    .id("check")
                    .on_click(check)
                    .child("Checkbox"),
                RadioGroup::new().root_with(
                    div().children([
                        first_radio
                            .root_with(div().child(first_radio.indicator_with(div())))
                            .id("radio-zero")
                            .on_click(radio_zero)
                            .child("First radio"),
                        second_radio
                            .root_with(div().child(second_radio.indicator_with(div())))
                            .id("radio-one")
                            .on_click(radio_one)
                            .child("Second radio"),
                        radio(false)
                            .id("radio-disabled")
                            .disabled(true)
                            .child("Disabled radio"),
                    ]),
                ),
                switch
                    .root_with(div().child(switch.thumb_with(div())))
                    .id("switch")
                    .on_click(toggle)
                    .child("Switch"),
                checkbox(false)
                    .id("disabled-check")
                    .disabled(true)
                    .child("Disabled"),
            ])
        }
    }

    #[test]
    fn parts_use_existing_click_focus_radio_and_keyboard_paths_without_idle_work() {
        let (mut cx, view) = TestAppContext::new(ControlView::default()).unwrap();
        let window = view.window_handle();

        cx.click(window, "check").unwrap();
        assert!(cx.read(view, |view| view.checked).unwrap());
        cx.simulate_keystrokes(window, "space").unwrap();
        assert!(!cx.read(view, |view| view.checked).unwrap());

        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("radio-zero".into()));
        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("radio-one".into()));
        assert_eq!(cx.read(view, |view| view.radio).unwrap(), 1);
        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("radio-zero".into()));
        assert_eq!(cx.read(view, |view| view.radio).unwrap(), 0);
        cx.simulate_keystrokes(window, "left").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("radio-one".into()));
        assert_eq!(cx.read(view, |view| view.radio).unwrap(), 1);

        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("switch".into()));
        cx.click(window, "switch").unwrap();
        assert!(cx.read(view, |view| view.switched).unwrap());
        assert!(cx.click(window, "disabled-check").is_err());

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn read_only_selection_controls_refuse_transitions_without_leaving_the_tab_sequence() {
        let checkbox = Checkbox::new(true).read_only(true);
        assert!(checkbox.is_read_only());
        assert_eq!(checkbox.next_state(), None);
        assert_eq!(checkbox.parent_next_checked(), None);
        let root = checkbox.root_with(div());
        assert!(root.accessibility.read_only);
        assert!(!root.accessibility.disabled);
        assert!(root.focusable);
        assert!(root.is_keyboard_focusable());

        // An ordinary checkbox still reports the transition a click should make.
        assert_eq!(Checkbox::new(false).next_state(), Some(ToggleState::On));
        assert_eq!(
            Checkbox::new(ToggleState::Mixed).next_state(),
            Some(ToggleState::On)
        );
        assert_eq!(Checkbox::new(true).next_state(), Some(ToggleState::Off));
        assert!(
            !Checkbox::new(false)
                .root_with(div())
                .accessibility
                .read_only
        );

        // A disabled control leaves the sequence; a read-only one does not.
        let disabled = Checkbox::new(true).root_with(div()).disabled(true);
        assert!(!disabled.is_keyboard_focusable());

        let radio = Radio::new(false).read_only(true);
        assert!(radio.is_read_only());
        assert!(!radio.accepts_selection());
        assert!(Radio::new(false).accepts_selection());
        // A radio that is already selected has nothing to select.
        assert!(!Radio::new(true).accepts_selection());
        assert!(radio.root_with(div()).accessibility.read_only);

        let group = RadioGroup::new().read_only(true).required(true);
        assert!(group.is_read_only());
        assert!(group.is_required());
        let group_root = group.root_with(div());
        assert!(group_root.accessibility.read_only);
        assert!(group_root.accessibility.required);
        assert_eq!(group_root.accessibility.role, AccessibilityRole::RadioGroup);

        let switch = Switch::new(true).read_only(true);
        assert_eq!(switch.next_checked(), None);
        assert_eq!(Switch::new(true).next_checked(), Some(false));
        assert_eq!(Switch::new(false).next_checked(), Some(true));
        assert!(switch.root_with(div()).accessibility.read_only);
        assert!(switch.thumb_with(div()).accessibility.hidden);
    }

    #[test]
    fn a_parent_checkbox_derives_its_state_and_the_move_it_makes() {
        assert_eq!(
            Checkbox::parent([true, true, true]).state(),
            ToggleState::On
        );
        assert_eq!(
            Checkbox::parent([false, false, false]).state(),
            ToggleState::Off
        );
        assert_eq!(
            Checkbox::parent([true, false, true]).state(),
            ToggleState::Mixed
        );
        assert_eq!(
            Checkbox::parent(std::iter::empty()).state(),
            ToggleState::Off
        );

        // A partly or fully unchecked parent checks everything; a full one clears it.
        assert_eq!(
            Checkbox::parent([true, false]).parent_next_checked(),
            Some(true)
        );
        assert_eq!(
            Checkbox::parent([false, false]).parent_next_checked(),
            Some(true)
        );
        assert_eq!(
            Checkbox::parent([true, true]).parent_next_checked(),
            Some(false)
        );
        assert_eq!(
            Checkbox::parent([true, false])
                .read_only(true)
                .parent_next_checked(),
            None
        );

        // The derived state reaches the mounted root as the mixed checkbox contract.
        let mixed = Checkbox::parent([true, false]).root_with(div());
        assert_eq!(mixed.accessibility.toggled, Some(ToggleState::Mixed));
        assert_eq!(mixed.accessibility.role, AccessibilityRole::CheckBox);
    }
}
