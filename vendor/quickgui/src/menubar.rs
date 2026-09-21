use crate::{
    AccessibilityOrientation, AccessibilityPopover, AccessibilityRole, Element, ElementId,
    EventContext, FocusHandle, KeyBinding, StateAccessor, ViewContext, div,
};

/// Maximum top-level menus one in-window menubar manages.
///
/// A menubar is a small fixed set of application commands. The bound keeps roving navigation,
/// hover switching, and the retained indices constant-size whatever the application declares.
pub const MAX_MENUBAR_MENUS: usize = 64;

const MENUBAR_KEY_CONTEXT: &str = "Menubar";
const MENUBAR_ITEM_ID_TAG: u64 = 0x4d17_ec6b_9a20_35f1;

/// Move menubar focus to the previous menu, wrapping at the start.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MenubarPrevious;
/// Move menubar focus to the next menu, wrapping at the end.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MenubarNext;
/// Move menubar focus to the first menu.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MenubarFirst;
/// Move menubar focus to the final menu.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MenubarLast;
/// Open the focused menu.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MenubarOpen;
/// Close the open menu and keep focus on the menubar.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MenubarClose;

/// Contextual bindings used by [`MenubarItem::key_with`].
///
/// Left and Right move between menus while the bar has focus, and they keep working while a menu is
/// open because an open menu switches instead of closing — the desktop menubar convention. Down,
/// Return, and Space open the focused menu; Escape closes it without leaving the bar.
pub fn menubar_key_bindings() -> [KeyBinding; 8] {
    [
        KeyBinding::new("left", MenubarPrevious, Some(MENUBAR_KEY_CONTEXT)),
        KeyBinding::new("right", MenubarNext, Some(MENUBAR_KEY_CONTEXT)),
        KeyBinding::new("home", MenubarFirst, Some(MENUBAR_KEY_CONTEXT)),
        KeyBinding::new("end", MenubarLast, Some(MENUBAR_KEY_CONTEXT)),
        KeyBinding::new("down", MenubarOpen, Some(MENUBAR_KEY_CONTEXT)),
        KeyBinding::new("enter", MenubarOpen, Some(MENUBAR_KEY_CONTEXT)),
        KeyBinding::new("space", MenubarOpen, Some(MENUBAR_KEY_CONTEXT)),
        KeyBinding::new("escape", MenubarClose, Some(MENUBAR_KEY_CONTEXT)),
    ]
}

/// Controlled roving-focus and open state for one in-window menubar.
///
/// The application declares the menus and composes each surface from
/// [`crate::PopoverMenu`]; QuickGUI retains only which menu owns the bar's single Tab stop and
/// which one is open. The state is a plain copyable value with no item registry, allocation, task,
/// timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MenubarState {
    count: usize,
    focused: usize,
    open: Option<usize>,
    disabled: bool,
}

impl Default for MenubarState {
    fn default() -> Self {
        Self::new(0)
    }
}

impl MenubarState {
    /// A closed menubar over `menu_count` menus, focused on the first one.
    ///
    /// A larger count than [`MAX_MENUBAR_MENUS`] is clamped rather than rejected, so a dynamic
    /// application menu can never grow the retained state without bound.
    pub const fn new(menu_count: usize) -> Self {
        Self {
            count: if menu_count > MAX_MENUBAR_MENUS {
                MAX_MENUBAR_MENUS
            } else {
                menu_count
            },
            focused: 0,
            open: None,
            disabled: false,
        }
    }

    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub const fn is_disabled(self) -> bool {
        self.disabled
    }

    pub const fn menu_count(self) -> usize {
        self.count
    }

    /// Replace the declared menu count, clamping focus and closing a menu that no longer exists.
    pub const fn set_menu_count(&mut self, menu_count: usize) -> bool {
        let count = if menu_count > MAX_MENUBAR_MENUS {
            MAX_MENUBAR_MENUS
        } else {
            menu_count
        };
        if self.count == count {
            return false;
        }
        self.count = count;
        if self.focused >= count {
            self.focused = if count == 0 { 0 } else { count - 1 };
        }
        if let Some(open) = self.open
            && open >= count
        {
            self.open = None;
        }
        true
    }

    /// The menu that owns the menubar's single Tab stop.
    pub const fn focused_menu(self) -> usize {
        self.focused
    }

    pub const fn open_menu(self) -> Option<usize> {
        self.open
    }

