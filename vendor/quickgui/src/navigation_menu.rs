use std::sync::Arc;
use web_time::{Duration, Instant};

use crate::{
    AccessibilityOrientation, AccessibilityPopover, AccessibilityRole, AnchorPlacement,
    ClickListener, DismissListener, Element, ElementId, EventContext, FocusHandle, HoverListener,
    KeyBinding, Popover, PopoverKind, StateAccessor, ViewContext, div,
};

/// Maximum top-level items one navigation menu retains.
///
/// Arrow navigation scans the caller's ordered item list rather than a registry, so the bound
/// keeps one keypress a bounded walk even when application data drives the bar.
pub const MAX_NAVIGATION_MENU_ITEMS: usize = 64;

/// Default hover delay before a navigation menu opens, matching Base UI's `delay`.
pub const DEFAULT_NAVIGATION_MENU_DELAY: Duration = Duration::from_millis(50);
/// Default delay before a navigation menu closes once the pointer leaves it.
pub const DEFAULT_NAVIGATION_MENU_CLOSE_DELAY: Duration = Duration::from_millis(50);
/// Longest open or close delay one navigation menu may declare.
pub const MAX_NAVIGATION_MENU_DELAY: Duration = Duration::from_secs(10);

/// Key context used by a horizontal navigation menu's triggers.
pub const NAVIGATION_MENU_HORIZONTAL_KEY_CONTEXT: &str = "NavigationMenuHorizontal";
/// Key context used by a vertical navigation menu's triggers.
pub const NAVIGATION_MENU_VERTICAL_KEY_CONTEXT: &str = "NavigationMenuVertical";

const NAVIGATION_MENU_ITEM_ID_TAG: u64 = 0x2a5f_3c81_b74e_d096;
const NAVIGATION_MENU_TRIGGER_ID_TAG: u64 = 0xc613_87ad_2f50_9be4;
const NAVIGATION_MENU_ICON_ID_TAG: u64 = 0x48e2_d09b_5a37_16cf;
const NAVIGATION_MENU_POPUP_ID_TAG: u64 = 0x9d70_1fb6_e483_2c5a;
const NAVIGATION_MENU_CONTENT_ID_TAG: u64 = 0x51ba_6e7c_309f_84d2;
const NAVIGATION_MENU_VIEWPORT_ID_TAG: u64 = 0xf28c_43a0_9d61_be75;
const NAVIGATION_MENU_ARROW_ID_TAG: u64 = 0x6704_b95e_1c8d_a2f3;

/// Move navigation-menu focus to the next enabled trigger.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NavigationMenuNext;
/// Move navigation-menu focus to the previous enabled trigger.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NavigationMenuPrevious;
/// Move navigation-menu focus to the first enabled trigger.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NavigationMenuFirst;
/// Move navigation-menu focus to the last enabled trigger.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NavigationMenuLast;
/// Close the open navigation menu without leaving the bar.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NavigationMenuClose;

