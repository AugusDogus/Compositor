use crate::{AccessibilityRole, Element, ElementId};

use crate::element::{AccessibilityOrientation, TabListBehavior};

const TABS_LIST_ID_TAG: u64 = 0x80bc_1828_48ed_58bf;
const TAB_ID_TAG: u64 = 0xf4a8_90a4_2ce9_3357;
const TAB_PANEL_ID_TAG: u64 = 0x37d8_909d_30bb_71c1;
const TAB_INDICATOR_ID_TAG: u64 = 0x11c4_1646_4527_d985;

/// Layout and keyboard axis for one unstyled tab set.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TabsOrientation {
    #[default]
    Horizontal,
    Vertical,
}

/// Allocation-free controlled active value for one tab set.
///
/// QuickGUI deliberately does not retain or discover application values outside the mounted tree.
/// The application owns this state, mutates it only from direct interaction, and rebuilds the
/// descriptor from [`Tabs::from_state`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TabsState {
    active: Option<ElementId>,
    active_index: Option<usize>,
    movement: TabsActivationMovement,
}

impl TabsState {
    pub fn new(active: impl Into<ElementId>) -> Self {
        Self {
            active: Some(active.into()),
            active_index: None,
            movement: TabsActivationMovement::None,
        }
    }

    pub const fn empty() -> Self {
        Self {
            active: None,
            active_index: None,
            movement: TabsActivationMovement::None,
        }
    }

    pub const fn active(&self) -> Option<ElementId> {
        self.active
    }

    /// The position the active tab was last selected at, when the application supplied one.
    pub const fn active_index(&self) -> Option<usize> {
        self.active_index
    }

    /// Which way the selection last moved along the tab list.
    ///
    /// This is the raw half of Base UI's `data-activation-direction`; [`Tabs::activation_direction`]
    /// turns it into a side using the declared orientation. It stays
    /// [`TabsActivationMovement::None`] until the application selects through
    /// [`Self::select_at`], because a bare value carries no ordering.
    pub const fn activation_movement(&self) -> TabsActivationMovement {
        self.movement
    }

    /// Replace the controlled value, returning whether it changed.
    ///
    /// The activation movement is cleared: a value on its own says nothing about which way the
    /// selection travelled. Use [`Self::select_at`] to record it.
    pub fn set_active(&mut self, active: Option<ElementId>) -> bool {
        self.movement = TabsActivationMovement::None;
        self.active_index = None;
        if self.active == active {
            false
        } else {
            self.active = active;
            true
        }
    }

    pub fn select(&mut self, value: impl Into<ElementId>) -> bool {
        self.set_active(Some(value.into()))
    }

    /// Select one tab by value and position, recording which way the selection moved.
    ///
    /// The application already knows the order it declares its tabs in, so it passes the index it
    /// is rendering; QuickGUI keeps no item registry to discover it from. Returns whether the value
    /// changed.
    pub fn select_at(&mut self, value: impl Into<ElementId>, index: usize) -> bool {
        let value = value.into();
        self.movement = match self.active_index {
            Some(previous) if index > previous => TabsActivationMovement::Forward,
            Some(previous) if index < previous => TabsActivationMovement::Backward,
            _ => TabsActivationMovement::None,
        };
        self.active_index = Some(index);
        if self.active == Some(value) {
            false
        } else {
            self.active = Some(value);
            true
        }
    }

    pub fn clear(&mut self) -> bool {
        self.set_active(None)
    }
}

/// Which way the selection last travelled along a tab list.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TabsActivationMovement {
    /// Nothing has moved, or the selection was set without an ordering.
    #[default]
    None,
    /// The selection moved toward the end of the list.
    Forward,
    /// The selection moved toward the start of the list.
    Backward,
}

/// The side the selection last travelled toward, Base UI's `data-activation-direction`.
///
/// An application animates a sliding indicator or a panel transition from this instead of
/// re-deriving it from its own previous render.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TabsActivationDirection {
    #[default]
    None,
    Left,
    Right,
    Up,
    Down,
}

/// The geometry of the active tab, for an application-animated indicator.
///
/// Base UI computes `--active-tab-left` and friends by measuring the DOM. QuickGUI reports the
/// rectangle the retained tree actually laid the active tab out at, in window logical coordinates,
/// through the same [`crate::AnchorPlacementHandle`] a popover uses. Width and height are directly
/// usable; positions are most useful as a frame-to-frame delta, and an indicator that is simply
/// anchored to the active tab needs no arithmetic at all.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TabsIndicatorGeometry {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// A controlled, unstyled in-window tab-set descriptor.
///
/// The application owns the active value, all content, layout, typography, colors, indicator
/// geometry, and motion. QuickGUI supplies stable part identities, one roving Tab stop, bounded
/// arrow/Home/End navigation, optional activation on arrow focus, disabled-item skipping, exact
/// tab/list/panel accessibility semantics, and optional hidden panel retention.
///
/// Manual activation and looping focus match Base UI's defaults. This descriptor retains no item
/// registry, allocation, task, timer, observer, animation, or idle scheduler source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a Tabs descriptor has no effect until its parts are mounted"]
pub struct Tabs {
    root_id: ElementId,
    active: Option<ElementId>,
    orientation: TabsOrientation,
    activate_on_focus: bool,
    loop_focus: bool,
    keep_mounted: bool,
    movement: TabsActivationMovement,
}