    pub const fn is_open(self, index: usize) -> bool {
        matches!(self.open, Some(open) if open == index)
    }

    /// Move the roving Tab stop, keeping an already open menu in sync.
    pub const fn focus_menu(&mut self, index: usize) -> bool {
        if self.disabled || index >= self.count || self.focused == index {
            return false;
        }
        self.focused = index;
        if self.open.is_some() {
            self.open = Some(index);
        }
        true
    }

    /// Move focus one menu along the bar, wrapping at both ends.
    pub const fn move_focus(&mut self, forward: bool) -> bool {
        if self.disabled || self.count == 0 {
            return false;
        }
        let next = if forward {
            (self.focused + 1) % self.count
        } else {
            (self.focused + self.count - 1) % self.count
        };
        self.focus_menu(next)
    }

    /// Move focus to the first or last menu.
    pub const fn focus_edge(&mut self, last: bool) -> bool {
        if self.disabled || self.count == 0 {
            return false;
        }
        self.focus_menu(if last { self.count - 1 } else { 0 })
    }

    /// Open one menu and give it the roving Tab stop.
    pub const fn open_menu_at(&mut self, index: usize) -> bool {
        if self.disabled || index >= self.count {
            return false;
        }
        let changed = self.focused != index || !self.is_open(index);
        self.focused = index;
        self.open = Some(index);
        changed
    }

    /// Open the focused menu.
    pub const fn open_focused(&mut self) -> bool {
        if self.disabled || self.count == 0 || self.is_open(self.focused) {
            return false;
        }
        self.open_menu_at(self.focused)
    }

    /// Open a closed menu or close the one that is open.
    pub const fn toggle_menu(&mut self, index: usize) -> bool {
        if self.disabled || index >= self.count {
            return false;
        }
        if self.is_open(index) {
            return self.close();
        }
        self.open_menu_at(index)
    }

    /// Close whichever menu is open. Focus stays on the bar.
    pub const fn close(&mut self) -> bool {
        if self.open.is_none() {
            return false;
        }
        self.open = None;
        true
    }

    /// Switch menus on hover.
    ///
    /// Hovering does nothing while the bar is closed, and switches the open menu while one is
    /// open, which is exactly how a desktop menubar behaves.
    pub const fn hover_menu(&mut self, index: usize) -> bool {
        if self.disabled || index >= self.count || self.open.is_none() || self.is_open(index) {
            return false;
        }
        self.focused = index;
        self.open = Some(index);
        true
    }
}

/// A copyable declaration for one unstyled in-window menubar.
///
/// The descriptor retains no allocation, task, timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a Menubar descriptor has no effect until one of its parts is mounted"]
pub struct Menubar {
    root_id: ElementId,
}

impl Menubar {
    pub fn new(root_id: impl Into<ElementId>) -> Self {
        Self {
            root_id: root_id.into(),
        }
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub fn item_id(self, index: usize) -> ElementId {
        derived_menubar_id(self.root_id, MENUBAR_ITEM_ID_TAG, index as u64)
    }

    /// Describe one top-level menu trigger.
    ///
    /// Indices at or past [`MenubarState::menu_count`] return `None`, so a caller-driven loop stays
    /// bounded by the state instead of by its own arithmetic.
    pub const fn item(self, state: MenubarState, index: usize) -> Option<MenubarItem> {
        if index >= state.count {
            return None;
        }
        Some(MenubarItem {
            bar: self,
            state,
            index,
        })
    }

    /// Decorate an application-owned menubar root without adding layout or appearance.
    pub fn root_with(self, state: MenubarState, root: Element) -> Element {
        let disabled = state.disabled || root.accessibility.disabled;
        let root = root
            .id(self.root_id)
            .accessibility_role(AccessibilityRole::MenuBar)
            .accessibility_orientation(AccessibilityOrientation::Horizontal)
            .accessibility_size_of_set(state.count)
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
            .disabled(disabled);
        match state.open {
            Some(open) => root.accessibility_active_descendant(self.item_id(open)),
            None => root.accessibility_active_descendant(self.item_id(state.focused)),
        }
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self, state: MenubarState) -> Element {
        self.root_with(state, crate::div())
    }
}

/// A copyable declaration for one menubar trigger.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a MenubarItem descriptor has no effect until its part is mounted"]
pub struct MenubarItem {
    bar: Menubar,
    state: MenubarState,
    index: usize,
}

impl MenubarItem {
    pub const fn index(self) -> usize {
        self.index
    }