/// Contextual bindings used by [`NavigationMenuEntry::key_with`].
///
/// A horizontal menu answers Left and Right; a vertical menu answers Up and Down. Enter and Space
/// stay ordinary button activation on the trigger, so they reach the caller's click listener
/// through the same path a plain button uses.
pub fn navigation_menu_key_bindings() -> [KeyBinding; 10] {
    [
        KeyBinding::new(
            "right",
            NavigationMenuNext,
            Some(NAVIGATION_MENU_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "left",
            NavigationMenuPrevious,
            Some(NAVIGATION_MENU_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "home",
            NavigationMenuFirst,
            Some(NAVIGATION_MENU_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "end",
            NavigationMenuLast,
            Some(NAVIGATION_MENU_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "escape",
            NavigationMenuClose,
            Some(NAVIGATION_MENU_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "down",
            NavigationMenuNext,
            Some(NAVIGATION_MENU_VERTICAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "up",
            NavigationMenuPrevious,
            Some(NAVIGATION_MENU_VERTICAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "home",
            NavigationMenuFirst,
            Some(NAVIGATION_MENU_VERTICAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "end",
            NavigationMenuLast,
            Some(NAVIGATION_MENU_VERTICAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "escape",
            NavigationMenuClose,
            Some(NAVIGATION_MENU_VERTICAL_KEY_CONTEXT),
        ),
    ]
}

/// Layout and keyboard axis for one navigation menu.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NavigationMenuOrientation {
    #[default]
    Horizontal,
    Vertical,
}

impl NavigationMenuOrientation {
    const fn accessibility(self) -> AccessibilityOrientation {
        match self {
            Self::Horizontal => AccessibilityOrientation::Horizontal,
            Self::Vertical => AccessibilityOrientation::Vertical,
        }
    }

    const fn key_context(self) -> &'static str {
        match self {
            Self::Horizontal => NAVIGATION_MENU_HORIZONTAL_KEY_CONTEXT,
            Self::Vertical => NAVIGATION_MENU_VERTICAL_KEY_CONTEXT,
        }
    }
}

/// Which way the active menu moved, for the application's own transition.
///
/// QuickGUI never animates the panel itself; it only reports the direction so an application can
/// slide its content the way the user's attention travelled.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NavigationMenuActivationDirection {
    /// The menu opened from nothing or closed, so there is no direction to animate.
    #[default]
    None,
    Left,
    Right,
    Up,
    Down,
}

/// Controlled, allocation-free state for one navigation menu.
///
/// The state owns which item is open, which trigger holds the bar's single Tab stop, the exact
/// hover open/close deadlines, and the activation direction. Like [`crate::ToastManager`], it owns
/// no timer: [`Self::next_deadline`] reports the single instant a repaint is needed, [`Self::poll`]
/// applies it, and a settled menu reports none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavigationMenuState {
    value: Option<ElementId>,
    value_index: usize,
    focused: Option<ElementId>,
    orientation: NavigationMenuOrientation,
    delay: Duration,
    close_delay: Duration,
    pending_open: Option<(ElementId, usize, Instant)>,
    pending_close: Option<Instant>,
    activation: NavigationMenuActivationDirection,
    popup_hovered: bool,
    trigger_hovered: Option<ElementId>,
}

impl Default for NavigationMenuState {
    fn default() -> Self {
        Self::new()
    }
}

impl NavigationMenuState {
    pub const fn new() -> Self {
        Self {
            value: None,
            value_index: 0,
            focused: None,
            orientation: NavigationMenuOrientation::Horizontal,
            delay: DEFAULT_NAVIGATION_MENU_DELAY,
            close_delay: DEFAULT_NAVIGATION_MENU_CLOSE_DELAY,
            pending_open: None,
            pending_close: None,
            activation: NavigationMenuActivationDirection::None,
            popup_hovered: false,
            trigger_hovered: None,
        }
    }

    #[must_use]
    pub const fn orientation(mut self, orientation: NavigationMenuOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    #[must_use]
    pub const fn vertical(self) -> Self {
        self.orientation(NavigationMenuOrientation::Vertical)
    }

    /// Replace the hover-to-open delay, clamped to [`MAX_NAVIGATION_MENU_DELAY`].
    #[must_use]
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay.min(MAX_NAVIGATION_MENU_DELAY);
        self
    }

    /// Replace the pointer-leave close delay, clamped to [`MAX_NAVIGATION_MENU_DELAY`].
    #[must_use]
    pub fn close_delay(mut self, close_delay: Duration) -> Self {
        self.close_delay = close_delay.min(MAX_NAVIGATION_MENU_DELAY);
        self
    }

    pub const fn axis(&self) -> NavigationMenuOrientation {
        self.orientation
    }

    pub const fn open_delay(&self) -> Duration {
        self.delay
    }

    pub const fn close_delay_value(&self) -> Duration {
        self.close_delay
    }

    /// The open item, or `None` while every menu is closed.
    pub const fn value(&self) -> Option<ElementId> {
        self.value
    }

    pub fn is_open(&self, value: impl Into<ElementId>) -> bool {
        self.value == Some(value.into())
    }

    pub const fn activation_direction(&self) -> NavigationMenuActivationDirection {
        self.activation
    }

    /// The trigger that currently owns the bar's single Tab stop.
    pub const fn focused(&self) -> Option<ElementId> {
        self.focused
    }

    /// Move the bar's single Tab stop, returning whether it changed.
    pub fn focus(&mut self, value: impl Into<ElementId>) -> bool {
        let value = Some(value.into());
        if self.focused == value {
            return false;
        }
        self.focused = value;
        true
    }

    /// Open one item immediately, returning whether the open value changed.
    pub fn open(&mut self, value: impl Into<ElementId>, index: usize) -> bool {
        let value = value.into();
        self.pending_open = None;
        self.pending_close = None;
        if self.value == Some(value) {
            return false;
        }
        self.activation = self.direction_from(index);
        self.value = Some(value);
        self.value_index = index;
        self.focused = Some(value);
        true
    }

    /// Toggle one item, returning whether the open value changed.
    pub fn toggle(&mut self, value: impl Into<ElementId>, index: usize) -> bool {
        let value = value.into();
        if self.value == Some(value) {
            return self.close();
        }
        self.open(value, index)
    }

    /// Close every menu, returning whether the open value changed.
    pub fn close(&mut self) -> bool {
        self.pending_open = None;
        self.pending_close = None;
        self.popup_hovered = false;
        self.trigger_hovered = None;
        self.activation = NavigationMenuActivationDirection::None;
        if self.value.is_none() {
            return false;
        }
        self.value = None;
        true
    }

    /// Report the pointer entering or leaving one trigger. Returns whether anything changed.
    ///
    /// With a menu already open, moving onto a different trigger switches immediately, which is
    /// the behavior a menu bar and a navigation menu share. With every menu closed, the first
    /// hover arms the exact open deadline instead.
    pub fn hover_trigger(
        &mut self,
        value: impl Into<ElementId>,
        index: usize,
        hovered: bool,
        now: Instant,
    ) -> bool {
        let value = value.into();
        if hovered {
            if self.trigger_hovered == Some(value) {
                return false;
            }
            self.trigger_hovered = Some(value);
            self.pending_close = None;
            if self.value.is_some() || self.delay.is_zero() {
                self.open(value, index);
                return true;
            }
            self.pending_open = Some((value, index, now + self.delay));
            return true;
        }
        if self.trigger_hovered != Some(value) {
            return false;
        }
        self.trigger_hovered = None;
        if self
            .pending_open
            .is_some_and(|(pending, _, _)| pending == value)
        {
            self.pending_open = None;
        }
        self.arm_close(now);
        true
    }

    /// Report the pointer entering or leaving the open popup. Returns whether anything changed.
    pub fn hover_popup(&mut self, hovered: bool, now: Instant) -> bool {
        if self.popup_hovered == hovered {
            return false;
        }
        self.popup_hovered = hovered;
        if hovered {
            self.pending_close = None;
        } else {
            self.arm_close(now);
        }
        true
    }

    /// The single instant a repaint is needed, or `None` while the menu is settled.
    pub fn next_deadline(&self) -> Option<Instant> {
        match (
            self.pending_open.map(|(_, _, deadline)| deadline),
            self.pending_close,
        ) {
            (Some(open), Some(close)) => Some(open.min(close)),
            (open, close) => open.or(close),
        }
    }

    /// Apply every elapsed deadline, returning whether the open value changed.
    pub fn poll(&mut self, now: Instant) -> bool {
        let mut changed = false;
        if let Some((value, index, deadline)) = self.pending_open
            && deadline <= now
        {
            self.pending_open = None;
            changed |= self.open(value, index);
        }
        if self.pending_close.is_some_and(|deadline| deadline <= now) {
            self.pending_close = None;
            changed |= self.close();
        }
        changed
    }

    fn arm_close(&mut self, now: Instant) {
        if self.value.is_none() || self.popup_hovered || self.trigger_hovered.is_some() {
            return;
        }
        if self.close_delay.is_zero() {
            self.pending_close = None;
            self.close();
        } else {
            self.pending_close = Some(now + self.close_delay);
        }
    }

    fn direction_from(&self, index: usize) -> NavigationMenuActivationDirection {
        if self.value.is_none() {
            return NavigationMenuActivationDirection::None;
        }
        match (self.orientation, index.cmp(&self.value_index)) {
            (_, std::cmp::Ordering::Equal) => NavigationMenuActivationDirection::None,
            (NavigationMenuOrientation::Horizontal, std::cmp::Ordering::Greater) => {
                NavigationMenuActivationDirection::Right
            }
            (NavigationMenuOrientation::Horizontal, std::cmp::Ordering::Less) => {
                NavigationMenuActivationDirection::Left
            }
            (NavigationMenuOrientation::Vertical, std::cmp::Ordering::Greater) => {
                NavigationMenuActivationDirection::Down
            }
            (NavigationMenuOrientation::Vertical, std::cmp::Ordering::Less) => {
                NavigationMenuActivationDirection::Up
            }
        }
    }
}

/// One caller-declared navigation-menu item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NavigationMenuItem {
    value: ElementId,
    disabled: bool,
}

impl NavigationMenuItem {
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

/// A controlled, unstyled navigation-menu descriptor.
///
/// The application owns every item's content, icon, panel layout, colors, and transitions.
/// QuickGUI supplies the Navigation landmark, the list relationship, stable per-item identities,
/// one roving Tab stop with bounded arrow/Home/End navigation, hover and click opening on exact
/// deadlines, Escape closing without leaving the bar, and each panel's anchored placement and
/// dismissal over the existing in-window [`Popover`].
///
/// The descriptor borrows the caller's ordered item list and retains no registry, task, timer,
/// observer, or idle scheduler source.
#[derive(Clone, Copy, Debug)]
#[must_use = "a NavigationMenu descriptor has no effect until its parts are mounted"]
pub struct NavigationMenu<'a> {
    root_id: ElementId,
    items: &'a [NavigationMenuItem],
    value: Option<ElementId>,
    focused: Option<ElementId>,
    orientation: NavigationMenuOrientation,
    placement: AnchorPlacement,
    loop_focus: bool,
}

impl<'a> NavigationMenu<'a> {
    /// Create a navigation menu over the caller's ordered items.
    ///
    /// # Panics
    ///
    /// Panics with more than [`MAX_NAVIGATION_MENU_ITEMS`] items or duplicate item values.
    pub fn new(
        root_id: impl Into<ElementId>,
        state: &NavigationMenuState,
        items: &'a [NavigationMenuItem],
    ) -> Self {
        assert!(
            items.len() <= MAX_NAVIGATION_MENU_ITEMS,
            "a navigation menu retains at most {MAX_NAVIGATION_MENU_ITEMS} items"
        );
        for (index, item) in items.iter().enumerate() {
            assert!(
                !items[..index].iter().any(|other| other.value == item.value),
                "navigation menu item values must be distinct"
            );
        }
        Self {
            root_id: root_id.into(),
            items,
            value: state.value(),
            focused: state.focused(),
            orientation: state.axis(),
            placement: AnchorPlacement::BottomStart,
            loop_focus: true,
        }
    }

    pub const fn placement(mut self, placement: AnchorPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Wrap from the last trigger to the first. The default is `true`.
    pub const fn loop_focus(mut self, loop_focus: bool) -> Self {
        self.loop_focus = loop_focus;
        self
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub const fn items(self) -> &'a [NavigationMenuItem] {
        self.items
    }

    pub const fn axis(self) -> NavigationMenuOrientation {
        self.orientation
    }

    pub const fn value(self) -> Option<ElementId> {
        self.value
    }

    /// The trigger that owns the bar's single Tab stop.
    ///
    /// This is the controlled focused value while it names an enabled mounted item, and otherwise
    /// the first enabled item, matching the composite-widget convention.
    pub fn roving_value(self) -> Option<ElementId> {
        let focused = self.focused.filter(|focused| {
            self.items
                .iter()
                .any(|item| item.value == *focused && !item.disabled)
        });
        focused.or_else(|| {
            self.items
                .iter()
                .find(|item| !item.disabled)
                .map(|item| item.value)
        })
    }

    /// Decorate an application-owned root without adding layout or appearance.
    pub fn root_with(self, root: Element) -> Element {
        root.id(self.root_id)
            .accessibility_role(AccessibilityRole::Navigation)
            .app_region_no_drag()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate the application-owned list of items.
    pub fn list_with(self, list: Element) -> Element {
        list.accessibility_role(AccessibilityRole::List)
            .accessibility_orientation(self.orientation.accessibility())
            .app_region_no_drag()
    }
    /// Create the unstyled list part. Use [`Self::list_with`] to supply an existing element.
    pub fn list(self) -> Element {
        self.list_with(crate::div())
    }

    /// Decorate an application-owned navigation link without adding appearance.
    ///
    /// `active` marks the link for the current destination; it projects as the native selected
    /// state rather than as a visual class.
    pub fn link_with(self, id: impl Into<ElementId>, active: bool, link: Element) -> Element {
        link.id(id)
            .accessibility_role(AccessibilityRole::Link)
            .selected(active)
            .clickable()
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled link part. Use [`Self::link_with`] to supply an existing element.
    pub fn link(self, id: impl Into<ElementId>, active: bool) -> Element {
        self.link_with(id, active, crate::div())
    }

    /// Describe one declared item.
    pub fn entry(self, value: impl Into<ElementId>) -> Option<NavigationMenuEntry<'a>> {
        let value = value.into();
        let index = self.items.iter().position(|item| item.value == value)?;
        Some(NavigationMenuEntry {
            menu: self,
            item: self.items[index],
            index,
            roving: self.roving_value() == Some(value),
        })
    }

    /// Ask for the one repaint an armed open or close deadline needs.
    ///
    /// Call this while rendering. A settled navigation menu requests nothing, so the window sleeps.
    pub fn schedule<V: 'static>(cx: &mut ViewContext<'_, V>, state: &NavigationMenuState) {
        if let Some(deadline) = state.next_deadline() {
            cx.request_repaint_at(deadline);
        }
    }
}

/// A copyable declaration for one mounted navigation-menu item.
#[derive(Clone, Copy, Debug)]
#[must_use = "a NavigationMenuEntry descriptor has no effect until its parts are mounted"]
pub struct NavigationMenuEntry<'a> {
    menu: NavigationMenu<'a>,
    item: NavigationMenuItem,
    index: usize,
    roving: bool,
}

impl<'a> NavigationMenuEntry<'a> {
    pub const fn value(self) -> ElementId {
        self.item.value
    }

    pub const fn index(self) -> usize {
        self.index
    }

    pub const fn is_disabled(self) -> bool {
        self.item.disabled
    }

    /// Whether this trigger owns the bar's single Tab stop.
    pub const fn is_roving_stop(self) -> bool {
        self.roving
    }

    pub fn is_open(self) -> bool {
        self.menu.value == Some(self.item.value)
    }

    pub fn item_id(self) -> ElementId {
        self.derived(NAVIGATION_MENU_ITEM_ID_TAG)
    }

    pub fn trigger_id(self) -> ElementId {
        self.derived(NAVIGATION_MENU_TRIGGER_ID_TAG)
    }

    pub fn icon_id(self) -> ElementId {
        self.derived(NAVIGATION_MENU_ICON_ID_TAG)
    }

    pub fn popup_id(self) -> ElementId {
        self.derived(NAVIGATION_MENU_POPUP_ID_TAG)
    }

    pub fn content_id(self) -> ElementId {
        self.derived(NAVIGATION_MENU_CONTENT_ID_TAG)
    }

    pub fn viewport_id(self) -> ElementId {
        self.derived(NAVIGATION_MENU_VIEWPORT_ID_TAG)
    }

    pub fn arrow_id(self) -> ElementId {
        self.derived(NAVIGATION_MENU_ARROW_ID_TAG)
    }

    pub fn positioner_id(self) -> ElementId {
        self.popover().positioner_id()
    }

    pub fn backdrop_id(self) -> ElementId {
        self.popover().backdrop_id()
    }

    /// The composed popover this item's panel is mounted through.
    pub fn popover(self) -> Popover {
        Popover::new(self.trigger_id(), self.popup_id(), self.is_open())
            .kind(PopoverKind::Menu)
            .placement(self.menu.placement)
    }

    /// Decorate an application-owned item without adding layout or appearance.
    pub fn item_with(self, item: Element) -> Element {
        item.id(self.item_id())
            .accessibility_role(AccessibilityRole::ListItem)
            .accessibility_position_in_set(self.index)
            .accessibility_size_of_set(self.menu.items.len())
            .app_region_no_drag()
    }
    /// Create the unstyled item part. Use [`Self::item_with`] to supply an existing element.
    pub fn item(self) -> Element {
        self.item_with(crate::div())
    }

    /// Decorate an application-owned trigger without adding appearance.
    ///
    /// Exactly one enabled trigger is in the window's normal Tab sequence; the rest are reachable
    /// with the menu's arrow keys.
    pub fn trigger_with(self, trigger: Element) -> Element {
        let disabled = self.item.disabled || trigger.accessibility.disabled;
        let trigger = trigger
            .id(self.trigger_id())
            .clickable()
            .tab_index(if self.roving { 0 } else { -1 })
            .key_context(self.menu.orientation.key_context())
            .accessibility_role(AccessibilityRole::Button)
            .accessibility_has_popover(AccessibilityPopover::Menu)
            .accessibility_expanded(self.is_open())
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
            .disabled(disabled);
        if self.is_open() {
            trigger.accessibility_controls(self.popup_id())
        } else {
            trigger
        }
    }
    /// Create the unstyled trigger part. Use [`Self::trigger_with`] to supply an existing element.
    pub fn trigger(self) -> Element {
        self.trigger_with(crate::button())
    }

    /// Hide an application-owned trigger icon from the accessible name.
    pub fn icon_with(self, icon: Element) -> Element {
        icon.id(self.icon_id()).accessibility_hidden(true)
    }
    /// Create the unstyled icon part. Use [`Self::icon_with`] to supply an existing element.
    pub fn icon(self) -> Element {
        self.icon_with(crate::div())
    }

    /// Decorate the caller-owned portal boundary.
    ///
    /// QuickGUI's retained overlay node is itself the portal, so this is the same boundary as
    /// [`Self::positioner_with`]; mount exactly one of them.
    pub fn portal_with(self, portal: Element) -> Element {
        self.popover().positioner_with(portal)
    }
    /// Create the unstyled portal part. Use [`Self::portal_with`] to supply an existing element.
    pub fn portal(self) -> Element {
        self.portal_with(crate::div())
    }

    /// Decorate the caller-owned positioner without adding appearance.
    pub fn positioner_with(self, positioner: Element) -> Element {
        self.popover().positioner_with(positioner)
    }
    /// Create the unstyled positioner part. Use [`Self::positioner_with`] to supply an existing element.
    pub fn positioner(self) -> Element {
        self.positioner_with(crate::div())
    }

    /// Decorate the application-owned popup without adding layout or appearance.
    ///
    /// The popup emits [`crate::Event::Dismiss`] under [`Self::popup_id`] for Escape and for an
    /// outside pointer press, and restores focus to its trigger.
    pub fn popup_with(self, popup: Element) -> Element {
        self.popover().popup_with(popup)
    }
    /// Create the unstyled popup part. Use [`Self::popup_with`] to supply an existing element.
    pub fn popup(self) -> Element {
        self.popup_with(crate::div())
    }

    /// Decorate the caller-owned viewport inside the popup.
    ///
    /// The viewport is where an application clips and animates a panel that changes size between
    /// items; QuickGUI adds no motion of its own.
    pub fn viewport_with(self, viewport: Element) -> Element {
        viewport.id(self.viewport_id())
    }
    /// Create the unstyled viewport part. Use [`Self::viewport_with`] to supply an existing element.
    pub fn viewport(self) -> Element {
        self.viewport_with(crate::div())
    }

    /// Decorate the caller-owned panel content.
    pub fn content_with(self, content: Element) -> Element {
        content
            .id(self.content_id())
            .accessibility_labelled_by(self.trigger_id())
    }
    /// Create the unstyled content part. Use [`Self::content_with`] to supply an existing element.
    pub fn content(self) -> Element {
        self.content_with(crate::div())
    }

    /// Decorate the application-owned arrow, which is decorative and hidden.
    pub fn arrow_with(self, arrow: Element) -> Element {
        arrow.id(self.arrow_id()).accessibility_hidden(true)
    }
    /// Create the unstyled arrow part. Use [`Self::arrow_with`] to supply an existing element.
    pub fn arrow(self) -> Element {
        self.arrow_with(crate::div())
    }

    /// Decorate an optional caller-painted viewport backdrop.
    pub fn backdrop_with(self, backdrop: Element) -> Element {
        self.popover().backdrop_with(backdrop)
    }
    /// Create the unstyled backdrop part. Use [`Self::backdrop_with`] to supply an existing element.
    pub fn backdrop(self) -> Element {
        self.backdrop_with(crate::div())
    }

    /// Attach QuickGUI's typed navigation-menu keyboard actions to this trigger.
    ///
    /// Install [`navigation_menu_key_bindings`] once on the application keymap. Each trigger
    /// answers its own arrow keys, so the focused trigger is always the one that moves.
    ///
    /// Arrow, Home, and End move the bar's single Tab stop between enabled triggers and leave the
    /// open panel alone; Enter and Space reach the caller's click listener through ordinary button
    /// activation and open the focused item, hovering switches panels immediately, and Escape
    /// closes the open panel without leaving the bar. Keyboard movement deliberately does not swap
    /// panels: unmounting the previous panel restores focus to its own trigger, which would undo
    /// the move the user just made.
    pub fn key_with<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        trigger: Element,
        access: fn(&mut V) -> &mut NavigationMenuState,
    ) -> Element {
        self.key_with_accessor(cx, trigger, StateAccessor::from(access))
    }
    /// Create the unstyled key part. Use [`Self::key_with`] to supply an existing element.
    pub fn key<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut NavigationMenuState,
    ) -> Element {
        self.key_with(cx, crate::div(), access)
    }

    /// Attach the typed keyboard actions against a per-instance state accessor.
    pub fn key_with_accessor<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        trigger: Element,
        access_source: StateAccessor<V, NavigationMenuState>,
    ) -> Element {
        let id = self.trigger_id();
        let root = self.menu.root_id;
        let value = self.item.value;
        let loop_focus = self.menu.loop_focus;
        let items: Arc<[NavigationMenuItem]> = Arc::from(self.menu.items);

        let next_items = items.clone();
        let access = access_source.clone();
        let next = cx.action_listener(id, move |view, _: &NavigationMenuNext, cx| {
            let target = neighbor(&next_items, value, true, loop_focus);
            move_navigation_focus(view, cx, &access, root, &next_items, target);
        });
        let previous_items = items.clone();
        let access = access_source.clone();
        let previous = cx.action_listener(id, move |view, _: &NavigationMenuPrevious, cx| {
            let target = neighbor(&previous_items, value, false, loop_focus);
            move_navigation_focus(view, cx, &access, root, &previous_items, target);
        });
        let first_items = items.clone();
        let access = access_source.clone();
        let first = cx.action_listener(id, move |view, _: &NavigationMenuFirst, cx| {
            let target = edge(&first_items, false);
            move_navigation_focus(view, cx, &access, root, &first_items, target);
        });
        let last_items = items.clone();
        let access = access_source.clone();
        let last = cx.action_listener(id, move |view, _: &NavigationMenuLast, cx| {
            let target = edge(&last_items, true);
            move_navigation_focus(view, cx, &access, root, &last_items, target);
        });
        let access = access_source;
        let close = cx.action_listener(id, move |view, _: &NavigationMenuClose, cx| {
            if access.get(view).close() {
                cx.focus(FocusHandle::new(id));
                cx.invalidate();
            } else {
                cx.propagate();
            }
        });

        trigger
            .on_action(next)
            .on_action(previous)
            .on_action(first)
            .on_action(last)
            .on_action(close)
    }

    /// Build this trigger's click behavior, which toggles its panel.
    pub fn on_trigger_click<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut NavigationMenuState,
        on_value_change: Change,
    ) -> ClickListener<V>
    where
        Change: Fn(&mut V, Option<ElementId>, &mut EventContext) + 'static,
    {
        self.on_trigger_click_with(cx, StateAccessor::from(access), on_value_change)
    }

    /// Build this trigger's click behavior against a per-instance state accessor.
    pub fn on_trigger_click_with<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, NavigationMenuState>,
        on_value_change: Change,
    ) -> ClickListener<V>
    where
        Change: Fn(&mut V, Option<ElementId>, &mut EventContext) + 'static,
    {
        let value = self.item.value;
        let index = self.index;
        cx.listener(self.trigger_id(), move |view, cx| {
            if access.get(view).toggle(value, index) {
                let open = access.get(view).value();
                on_value_change(view, open, cx);
                cx.invalidate();
            }
        })
    }

    /// Build this trigger's hover behavior.
    pub fn on_trigger_hover<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut NavigationMenuState,
        on_value_change: Change,
    ) -> HoverListener<V>
    where
        Change: Fn(&mut V, Option<ElementId>, &mut EventContext) + 'static,
    {
        self.on_trigger_hover_with(cx, StateAccessor::from(access), on_value_change)
    }

    /// Build this trigger's hover behavior against a per-instance state accessor.
    pub fn on_trigger_hover_with<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, NavigationMenuState>,
        on_value_change: Change,
    ) -> HoverListener<V>
    where
        Change: Fn(&mut V, Option<ElementId>, &mut EventContext) + 'static,
    {
        let value = self.item.value;
        let index = self.index;
        cx.hover_listener(self.trigger_id(), move |view, hovered, cx| {
            let hovered = *hovered;
            let before = access.get(view).value();
            if access
                .get(view)
                .hover_trigger(value, index, hovered, Instant::now())
            {
                let after = access.get(view).value();
                if before != after {
                    on_value_change(view, after, cx);
                }
                cx.invalidate();
            }
        })
    }

    /// Build the popup's hover behavior, which keeps an open panel open.
    pub fn on_popup_hover<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut NavigationMenuState,
        on_value_change: Change,
    ) -> HoverListener<V>
    where
        Change: Fn(&mut V, Option<ElementId>, &mut EventContext) + 'static,
    {
        self.on_popup_hover_with(cx, StateAccessor::from(access), on_value_change)
    }

    /// Build the popup's hover behavior against a per-instance state accessor.
    pub fn on_popup_hover_with<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, NavigationMenuState>,
        on_value_change: Change,
    ) -> HoverListener<V>
    where
        Change: Fn(&mut V, Option<ElementId>, &mut EventContext) + 'static,
    {
        cx.hover_listener(self.popup_id(), move |view, hovered, cx| {
            let hovered = *hovered;
            let before = access.get(view).value();
            if access.get(view).hover_popup(hovered, Instant::now()) {
                let after = access.get(view).value();
                if before != after {
                    on_value_change(view, after, cx);
                }
                cx.invalidate();
            }
        })
    }

    /// Build the popup's dismissal behavior for Escape and outside presses.
    pub fn on_dismiss<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut NavigationMenuState,
        on_value_change: Change,
    ) -> DismissListener<V>
    where
        Change: Fn(&mut V, Option<ElementId>, &mut EventContext) + 'static,
    {
        self.on_dismiss_with(cx, StateAccessor::from(access), on_value_change)
    }

    /// Build the popup's dismissal behavior against a per-instance state accessor.
    pub fn on_dismiss_with<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, NavigationMenuState>,
        on_value_change: Change,
    ) -> DismissListener<V>
    where
        Change: Fn(&mut V, Option<ElementId>, &mut EventContext) + 'static,
    {
        cx.dismiss_listener(self.popup_id(), move |view, cx| {
            if access.get(view).close() {
                on_value_change(view, None, cx);
                cx.invalidate();
            }
        })
    }

    fn derived(self, tag: u64) -> ElementId {
        derived_navigation_menu_id(self.menu.root_id, tag, self.item.value)
    }
}