impl Tabs {
    pub fn new(root_id: impl Into<ElementId>, active: impl Into<ElementId>) -> Self {
        Self {
            root_id: root_id.into(),
            active: Some(active.into()),
            orientation: TabsOrientation::Horizontal,
            activate_on_focus: false,
            loop_focus: true,
            keep_mounted: false,
            movement: TabsActivationMovement::None,
        }
    }

    pub fn without_selection(root_id: impl Into<ElementId>) -> Self {
        Self {
            root_id: root_id.into(),
            active: None,
            orientation: TabsOrientation::Horizontal,
            activate_on_focus: false,
            loop_focus: true,
            keep_mounted: false,
            movement: TabsActivationMovement::None,
        }
    }

    pub fn from_state(root_id: impl Into<ElementId>, state: &TabsState) -> Self {
        let mut tabs = Self::without_selection(root_id);
        tabs.active = state.active();
        tabs.movement = state.activation_movement();
        tabs
    }

    pub const fn orientation(mut self, orientation: TabsOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub const fn vertical(self) -> Self {
        self.orientation(TabsOrientation::Vertical)
    }

    /// Activate an enabled tab as arrow/Home/End navigation moves focus to it.
    ///
    /// The default is manual activation: arrows move focus and Enter or Space invokes the tab's
    /// ordinary click listener.
    pub const fn activate_on_focus(mut self, activate_on_focus: bool) -> Self {
        self.activate_on_focus = activate_on_focus;
        self
    }

    pub const fn loop_focus(mut self, loop_focus: bool) -> Self {
        self.loop_focus = loop_focus;
        self
    }

    /// Retain inactive panels as `display: none` instead of omitting them from the mounted tree.
    pub const fn keep_mounted(mut self, keep_mounted: bool) -> Self {
        self.keep_mounted = keep_mounted;
        self
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub fn list_id(self) -> ElementId {
        derived_tabs_id(self.root_id, self.root_id, TABS_LIST_ID_TAG)
    }

    pub const fn active_value(self) -> Option<ElementId> {
        self.active
    }

    pub const fn activates_on_focus(self) -> bool {
        self.activate_on_focus
    }

    pub const fn loops_focus(self) -> bool {
        self.loop_focus
    }

    pub const fn keeps_panels_mounted(self) -> bool {
        self.keep_mounted
    }

    /// Declare which way the selection travelled, for a descriptor built without a [`TabsState`].
    pub const fn activation_movement(mut self, movement: TabsActivationMovement) -> Self {
        self.movement = movement;
        self
    }

    /// The side the selection last travelled toward, Base UI's `data-activation-direction`.
    ///
    /// A horizontal tab list reports `Left` or `Right`, a vertical one `Up` or `Down`, and both
    /// report `None` until the application selects through [`TabsState::select_at`].
    pub const fn activation_direction(self) -> TabsActivationDirection {
        match (self.movement, self.orientation) {
            (TabsActivationMovement::None, _) => TabsActivationDirection::None,
            (TabsActivationMovement::Forward, TabsOrientation::Horizontal) => {
                TabsActivationDirection::Right
            }
            (TabsActivationMovement::Backward, TabsOrientation::Horizontal) => {
                TabsActivationDirection::Left
            }
            (TabsActivationMovement::Forward, TabsOrientation::Vertical) => {
                TabsActivationDirection::Down
            }
            (TabsActivationMovement::Backward, TabsOrientation::Vertical) => {
                TabsActivationDirection::Up
            }
        }
    }

    /// Read back the active tab's laid-out geometry from a bound indicator handle.
    ///
    /// Mount the indicator with [`Tab::tracked_indicator_with`] and pass the same handle here on
    /// the next frame. The rectangle is the active tab's own, in window logical coordinates, and is
    /// `None` until the indicator has been painted once.
    pub fn indicator_geometry(
        self,
        handle: &crate::AnchorPlacementHandle,
    ) -> Option<TabsIndicatorGeometry> {
        let resolved = handle.resolved()?;
        Some(TabsIndicatorGeometry {
            left: resolved.anchor.x,
            top: resolved.anchor.y,
            width: resolved.anchor.width,
            height: resolved.anchor.height,
        })
    }

    /// Decorate an application-owned structural root without adding role or appearance.
    pub fn root_with(self, root: Element) -> Element {
        root.id(self.root_id)
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate an application-owned tab-list root.
    pub fn list_with(self, list: Element) -> Element {
        let mut list = list
            .id(self.list_id())
            .accessibility_role(AccessibilityRole::TabList);
        let vertical = self.orientation == TabsOrientation::Vertical;
        list.accessibility.orientation = Some(if vertical {
            AccessibilityOrientation::Vertical
        } else {
            AccessibilityOrientation::Horizontal
        });
        list.tab_list_behavior = Some(TabListBehavior {
            vertical,
            activate_on_focus: self.activate_on_focus,
            loop_focus: self.loop_focus,
        });
        list
    }
    /// Create the unstyled list part. Use [`Self::list_with`] to supply an existing element.
    pub fn list(self) -> Element {
        self.list_with(crate::div())
    }

    pub fn tab(self, value: impl Into<ElementId>) -> Tab {
        let value = value.into();
        Tab {
            tabs_id: self.root_id,
            value,
            state: TabState {
                active: self.active == Some(value),
                disabled: false,
                orientation: self.orientation,
            },
            keep_mounted: self.keep_mounted,
            direction: self.activation_direction(),
        }
    }
}

/// Caller-visible state projected across one tab's unstyled parts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TabState {
    pub active: bool,
    pub disabled: bool,
    pub orientation: TabsOrientation,
}

/// A copyable declaration for one controlled tab and its associated panel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a Tab descriptor has no effect until one of its parts is mounted"]
pub struct Tab {
    tabs_id: ElementId,
    value: ElementId,
    state: TabState,
    keep_mounted: bool,
    direction: TabsActivationDirection,
}

impl Tab {
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.state.disabled = disabled;
        self
    }

    pub const fn keep_mounted(mut self, keep_mounted: bool) -> Self {
        self.keep_mounted = keep_mounted;
        self
    }

    pub const fn value(self) -> ElementId {
        self.value
    }

    pub const fn state(self) -> TabState {
        self.state
    }

    pub const fn is_active(self) -> bool {
        self.state.active
    }

    pub const fn is_disabled(self) -> bool {
        self.state.disabled
    }

    pub const fn panel_is_mounted(self) -> bool {
        self.state.active || self.keep_mounted
    }

    pub fn tab_id(self) -> ElementId {
        derived_tabs_id(self.tabs_id, self.value, TAB_ID_TAG)
    }

    pub fn panel_id(self) -> ElementId {
        derived_tabs_id(self.tabs_id, self.value, TAB_PANEL_ID_TAG)
    }

    pub fn indicator_id(self) -> ElementId {
        derived_tabs_id(self.tabs_id, self.value, TAB_INDICATOR_ID_TAG)
    }

    /// Decorate an application-owned tab button without adding appearance.
    pub fn tab_with(self, tab: Element) -> Element {
        let disabled = self.state.disabled || tab.accessibility.disabled;
        tab.id(self.tab_id())
            .accessibility_role(AccessibilityRole::Tab)
            .selected(self.state.active)
            .accessibility_controls(self.panel_id())
            .clickable()
            .tab_index(0)
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
            .disabled(disabled)
    }
    /// Create the unstyled tab part. Use [`Self::tab_with`] to supply an existing element.
    pub fn tab(self) -> Element {
        self.tab_with(crate::button())
    }

    /// Mount a caller-owned decorative indicator only for the active tab.
    ///
    /// Indicator geometry and motion are intentionally application-owned. Put this part inside the
    /// tab or position it absolutely in the caller's list layout.
    pub fn indicator_with(self, indicator: Element) -> Option<Element> {
        self.state
            .active
            .then(|| indicator.id(self.indicator_id()).accessibility_hidden(true))
    }
    /// Create the unstyled indicator part. Use [`Self::indicator_with`] to supply an existing element.
    pub fn indicator(self) -> Option<Element> {
        self.indicator_with(crate::div())
    }

    /// The side the selection travelled toward when this tab became active.
    pub const fn activation_direction(self) -> TabsActivationDirection {
        self.direction
    }

    /// Mount an indicator the framework keeps positioned on the active tab.
    ///
    /// The indicator is anchored to the tab, so QuickGUI's existing placement keeps it aligned
    /// without the application re-deriving the tab's box every frame. `placement` chooses the edge:
    /// `AnchorPlacement::Bottom` draws the familiar underline. Size, colour, radius, and motion stay
    /// application-owned, and the part is still mounted only for the active tab.
    pub fn anchored_indicator_with(
        self,
        indicator: Element,
        placement: crate::AnchorPlacement,
    ) -> Option<Element> {
        self.indicator_with(indicator).map(|indicator| {
            indicator
                .anchor_to(self.tab_id(), placement)
                .anchor_gap(0.0)
                // An indicator belongs to its tab, not to the window: it must never be nudged off
                // the tab by a collision margin, and it travels off screen with a scrolled list.
                .viewport_margin(0.0)
                .anchor_sticky(false)
        })
    }
    /// Create the unstyled anchored indicator part. Use [`Self::anchored_indicator_with`] to supply an existing element.
    pub fn anchored_indicator(self, placement: crate::AnchorPlacement) -> Option<Element> {
        self.anchored_indicator_with(crate::div(), placement)
    }

    /// Mount an anchored indicator that also publishes the active tab's laid-out geometry.
    ///
    /// Read it back with [`Tabs::indicator_geometry`] on the next frame. QuickGUI writes the handle
    /// during the paint it was already performing and requests exactly one correcting frame when
    /// the geometry changes, so a settled tab list adds no redraw source.
    pub fn tracked_indicator_with(
        self,
        indicator: Element,
        placement: crate::AnchorPlacement,
        geometry: &crate::AnchorPlacementHandle,
    ) -> Option<Element> {
        self.anchored_indicator_with(
            indicator.report_anchor_placement(geometry.clone()),
            placement,
        )
    }
    /// Create the unstyled tracked indicator part. Use [`Self::tracked_indicator_with`] to supply an existing element.
    pub fn tracked_indicator(
        self,
        placement: crate::AnchorPlacement,
        geometry: &crate::AnchorPlacementHandle,
    ) -> Option<Element> {
        self.tracked_indicator_with(crate::div(), placement, geometry)
    }

    /// Decorate and mount this tab's application-owned panel.
    ///
    /// The active panel is focusable so Tab can move from the composite tab list into panel
    /// content even when that content has no focusable first child. Inactive retained panels use
    /// `display: none`, contributing no layout, paint, input, accessibility node, or runtime work.
    pub fn panel_with(self, panel: Element) -> Option<Element> {
        let panel = panel
            .id(self.panel_id())
            .accessibility_role(AccessibilityRole::TabPanel)
            .accessibility_labelled_by(self.tab_id())
            .focusable();
        if self.state.active {
            Some(panel)
        } else if self.keep_mounted {
            Some(panel.hidden())
        } else {
            None
        }
    }
    /// Create the unstyled panel part. Use [`Self::panel_with`] to supply an existing element.
    pub fn panel(self) -> Option<Element> {
        self.panel_with(crate::div())
    }
}

fn derived_tabs_id(scope: ElementId, value: ElementId, tag: u64) -> ElementId {
    let mut hash = scope
        .as_u64()
        .rotate_left(19)
        .wrapping_add(value.as_u64().rotate_right(7))
        ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == u64::MAX || hash == scope.as_u64() || hash == value.as_u64() {
        hash ^= tag.rotate_left(29);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppRegion, Color, CursorStyle, Insets, IntoElement, TestAppContext, UserSelect, View,
        ViewContext, button, div, text,
    };

    #[test]
    fn state_and_parts_are_bounded_unstyled_and_exact() {
        let mut state = TabsState::new("overview");
        assert_eq!(state.active(), Some("overview".into()));
        assert!(!state.select("overview"));
        assert!(state.select("files"));
        assert!(state.clear());
        assert!(!state.clear());

        let tabs = Tabs::new("workspace", "overview")
            .vertical()
            .activate_on_focus(true)
            .loop_focus(false)
            .keep_mounted(true);
        assert_eq!(tabs.active_value(), Some("overview".into()));
        assert!(tabs.activates_on_focus());
        assert!(!tabs.loops_focus());
        assert!(tabs.keeps_panels_mounted());

        let root = tabs.root_with(div().w(317.0).bg(Color::rgb8(1, 2, 3)));
        assert_eq!(root.explicit_id, Some(tabs.root_id()));
        assert_eq!(root.visual.background, Some(Color::rgb8(1, 2, 3)));

        let list = tabs.list_with(div().gap_3().border(2.0, Color::rgb8(4, 5, 6)));
        assert_eq!(list.explicit_id, Some(tabs.list_id()));
        assert_eq!(list.accessibility.role, AccessibilityRole::TabList);
        assert_eq!(
            list.accessibility.orientation,
            Some(AccessibilityOrientation::Vertical)
        );
        assert_eq!(
            list.tab_list_behavior,
            Some(TabListBehavior {
                vertical: true,
                activate_on_focus: true,
                loop_focus: false,
            })
        );
        assert_eq!(list.visual.border_widths, Insets::all(2.0));
        assert!(list.key_listeners.is_none());

        let active = tabs.tab("overview");
        assert_eq!(
            active.state(),
            TabState {
                active: true,
                disabled: false,
                orientation: TabsOrientation::Vertical,
            }
        );
        let ids = [active.tab_id(), active.panel_id(), active.indicator_id()];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(*id, tabs.root_id());
            assert_ne!(*id, tabs.list_id());
            assert!(!ids[..index].contains(id));
        }

        let tab = active.tab_with(div().px_4().bg(Color::rgb8(7, 8, 9)));
        assert_eq!(tab.explicit_id, Some(active.tab_id()));
        assert_eq!(tab.accessibility.role, AccessibilityRole::Tab);
        assert!(tab.accessibility.selected);
        assert_eq!(
            tab.accessibility.relations.controls(),
            Some(active.panel_id())
        );
        assert!(tab.clickable);
        assert!(tab.focusable);
        assert_eq!(tab.tab_index, 0);
        assert_eq!(tab.cursor_style, Some(CursorStyle::Arrow));
        assert_eq!(tab.app_region, Some(AppRegion::NoDrag));
        assert_eq!(tab.user_select, UserSelect::None);
        assert_eq!(tab.visual.background, Some(Color::rgb8(7, 8, 9)));
        assert!(tab.transition.is_none());

        let indicator = active
            .indicator_with(div().h(3.0).bg(Color::rgb8(10, 11, 12)))
            .expect("active indicator");
        assert_eq!(indicator.explicit_id, Some(active.indicator_id()));
        assert!(indicator.accessibility.hidden);
        assert_eq!(indicator.visual.background, Some(Color::rgb8(10, 11, 12)));

        let panel = active
            .panel_with(div().bg(Color::rgb8(13, 14, 15)))
            .expect("active panel");
        assert_eq!(panel.explicit_id, Some(active.panel_id()));
        assert_eq!(panel.accessibility.role, AccessibilityRole::TabPanel);
        assert_eq!(
            panel.accessibility.relations.labelled_by(),
            Some(active.tab_id())
        );
        assert!(panel.focusable);
        assert!(!panel.is_display_none());

        let inactive = tabs.tab("files");
        assert!(!inactive.is_active());
        assert!(inactive.indicator_with(div()).is_none());
        assert!(
            inactive
                .panel_with(div())
                .expect("retained inactive panel")
                .is_display_none()
        );
        let disabled = inactive.disabled(true).tab_with(div());
        assert!(disabled.accessibility.disabled);
        assert!(!disabled.accessibility.selected);
    }