    pub const fn is_open(self) -> bool {
        self.state.is_open(self.index)
    }

    pub const fn is_focused(self) -> bool {
        self.state.focused == self.index
    }

    pub fn item_id(self) -> ElementId {
        self.bar.item_id(self.index)
    }

    /// Decorate an application-owned trigger without adding layout or appearance.
    ///
    /// Exactly one trigger carries the bar's Tab stop; the rest stay reachable only through the
    /// arrow keys, which is the roving pattern desktop menubars use.
    pub fn item_with(self, item: Element) -> Element {
        let disabled = self.state.disabled || item.accessibility.disabled;
        item.id(self.item_id())
            .accessibility_role(AccessibilityRole::MenuItem)
            .accessibility_has_popover(AccessibilityPopover::Menu)
            .accessibility_expanded(self.is_open())
            .accessibility_position_in_set(self.index)
            .accessibility_size_of_set(self.state.count)
            .selected(self.is_open())
            .clickable()
            .tab_index(if self.is_focused() { 0 } else { -1 })
            .key_context(MENUBAR_KEY_CONTEXT)
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
            .disabled(disabled)
    }
    /// Create the unstyled item part. Use [`Self::item_with`] to supply an existing element.
    pub fn item(self) -> Element {
        self.item_with(crate::div())
    }