fn move_navigation_focus<V: 'static>(
    view: &mut V,
    cx: &mut EventContext,
    access: &StateAccessor<V, NavigationMenuState>,
    root: ElementId,
    items: &[NavigationMenuItem],
    target: Option<ElementId>,
) {
    let Some(target) = target else {
        return;
    };
    let Some(index) = items.iter().position(|item| item.value == target) else {
        return;
    };
    debug_assert!(index < items.len());
    let changed = access.get(view).focus(target);
    cx.focus(FocusHandle::new(derived_navigation_menu_id(
        root,
        NAVIGATION_MENU_TRIGGER_ID_TAG,
        target,
    )));
    if changed {
        cx.invalidate();
    }
}

fn neighbor(
    items: &[NavigationMenuItem],
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

fn edge(items: &[NavigationMenuItem], last: bool) -> Option<ElementId> {
    if last {
        items.iter().rev().find(|item| !item.disabled)
    } else {
        items.iter().find(|item| !item.disabled)
    }
    .map(|item| item.value)
}

/// Create an unstyled navigation-menu landmark root.
///
/// This shorthand is equivalent to `NavigationMenu::new(id, state, items).root_with(div())`.
pub fn navigation_menu(
    id: impl Into<ElementId>,
    state: &NavigationMenuState,
    items: &[NavigationMenuItem],
) -> Element {
    NavigationMenu::new(id, state, items).root_with(div())
}

fn derived_navigation_menu_id(scope: ElementId, tag: u64, value: ElementId) -> ElementId {
    let mut hash = scope
        .as_u64()
        .rotate_left(7)
        .wrapping_add(value.as_u64().rotate_right(23))
        ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == u64::MAX || hash == scope.as_u64() {
        hash ^= tag.rotate_left(11);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Application, Color, IntoElement, KeyContext, View, WindowOptions, button, text};

    fn items() -> [NavigationMenuItem; 3] {
        [
            NavigationMenuItem::new("products"),
            NavigationMenuItem::new("solutions"),
            NavigationMenuItem::new("company").disabled(true),
        ]
    }

    #[test]
    fn hover_and_click_use_exact_deadlines_and_report_direction() {
        let start = Instant::now();
        let mut state = NavigationMenuState::new();
        assert_eq!(state.open_delay(), DEFAULT_NAVIGATION_MENU_DELAY);
        assert_eq!(
            state.close_delay_value(),
            DEFAULT_NAVIGATION_MENU_CLOSE_DELAY
        );
        assert_eq!(state.value(), None);
        assert_eq!(state.next_deadline(), None);

        assert!(state.hover_trigger("products", 0, true, start));
        assert!(!state.hover_trigger("products", 0, true, start));
        assert_eq!(state.value(), None);
        assert_eq!(
            state.next_deadline(),
            Some(start + DEFAULT_NAVIGATION_MENU_DELAY)
        );
        assert!(!state.poll(start));
        assert!(state.poll(start + DEFAULT_NAVIGATION_MENU_DELAY));
        assert!(state.is_open("products"));
        assert_eq!(
            state.activation_direction(),
            NavigationMenuActivationDirection::None
        );

        // With one panel open, moving onto another trigger switches at once and reports a
        // direction the application can animate.
        let later = start + Duration::from_secs(1);
        assert!(state.hover_trigger("products", 0, false, later));
        assert!(state.hover_trigger("solutions", 1, true, later));
        assert!(state.is_open("solutions"));
        assert_eq!(
            state.activation_direction(),
            NavigationMenuActivationDirection::Right
        );
        assert!(state.hover_trigger("solutions", 1, false, later));
        assert!(state.hover_trigger("products", 0, true, later));
        assert_eq!(
            state.activation_direction(),
            NavigationMenuActivationDirection::Left
        );

        // Leaving both the trigger and the popup arms the exact close deadline.
        assert!(state.hover_popup(true, later));
        assert!(state.hover_trigger("products", 0, false, later));
        assert_eq!(state.next_deadline(), None);
        assert!(state.hover_popup(false, later));
        assert_eq!(
            state.next_deadline(),
            Some(later + DEFAULT_NAVIGATION_MENU_CLOSE_DELAY)
        );
        assert!(state.poll(later + DEFAULT_NAVIGATION_MENU_CLOSE_DELAY));
        assert_eq!(state.value(), None);

        assert!(state.toggle("solutions", 1));
        assert!(state.is_open("solutions"));
        assert!(state.toggle("solutions", 1));
        assert_eq!(state.value(), None);
        assert!(!state.close());
        assert!(state.focus("products"));
        assert!(!state.focus("products"));
        assert_eq!(state.focused(), Some("products".into()));

        let mut vertical = NavigationMenuState::new().vertical();
        assert_eq!(vertical.axis(), NavigationMenuOrientation::Vertical);
        vertical.open("products", 0);
        vertical.open("solutions", 1);
        assert_eq!(
            vertical.activation_direction(),
            NavigationMenuActivationDirection::Down
        );
        vertical.open("products", 0);
        assert_eq!(
            vertical.activation_direction(),
            NavigationMenuActivationDirection::Up
        );

        let mut instant = NavigationMenuState::new()
            .delay(Duration::ZERO)
            .close_delay(Duration::ZERO);
        assert!(instant.hover_trigger("products", 0, true, start));
        assert!(instant.is_open("products"));
        assert!(instant.hover_trigger("products", 0, false, start));
        assert_eq!(instant.value(), None);

        let clamped = NavigationMenuState::new()
            .delay(Duration::from_secs(600))
            .close_delay(Duration::from_secs(600));
        assert_eq!(clamped.open_delay(), MAX_NAVIGATION_MENU_DELAY);
        assert_eq!(clamped.close_delay_value(), MAX_NAVIGATION_MENU_DELAY);
        assert_eq!(NavigationMenuState::default(), NavigationMenuState::new());
    }

    #[test]
    fn parts_add_exact_semantics_without_appearance() {
        let items = items();
        let mut state = NavigationMenuState::new();
        state.open("products", 0);
        let menu = NavigationMenu::new("main", &state, &items);

        let root = menu.root_with(div().bg(Color::rgb8(1, 2, 3)));
        assert_eq!(root.explicit_id, Some("main".into()));
        assert_eq!(root.accessibility.role, AccessibilityRole::Navigation);
        assert_eq!(root.visual.background, Some(Color::rgb8(1, 2, 3)));

        let list = menu.list_with(div());
        assert_eq!(list.accessibility.role, AccessibilityRole::List);
        assert_eq!(
            list.accessibility.orientation,
            Some(AccessibilityOrientation::Horizontal)
        );

        let entry = menu.entry("products").expect("declared item");
        assert!(entry.is_open());
        assert_eq!(entry.index(), 0);
        assert!(entry.is_roving_stop());
        let item = entry.item_with(div());
        assert_eq!(item.explicit_id, Some(entry.item_id()));
        assert_eq!(item.accessibility.role, AccessibilityRole::ListItem);
        assert_eq!(item.accessibility.collection.position_in_set, 0);
        assert_eq!(item.accessibility.collection.size_of_set, 3);

        let trigger = entry.trigger_with(div());
        assert_eq!(trigger.explicit_id, Some(entry.trigger_id()));
        assert_eq!(trigger.accessibility.role, AccessibilityRole::Button);
        assert_eq!(
            trigger.accessibility.has_popover,
            Some(AccessibilityPopover::Menu)
        );
        assert_eq!(trigger.accessibility.expanded, Some(true));
        assert_eq!(
            trigger.accessibility.relations.controls(),
            Some(entry.popup_id())
        );
        assert_eq!(trigger.tab_index, 0);
        assert_eq!(trigger.visual.background, None);

        let icon = entry.icon_with(div());
        assert_eq!(icon.explicit_id, Some(entry.icon_id()));
        assert!(icon.accessibility.hidden);

        let popup = entry.popup_with(div());
        assert_eq!(popup.explicit_id, Some(entry.popup_id()));
        assert_eq!(popup.accessibility.role, AccessibilityRole::Menu);
        assert!(popup.dismiss_policy.on_escape());
        let positioner = entry.positioner_with(div());
        assert_eq!(positioner.explicit_id, Some(entry.positioner_id()));
        assert_eq!(
            entry.portal_with(div()).explicit_id,
            Some(entry.positioner_id())
        );
        assert_eq!(
            entry.viewport_with(div()).explicit_id,
            Some(entry.viewport_id())
        );
        let content = entry.content_with(div());
        assert_eq!(content.explicit_id, Some(entry.content_id()));
        assert_eq!(
            content.accessibility.relations.labelled_by(),
            Some(entry.trigger_id())
        );
        assert!(entry.arrow_with(div()).accessibility.hidden);
        assert_eq!(
            entry.backdrop_with(div()).explicit_id,
            Some(entry.backdrop_id())
        );

        let other = menu.entry("solutions").expect("declared item");
        assert!(!other.is_open());
        assert_eq!(other.trigger_with(div()).tab_index, -1);
        let disabled = menu.entry("company").expect("declared item");
        assert!(disabled.is_disabled());
        assert!(disabled.trigger_with(div()).accessibility.disabled);
        assert!(menu.entry("missing").is_none());

        let link = menu.link_with("pricing", true, div());
        assert_eq!(link.explicit_id, Some("pricing".into()));
        assert_eq!(link.accessibility.role, AccessibilityRole::Link);
        assert!(link.accessibility.selected);

        let ids = [
            menu.root_id(),
            entry.item_id(),
            entry.trigger_id(),
            entry.icon_id(),
            entry.popup_id(),
            entry.content_id(),
            entry.viewport_id(),
            entry.arrow_id(),
            entry.positioner_id(),
            entry.backdrop_id(),
            other.trigger_id(),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(*id, ElementId::new(0));
            assert_ne!(*id, ElementId::new(u64::MAX));
            assert!(!ids[..index].contains(id));
        }

        let shorthand = navigation_menu("main", &state, &items);
        assert_eq!(shorthand.accessibility.role, AccessibilityRole::Navigation);
        assert!(shorthand.children.is_empty());
    }

    #[test]
    #[should_panic(expected = "at most")]
    fn oversized_menus_are_rejected() {
        let items: Vec<NavigationMenuItem> = (0..=MAX_NAVIGATION_MENU_ITEMS)
            .map(|index| NavigationMenuItem::new(ElementId::new(index as u64 + 1)))
            .collect();
        let _ = NavigationMenu::new("main", &NavigationMenuState::new(), &items);
    }

    #[test]
    #[should_panic(expected = "distinct")]
    fn duplicate_items_are_rejected() {
        let items = [NavigationMenuItem::new("a"), NavigationMenuItem::new("a")];
        let _ = NavigationMenu::new("main", &NavigationMenuState::new(), &items);
    }

    #[test]
    fn bindings_are_contextual_and_complete() {
        let bindings = navigation_menu_key_bindings();
        assert_eq!(bindings.len(), 10);
        assert!(bindings.iter().all(|binding| {
            binding.context_predicate().is_some_and(|context| {
                context
                    .depth_of(&[KeyContext::parse(NAVIGATION_MENU_HORIZONTAL_KEY_CONTEXT).unwrap()])
                    .is_some()
                    || context
                        .depth_of(&[
                            KeyContext::parse(NAVIGATION_MENU_VERTICAL_KEY_CONTEXT).unwrap()
                        ])
                        .is_some()
            })
        }));
    }

    struct NavigationView {
        menu: NavigationMenuState,
        changes: Vec<Option<ElementId>>,
    }

    impl NavigationView {
        fn menu(view: &mut Self) -> &mut NavigationMenuState {
            &mut view.menu
        }
    }

    impl View for NavigationView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let _ = self.menu.poll(Instant::now());
            NavigationMenu::schedule(cx, &self.menu);
            let items = items();
            let menu = NavigationMenu::new("main", &self.menu, &items);
            let mut list = menu.list_with(div().flex_row());
            let mut panels = div();
            for item in items.iter() {
                let entry = menu.entry(item.value()).expect("declared item");
                let click = entry.on_trigger_click(cx, Self::menu, |view, value, _| {
                    view.changes.push(value);
                });
                let hover = entry.on_trigger_hover(cx, Self::menu, |view, value, _| {
                    view.changes.push(value);
                });
                let dismiss = entry.on_dismiss(cx, Self::menu, |view, value, _| {
                    view.changes.push(value);
                });
                let popup_hover = entry.on_popup_hover(cx, Self::menu, |view, value, _| {
                    view.changes.push(value);
                });
                let trigger = entry.key_with(
                    cx,
                    entry
                        .trigger_with(button().child(text("Menu")))
                        .on_click(click)
                        .on_hover(hover),
                    Self::menu,
                );
                list = list.child(
                    entry
                        .item_with(div())
                        .child(trigger)
                        .child(entry.icon_with(div())),
                );
                if entry.is_open() {
                    panels = panels.child(
                        entry.positioner_with(div()).child(
                            entry
                                .popup_with(div().w(240.0).h(120.0))
                                .on_hover(popup_hover)
                                .on_dismiss(dismiss)
                                .child(entry.viewport_with(div()).child(
                                    entry.content_with(div()).child(menu.link_with(
                                        "pricing",
                                        false,
                                        div(),
                                    )),
                                ))
                                .child(entry.arrow_with(div())),
                        ),
                    );
                }
            }
            menu.root_with(div().size_full().relative())
                .child(list)
                .child(panels)
        }
    }

    #[test]
    fn triggers_open_navigate_and_close_without_idle_work() {
        let (mut cx, view) = Application::new()
            .bind_keys(navigation_menu_key_bindings())
            .into_test_context(
                WindowOptions::default(),
                NavigationView {
                    menu: NavigationMenuState::new(),
                    changes: Vec::new(),
                },
            )
            .unwrap();
        let window = view.window_handle();
        let items = items();
        let state = NavigationMenuState::new();
        let menu = NavigationMenu::new("main", &state, &items);
        let products = menu.entry("products").expect("declared item");
        let solutions = menu.entry("solutions").expect("declared item");

        cx.click(window, products.trigger_id()).unwrap();
        assert_eq!(
            cx.read(view, |view| view.menu.value()).unwrap(),
            Some("products".into())
        );
        assert!(cx.contains_element(window, products.popup_id()).unwrap());

        // Arrow navigation moves the bar's single Tab stop and leaves the open panel alone.
        cx.focus(window, products.trigger_id()).unwrap();
        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(solutions.trigger_id()));
        assert_eq!(
            cx.read(view, |view| view.menu.value()).unwrap(),
            Some("products".into())
        );
        // The disabled item is skipped, wrapping back to the first trigger.
        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(products.trigger_id()));
        cx.simulate_keystrokes(window, "end").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(solutions.trigger_id()));
        cx.simulate_keystrokes(window, "home").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(products.trigger_id()));

        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("navigation menu accessibility node")
        };
        assert_eq!(node(menu.root_id()).role(), accesskit::Role::Navigation);
        let trigger = node(products.trigger_id());
        assert_eq!(trigger.role(), accesskit::Role::Button);
        assert_eq!(trigger.is_expanded(), Some(true));
        assert_eq!(node(products.popup_id()).role(), accesskit::Role::Menu);

        cx.simulate_keystrokes(window, "escape").unwrap();
        assert_eq!(cx.read(view, |view| view.menu.value()).unwrap(), None);
        assert_eq!(
            cx.focused(window).unwrap(),
            Some(products.trigger_id()),
            "Escape closes without leaving the bar"
        );

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