    struct TabsView {
        manual: TabsState,
        automatic: TabsState,
    }

    impl Default for TabsView {
        fn default() -> Self {
            Self {
                manual: TabsState::new("overview"),
                automatic: TabsState::new("alpha"),
            }
        }
    }

    impl View for TabsView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let manual = Tabs::from_state("manual-tabs", &self.manual);
            let overview = manual.tab("overview");
            let disabled = manual.tab("disabled").disabled(true);
            let files = manual.tab("files");
            let settings = manual.tab("settings");
            let select_overview = cx.listener(overview.tab_id(), |view, cx| {
                if view.manual.select("overview") {
                    cx.invalidate();
                }
            });
            let select_disabled = cx.listener(disabled.tab_id(), |_view, _cx| {});
            let select_files = cx.listener(files.tab_id(), |view, cx| {
                if view.manual.select("files") {
                    cx.invalidate();
                }
            });
            let select_settings = cx.listener(settings.tab_id(), |view, cx| {
                if view.manual.select("settings") {
                    cx.invalidate();
                }
            });

            let automatic = Tabs::from_state("automatic-tabs", &self.automatic)
                .vertical()
                .activate_on_focus(true);
            let alpha = automatic.tab("alpha");
            let beta = automatic.tab("beta").disabled(true);
            let gamma = automatic.tab("gamma");
            let delta = automatic.tab("delta");
            let select_alpha = cx.listener(alpha.tab_id(), |view, cx| {
                if view.automatic.select("alpha") {
                    cx.invalidate();
                }
            });
            let select_beta = cx.listener(beta.tab_id(), |_view, _cx| {});
            let select_gamma = cx.listener(gamma.tab_id(), |view, cx| {
                if view.automatic.select("gamma") {
                    cx.invalidate();
                }
            });
            let select_delta = cx.listener(delta.tab_id(), |view, cx| {
                if view.automatic.select("delta") {
                    cx.invalidate();
                }
            });