    /// Attach QuickGUI's typed menubar actions, click opening, and hover switching.
    ///
    /// Install [`menubar_key_bindings`] once on the application keymap. Compose the surface itself
    /// from [`crate::PopoverMenu`] anchored to [`Self::item_id`]; the menubar owns which menu
    /// is open, never the menu's own contents.
    pub fn key_with<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        item: Element,
        access: fn(&mut V) -> &mut MenubarState,
    ) -> Element {
        self.key_with_accessor(cx, item, StateAccessor::from(access))
    }
    /// Create the unstyled key part. Use [`Self::key_with`] to supply an existing element.
    pub fn key<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut MenubarState,
    ) -> Element {
        self.key_with(cx, crate::div(), access)
    }

    /// Attach the typed menubar actions against a per-instance state accessor.
    ///
    /// A host that renders many declared menubars through one view passes an accessor that
    /// captures which [`MenubarState`] this trigger belongs to.
    pub fn key_with_accessor<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        item: Element,
        access_source: StateAccessor<V, MenubarState>,
    ) -> Element {
        let id = self.item_id();
        let bar = self.bar;
        let index = self.index;
        let access = access_source.clone();
        let previous = cx.action_listener(id, move |view, _: &MenubarPrevious, cx| {
            move_menubar_focus(view, cx, &access, bar, index, false);
        });
        let access = access_source.clone();
        let next = cx.action_listener(id, move |view, _: &MenubarNext, cx| {
            move_menubar_focus(view, cx, &access, bar, index, true);
        });
        let access = access_source.clone();
        let first = cx.action_listener(id, move |view, _: &MenubarFirst, cx| {
            let state = access.get(view);
            state.focus_menu(index);
            if state.focus_edge(false) {
                let focused = state.focused_menu();
                cx.focus(FocusHandle::new(bar.item_id(focused)));
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let last = cx.action_listener(id, move |view, _: &MenubarLast, cx| {
            let state = access.get(view);
            state.focus_menu(index);
            if state.focus_edge(true) {
                let focused = state.focused_menu();
                cx.focus(FocusHandle::new(bar.item_id(focused)));
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let open = cx.action_listener(id, move |view, _: &MenubarOpen, cx| {
            let state = access.get(view);
            state.focus_menu(index);
            if state.open_menu_at(index) {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let close = cx.action_listener(id, move |view, _: &MenubarClose, cx| {
            if access.get(view).close() {
                cx.focus(FocusHandle::new(id));
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let clicked = cx.listener(id, move |view, cx| {
            let state = access.get(view);
            state.focus_menu(index);
            if state.toggle_menu(index) {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let hovered = cx.hover_listener(id, move |view, hovered, cx| {
            if *hovered && access.get(view).hover_menu(index) {
                cx.focus(FocusHandle::new(id));
                cx.invalidate();
            }
        });

        item.on_action(previous)
            .on_action(next)
            .on_action(first)
            .on_action(last)
            .on_action(open)
            .on_action(close)
            .on_click(clicked)
            .on_hover(hovered)
    }
}

fn move_menubar_focus<V: 'static>(
    view: &mut V,
    cx: &mut EventContext,
    access: &StateAccessor<V, MenubarState>,
    bar: Menubar,
    index: usize,
    forward: bool,
) {
    let state = access.get(view);
    state.focus_menu(index);
    if state.move_focus(forward) {
        let focused = state.focused_menu();
        cx.focus(FocusHandle::new(bar.item_id(focused)));
        cx.invalidate();
    }
}

/// Create an unstyled semantic menubar root.
///
/// This shorthand is equivalent to `Menubar::new(id).root_with(state, div())`.
pub fn menubar(id: impl Into<ElementId>, state: MenubarState) -> Element {
    Menubar::new(id).root_with(state, div())
}

fn derived_menubar_id(parent: ElementId, tag: u64, index: u64) -> ElementId {
    let mut hash = parent.as_u64() ^ tag ^ index.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == parent.as_u64() || hash == u64::MAX {
        hash ^= tag.rotate_left(23);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppRegion, Application, Color, CursorStyle, IntoElement, UserSelect, View, WindowOptions,
        text,
    };

    #[test]
    fn roving_focus_open_and_hover_switching_stay_bounded() {
        let mut state = MenubarState::new(4);
        assert_eq!(state.menu_count(), 4);
        assert_eq!(state.focused_menu(), 0);
        assert_eq!(state.open_menu(), None);

        assert!(state.move_focus(true));
        assert_eq!(state.focused_menu(), 1);
        assert!(state.move_focus(false));
        assert!(state.move_focus(false));
        assert_eq!(state.focused_menu(), 3, "focus wraps at the start");
        assert!(state.move_focus(true));
        assert_eq!(state.focused_menu(), 0, "focus wraps at the end");
        assert!(state.focus_edge(true));
        assert_eq!(state.focused_menu(), 3);
        assert!(state.focus_edge(false));
        assert!(!state.focus_menu(9), "an index past the end is refused");

        // Hover does nothing until a menu is open, then it switches menus.
        assert!(!state.hover_menu(2));
        assert!(state.open_focused());
        assert!(state.is_open(0));
        assert!(!state.open_focused(), "the focused menu is already open");
        assert!(state.hover_menu(2));
        assert!(state.is_open(2));
        assert_eq!(state.focused_menu(), 2);
        assert!(!state.hover_menu(2));

        // Arrow movement while open keeps the open menu under the roving focus.
        assert!(state.move_focus(true));
        assert!(state.is_open(3));
        assert!(state.toggle_menu(3));
        assert_eq!(state.open_menu(), None);
        assert!(state.toggle_menu(1));
        assert!(state.is_open(1));
        assert!(state.close());
        assert!(!state.close());

        // A shrinking declaration clamps focus and closes a menu that no longer exists.
        assert!(state.open_menu_at(3));
        assert!(state.set_menu_count(2));
        assert_eq!(state.focused_menu(), 1);
        assert_eq!(state.open_menu(), None);
        assert!(!state.set_menu_count(2));

        let mut oversized = MenubarState::new(MAX_MENUBAR_MENUS + 10);
        assert_eq!(oversized.menu_count(), MAX_MENUBAR_MENUS);
        assert!(
            !oversized.set_menu_count(usize::MAX),
            "the clamped count is already at the bound"
        );
        assert_eq!(oversized.menu_count(), MAX_MENUBAR_MENUS);

        let mut disabled = MenubarState::new(3).disabled(true);
        assert!(disabled.is_disabled());
        assert!(!disabled.move_focus(true));
        assert!(!disabled.open_focused());
        assert!(!disabled.toggle_menu(1));
        assert!(!disabled.hover_menu(1));
    }

    #[test]
    fn parts_add_exact_semantics_without_layout_or_appearance() {
        let mut state = MenubarState::new(3);
        state.open_menu_at(1);
        let bar = Menubar::new("menubar");

        let root = bar.root_with(
            state,
            div()
                .h(28.0)
                .bg(Color::rgb8(1, 2, 3))
                .child("Application-owned bar"),
        );
        assert_eq!(root.accessibility.role, AccessibilityRole::MenuBar);
        assert_eq!(root.visual.background, Some(Color::rgb8(1, 2, 3)));
        assert_eq!(root.children.len(), 1);
        assert_eq!(
            root.accessibility.relations.active_descendant(),
            Some(bar.item_id(1))
        );
        assert_eq!(root.app_region, Some(AppRegion::NoDrag));
        assert_eq!(root.user_select, UserSelect::None);

        let open = bar.item(state, 1).expect("a declared menu");
        assert!(open.is_open());
        assert!(open.is_focused());
        let item = open.item_with(div().px(8.0).child(text("Edit")));
        assert_eq!(item.accessibility.role, AccessibilityRole::MenuItem);
        assert_eq!(item.accessibility.expanded, Some(true));
        assert!(item.clickable);
        assert!(item.focusable);
        assert_eq!(item.tab_index, 0, "the focused menu owns the Tab stop");
        assert_eq!(item.cursor_style, Some(CursorStyle::Arrow));
        assert_eq!(item.visual.background, None);
        assert_eq!(item.children.len(), 1);

        let closed = bar.item(state, 2).expect("a declared menu");
        let closed_item = closed.item_with(div());
        assert_eq!(closed_item.accessibility.expanded, Some(false));
        assert_eq!(
            closed_item.tab_index, -1,
            "unfocused menus are reached with the arrow keys"
        );

        assert!(
            bar.item(state, 3).is_none(),
            "indices stay inside the state"
        );
        assert_eq!(
            menubar("menubar", state).accessibility.role,
            AccessibilityRole::MenuBar
        );
        assert!(menubar("menubar", state).children.is_empty());
    }

    #[test]
    fn menubar_bindings_are_contextual_and_complete() {
        let bindings = menubar_key_bindings();
        assert_eq!(bindings.len(), 8);
        assert!(bindings.iter().all(|binding| {
            binding.context_predicate().is_some_and(|predicate| {
                predicate
                    .depth_of(&[crate::KeyContext::parse(MENUBAR_KEY_CONTEXT).unwrap()])
                    .is_some()
            })
        }));
    }

    #[derive(Default)]
    struct MenubarView {
        menubar: MenubarState,
    }

    impl MenubarView {
        fn menubar(view: &mut Self) -> &mut MenubarState {
            &mut view.menubar
        }
    }

    impl View for MenubarView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let bar = Menubar::new("menubar");
            let mut root = bar
                .root_with(self.menubar, div().flex_row())
                .accessibility_label("Application menus");
            for index in 0..self.menubar.menu_count() {
                let item = bar.item(self.menubar, index).expect("a declared menu");
                let element = item.item_with(div().child(text(match index {
                    0 => "File",
                    1 => "Edit",
                    _ => "View",
                })));
                root = root.child(item.key_with(cx, element, Self::menubar));
            }
            root
        }
    }

    #[test]
    fn menubar_uses_existing_focus_click_hover_and_idle_paths() {
        let (mut cx, view) = Application::new()
            .bind_keys(menubar_key_bindings())
            .into_test_context(
                WindowOptions::default(),
                MenubarView {
                    menubar: MenubarState::new(3),
                },
            )
            .unwrap();
        let window = view.window_handle();
        let bar = Menubar::new("menubar");

        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(bar.item_id(0)));

        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(bar.item_id(1)));
        assert_eq!(
            cx.read(view, |view| view.menubar.open_menu()).unwrap(),
            None
        );

        cx.simulate_keystrokes(window, "down").unwrap();
        assert_eq!(
            cx.read(view, |view| view.menubar.open_menu()).unwrap(),
            Some(1)
        );

        // Arrows keep switching menus while one is open.
        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(
            cx.read(view, |view| view.menubar.open_menu()).unwrap(),
            Some(2)
        );
        assert_eq!(cx.focused(window).unwrap(), Some(bar.item_id(2)));

        cx.simulate_keystrokes(window, "escape").unwrap();
        assert_eq!(
            cx.read(view, |view| view.menubar.open_menu()).unwrap(),
            None
        );
        assert_eq!(
            cx.focused(window).unwrap(),
            Some(bar.item_id(2)),
            "closing keeps focus on the bar"
        );

        cx.click(window, bar.item_id(0)).unwrap();
        assert_eq!(
            cx.read(view, |view| view.menubar.open_menu()).unwrap(),
            Some(0)
        );
        cx.click(window, bar.item_id(0)).unwrap();
        assert_eq!(
            cx.read(view, |view| view.menubar.open_menu()).unwrap(),
            None
        );

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