            div()
                .child(button().id("before").child("Before"))
                .child(
                    manual.root_with(
                        div()
                            .child(
                                manual.list_with(
                                    div()
                                        .child(
                                            overview
                                                .tab_with(div().child("Overview"))
                                                .on_click(select_overview),
                                        )
                                        .child(
                                            disabled
                                                .tab_with(div().child("Disabled"))
                                                .on_click(select_disabled),
                                        )
                                        .child(
                                            files
                                                .tab_with(div().child("Files"))
                                                .on_click(select_files),
                                        )
                                        .child(
                                            settings
                                                .tab_with(div().child("Settings"))
                                                .on_click(select_settings),
                                        ),
                                ),
                            )
                            .children(overview.panel_with(text("Overview panel")))
                            .children(disabled.panel_with(text("Disabled panel")))
                            .children(files.panel_with(text("Files panel")))
                            .children(settings.panel_with(text("Settings panel"))),
                    ),
                )
                .child(button().id("between").child("Between"))
                .child(
                    automatic.root_with(
                        div()
                            .child(
                                automatic.list_with(
                                    div()
                                        .child(
                                            alpha
                                                .tab_with(div().child("Alpha"))
                                                .on_click(select_alpha),
                                        )
                                        .child(
                                            beta.tab_with(div().child("Beta"))
                                                .on_click(select_beta),
                                        )
                                        .child(
                                            gamma
                                                .tab_with(div().child("Gamma"))
                                                .on_click(select_gamma),
                                        )
                                        .child(
                                            delta
                                                .tab_with(div().child("Delta"))
                                                .on_click(select_delta),
                                        ),
                                ),
                            )
                            .children(alpha.panel_with(text("Alpha panel")))
                            .children(beta.panel_with(text("Beta panel")))
                            .children(gamma.panel_with(text("Gamma panel")))
                            .children(delta.panel_with(text("Delta panel"))),
                    ),
                )
                .child(button().id("after").child("After"))
        }
    }

    #[test]
    fn keyboard_mounting_accessibility_and_idle_paths_are_deterministic() {
        let (mut cx, view) = TestAppContext::new(TabsView::default()).unwrap();
        let window = view.window_handle();
        let manual = Tabs::new("manual-tabs", "overview");
        let overview = manual.tab("overview");
        let disabled = manual.tab("disabled").disabled(true);
        let files = manual.tab("files");
        let settings = manual.tab("settings");
        let automatic = Tabs::new("automatic-tabs", "alpha")
            .vertical()
            .activate_on_focus(true);
        let alpha = automatic.tab("alpha");
        let beta = automatic.tab("beta").disabled(true);
        let gamma = automatic.tab("gamma");
        let delta = automatic.tab("delta");

        cx.simulate_keystrokes(window, "tab tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(overview.tab_id()));
        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(files.tab_id()));
        assert!(cx.contains_element(window, overview.panel_id()).unwrap());
        assert!(!cx.contains_element(window, files.panel_id()).unwrap());

        // A manually focused inactive tab still represents the whole roving tablist for normal
        // Tab traversal, so focus exits into the active panel instead of restarting the window.
        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(overview.panel_id()));
        cx.focus(window, overview.tab_id()).unwrap();
        cx.simulate_keystrokes(window, "end space").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(settings.tab_id()));
        assert!(!cx.contains_element(window, overview.panel_id()).unwrap());
        assert!(cx.contains_element(window, settings.panel_id()).unwrap());
        cx.simulate_keystrokes(window, "home enter left").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(settings.tab_id()));
        assert!(cx.contains_element(window, overview.panel_id()).unwrap());
        assert!(!cx.contains_element(window, settings.panel_id()).unwrap());
        assert!(cx.click(window, disabled.tab_id()).is_err());

        cx.focus(window, alpha.tab_id()).unwrap();
        cx.simulate_keystrokes(window, "down").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(gamma.tab_id()));
        assert!(!cx.contains_element(window, alpha.panel_id()).unwrap());
        assert!(cx.contains_element(window, gamma.panel_id()).unwrap());
        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(gamma.tab_id()));
        cx.simulate_keystrokes(window, "end").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(delta.tab_id()));
        assert!(cx.contains_element(window, delta.panel_id()).unwrap());
        cx.simulate_keystrokes(window, "down").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(alpha.tab_id()));
        assert!(cx.contains_element(window, alpha.panel_id()).unwrap());

        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("tab accessibility node")
        };
        assert_eq!(node(manual.list_id()).role(), accesskit::Role::TabList);
        assert_eq!(
            node(manual.list_id()).orientation(),
            Some(accesskit::Orientation::Horizontal)
        );
        assert_eq!(node(overview.tab_id()).role(), accesskit::Role::Tab);
        assert_eq!(node(overview.tab_id()).is_selected(), Some(true));
        assert_eq!(node(files.tab_id()).is_selected(), Some(false));
        assert_eq!(
            node(overview.tab_id()).controls(),
            &[accesskit::NodeId(overview.panel_id().as_u64())]
        );
        assert!(node(files.tab_id()).controls().is_empty());
        assert!(node(disabled.tab_id()).is_disabled());
        assert!(!node(disabled.tab_id()).supports_action(accesskit::Action::Click));
        assert_eq!(node(overview.panel_id()).role(), accesskit::Role::TabPanel);
        assert_eq!(
            node(overview.panel_id()).labelled_by(),
            &[accesskit::NodeId(overview.tab_id().as_u64())]
        );
        assert_eq!(
            node(automatic.list_id()).orientation(),
            Some(accesskit::Orientation::Vertical)
        );
        assert_eq!(node(alpha.tab_id()).is_selected(), Some(true));
        assert_eq!(node(gamma.tab_id()).is_selected(), Some(false));
        assert!(node(beta.tab_id()).is_disabled());

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    struct NonLoopingTabsView {
        state: TabsState,
    }

    impl Default for NonLoopingTabsView {
        fn default() -> Self {
            Self {
                state: TabsState::new("first"),
            }
        }
    }

    impl View for NonLoopingTabsView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let tabs = Tabs::from_state("non-looping-tabs", &self.state)
                .activate_on_focus(true)
                .loop_focus(false);
            let first = tabs.tab("first");
            let unavailable = tabs.tab("unavailable").disabled(true);
            let last = tabs.tab("last");
            let select_first = cx.listener(first.tab_id(), |view, cx| {
                if view.state.select("first") {
                    cx.invalidate();
                }
            });
            let select_last = cx.listener(last.tab_id(), |view, cx| {
                if view.state.select("last") {
                    cx.invalidate();
                }
            });

            tabs.root_with(
                div()
                    .child(
                        tabs.list_with(
                            div()
                                .child(first.tab_with(div().child("First")).on_click(select_first))
                                .child(unavailable.tab_with(div().child("Unavailable")))
                                .child(last.tab_with(div().child("Last")).on_click(select_last)),
                        ),
                    )
                    .children(first.panel_with(text("First panel")))
                    .children(unavailable.panel_with(text("Unavailable panel")))
                    .children(last.panel_with(text("Last panel"))),
            )
        }
    }

    #[test]
    fn non_looping_navigation_stops_at_each_enabled_edge() {
        let (mut cx, view) = TestAppContext::new(NonLoopingTabsView::default()).unwrap();
        let window = view.window_handle();
        let tabs = Tabs::new("non-looping-tabs", "first")
            .activate_on_focus(true)
            .loop_focus(false);
        let first = tabs.tab("first");
        let unavailable = tabs.tab("unavailable").disabled(true);
        let last = tabs.tab("last");

        cx.focus(window, first.tab_id()).unwrap();
        cx.simulate_keystrokes(window, "left").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(first.tab_id()));
        assert!(cx.contains_element(window, first.panel_id()).unwrap());

        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(last.tab_id()));
        assert!(!cx.contains_element(window, first.panel_id()).unwrap());
        assert!(cx.contains_element(window, last.panel_id()).unwrap());
        assert!(cx.click(window, unavailable.tab_id()).is_err());

        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(last.tab_id()));
        assert!(cx.contains_element(window, last.panel_id()).unwrap());

        cx.simulate_keystrokes(window, "home").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(first.tab_id()));
        assert!(cx.contains_element(window, first.panel_id()).unwrap());

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn activation_direction_follows_the_declared_order_and_orientation() {
        let mut state = TabsState::new("first");
        assert_eq!(state.activation_movement(), TabsActivationMovement::None);
        assert_eq!(state.active_index(), None);
        assert_eq!(
            Tabs::from_state("tabs", &state).activation_direction(),
            TabsActivationDirection::None
        );

        // The first indexed selection has no previous position to compare against.
        assert!(state.select_at("second", 1));
        assert_eq!(state.activation_movement(), TabsActivationMovement::None);
        assert_eq!(state.active_index(), Some(1));

        assert!(state.select_at("third", 2));
        assert_eq!(state.activation_movement(), TabsActivationMovement::Forward);
        assert_eq!(
            Tabs::from_state("tabs", &state).activation_direction(),
            TabsActivationDirection::Right
        );
        assert_eq!(
            Tabs::from_state("tabs", &state)
                .vertical()
                .activation_direction(),
            TabsActivationDirection::Down
        );

        assert!(state.select_at("first", 0));
        assert_eq!(
            state.activation_movement(),
            TabsActivationMovement::Backward
        );
        assert_eq!(
            Tabs::from_state("tabs", &state).activation_direction(),
            TabsActivationDirection::Left
        );
        assert_eq!(
            Tabs::from_state("tabs", &state)
                .vertical()
                .activation_direction(),
            TabsActivationDirection::Up
        );

        // Reselecting the same position reports no movement and no change.
        assert!(!state.select_at("first", 0));
        assert_eq!(state.activation_movement(), TabsActivationMovement::None);

        // A bare value carries no ordering, so it clears the recorded movement.
        assert!(state.select("third"));
        assert_eq!(state.activation_movement(), TabsActivationMovement::None);
        assert_eq!(state.active_index(), None);
        assert!(state.clear());
        assert_eq!(state.active(), None);

        // Each tab carries the direction its own tab set resolved.
        let mut moved = TabsState::new("a");
        moved.select_at("a", 0);
        moved.select_at("b", 1);
        let tabs = Tabs::from_state("tabs", &moved);
        assert_eq!(
            tabs.tab("b").activation_direction(),
            TabsActivationDirection::Right
        );
        assert_eq!(
            Tabs::new("tabs", "b")
                .activation_movement(TabsActivationMovement::Backward)
                .tab("b")
                .activation_direction(),
            TabsActivationDirection::Left
        );
    }

    struct IndicatorView {
        tabs: TabsState,
        geometry: crate::AnchorPlacementHandle,
    }

    impl Default for IndicatorView {
        fn default() -> Self {
            let mut tabs = TabsState::new("overview");
            tabs.select_at("overview", 0);
            Self {
                tabs,
                geometry: crate::AnchorPlacementHandle::new(),
            }
        }
    }

    impl View for IndicatorView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let tabs = Tabs::from_state("tabs", &self.tabs);
            let mut list = tabs.list_with(div().relative().flex_row());
            for (index, value) in ["overview", "activity"].into_iter().enumerate() {
                let tab = tabs.tab(value);
                let select = cx.listener(tab.tab_id(), move |view: &mut Self, cx| {
                    if view.tabs.select_at(value, index) {
                        cx.invalidate();
                    }
                });
                let mut element = tab.tab_with(div().w(120.0).h(32.0).on_click(select));
                if let Some(indicator) = tab.tracked_indicator_with(
                    div().h(3.0).w(120.0),
                    crate::AnchorPlacement::Bottom,
                    &self.geometry,
                ) {
                    element = element.child(indicator);
                }
                list = list.child(element);
            }
            div().size_full().child(tabs.root_with(div()).child(list))
        }
    }

    #[test]
    fn a_tracked_indicator_is_anchored_to_the_active_tab_and_reports_its_geometry() {
        let (mut cx, view) = TestAppContext::new(IndicatorView::default()).unwrap();
        let window = view.window_handle();
        let tabs = Tabs::new("tabs", "overview");
        let first = tabs.tab("overview");
        let second = tabs.tab("activity");

        assert!(cx.contains_element(window, first.indicator_id()).unwrap());
        assert!(!cx.contains_element(window, second.indicator_id()).unwrap());

        let tab_bounds = cx.element_bounds(window, first.tab_id()).unwrap();
        let indicator_bounds = cx.element_bounds(window, first.indicator_id()).unwrap();
        cx.run_until_idle().unwrap();
        // The framework places the indicator on the tab's bottom edge; the application only
        // declared its size.
        assert_eq!(indicator_bounds.y, tab_bounds.bottom());
        assert_eq!(indicator_bounds.x, tab_bounds.x);

        let geometry = cx
            .read(view, |view| {
                Tabs::from_state("tabs", &view.tabs).indicator_geometry(&view.geometry)
            })
            .unwrap()
            .expect("a painted indicator publishes the active tab's geometry");
        assert_eq!(geometry.left, tab_bounds.x);
        assert_eq!(geometry.top, tab_bounds.y);
        assert_eq!(geometry.width, tab_bounds.width);
        assert_eq!(geometry.height, tab_bounds.height);

        // Selecting the next tab moves both the mounted indicator and the reported geometry.
        cx.click(window, second.tab_id()).unwrap();
        assert_eq!(
            cx.read(view, |view| view.tabs.active()).unwrap(),
            Some("activity".into())
        );
        assert_eq!(
            cx.read(view, |view| Tabs::from_state("tabs", &view.tabs)
                .activation_direction())
                .unwrap(),
            TabsActivationDirection::Right
        );
        assert!(cx.contains_element(window, second.indicator_id()).unwrap());
        assert!(!cx.contains_element(window, first.indicator_id()).unwrap());
        let second_bounds = cx.element_bounds(window, second.tab_id()).unwrap();
        cx.run_until_idle().unwrap();
        let geometry = cx
            .read(view, |view| {
                Tabs::from_state("tabs", &view.tabs).indicator_geometry(&view.geometry)
            })
            .unwrap()
            .expect("the moved indicator republishes its geometry");
        assert_eq!(geometry.left, second_bounds.x);
        assert!(geometry.left > tab_bounds.x);

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
