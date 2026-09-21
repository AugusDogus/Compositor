use std::{rc::Rc, time::Duration};

use crate::{
    AccessibilityPopover, AnchorPlacement, AsyncViewContext, Color, ContextMenuEvent, Element,
    ElementId, EventContext, MAX_WINDOW_LOGICAL_COORDINATE, MAX_WINDOW_LOGICAL_DIMENSION,
    MenuItemPartState, MouseExitEvent, MouseMoveEvent, Point, PopoverConstraintAdjustment,
    PopoverMenu, PopoverMenuItem, PopoverMenuItemKind, PopoverMenuItemState, PopoverOptions, Rect,
    Size, StateAccessor, SystemPopover, Task, View, ViewContext, WindowBackgroundAppearance,
    WindowHandle, WindowOptions,
};

const CONTEXT_MENU_SURFACE_ID_TAG: u64 = 0xa255_e4af_3580_dd21;
const DEFAULT_SEPARATOR_HEIGHT: f32 = 9.0;
const DEFAULT_GROUP_LABEL_HEIGHT: f32 = 22.0;
const DEFAULT_VERTICAL_PADDING: f32 = 4.0;
const DEFAULT_SUBMENU_GAP: f32 = 2.0;
const MAX_CONTEXT_MENU_PART_HEIGHT: f32 = 256.0;
const MAX_CONTEXT_MENU_PADDING: f32 = 512.0;
const SUBMENU_CORRIDOR_VERTICAL_TOLERANCE: f32 = 8.0;

/// Native-style dwell before pointer hover opens a submenu.
pub const CONTEXT_MENU_SUBMENU_HOVER_DELAY: Duration = Duration::from_millis(150);
/// Maximum grace interval while the pointer follows a safe corridor into an open submenu.
pub const CONTEXT_MENU_SUBMENU_AIM_DELAY: Duration = Duration::from_millis(300);

/// Structural geometry for an unstyled cursor-point context menu.
///
/// Colors, borders, radii, shadows, icons, typography, and row contents remain entirely
/// application-owned. QuickGUI uses these measurements only to size the separate native popover
/// surface and to keep caller-rendered rows consistent with that surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextMenuLayout {
    width: f32,
    item_height: f32,
    separator_height: f32,
    group_label_height: f32,
    vertical_padding: f32,
    submenu_gap: f32,
    offset: Point,
    constraints: PopoverConstraintAdjustment,
}

impl ContextMenuLayout {
    pub fn new(width: f32, item_height: f32) -> Self {
        Self {
            width: finite_clamped(width, 1.0, MAX_WINDOW_LOGICAL_DIMENSION, 224.0),
            item_height: finite_clamped(item_height, 1.0, MAX_CONTEXT_MENU_PART_HEIGHT, 36.0),
            separator_height: DEFAULT_SEPARATOR_HEIGHT,
            group_label_height: DEFAULT_GROUP_LABEL_HEIGHT,
            vertical_padding: DEFAULT_VERTICAL_PADDING,
            submenu_gap: DEFAULT_SUBMENU_GAP,
            offset: Point::ZERO,
            constraints: PopoverConstraintAdjustment::FIT,
        }
    }

    pub const fn width(self) -> f32 {
        self.width
    }

    pub const fn item_height(self) -> f32 {
        self.item_height
    }

    pub const fn separator_row_height(self) -> f32 {
        self.separator_height
    }

    pub const fn group_label_row_height(self) -> f32 {
        self.group_label_height
    }

    pub const fn vertical_padding_value(self) -> f32 {
        self.vertical_padding
    }

    pub const fn submenu_gap_value(self) -> f32 {
        self.submenu_gap
    }

    pub const fn offset_value(self) -> Point {
        self.offset
    }

    pub const fn constraints(self) -> PopoverConstraintAdjustment {
        self.constraints
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = finite_clamped(width, 1.0, MAX_WINDOW_LOGICAL_DIMENSION, 224.0);
        self
    }

    pub fn with_item_height(mut self, height: f32) -> Self {
        self.item_height = finite_clamped(height, 1.0, MAX_CONTEXT_MENU_PART_HEIGHT, 36.0);
        self
    }

    pub fn separator_height(mut self, height: f32) -> Self {
        self.separator_height = finite_clamped(
            height,
            1.0,
            MAX_CONTEXT_MENU_PART_HEIGHT,
            DEFAULT_SEPARATOR_HEIGHT,
        );
        self
    }

    pub fn group_label_height(mut self, height: f32) -> Self {
        self.group_label_height = finite_clamped(
            height,
            1.0,
            MAX_CONTEXT_MENU_PART_HEIGHT,
            DEFAULT_GROUP_LABEL_HEIGHT,
        );
        self
    }

    pub fn vertical_padding(mut self, padding: f32) -> Self {
        self.vertical_padding = finite_clamped(
            padding,
            0.0,
            MAX_CONTEXT_MENU_PADDING,
            DEFAULT_VERTICAL_PADDING,
        );
        self
    }

    pub fn submenu_gap(mut self, gap: f32) -> Self {
        self.submenu_gap = finite_clamped(gap, 0.0, MAX_CONTEXT_MENU_PADDING, DEFAULT_SUBMENU_GAP);
        self
    }

    pub fn offset(mut self, x: f32, y: f32) -> Self {
        self.offset = Point::new(
            finite_clamped(
                x,
                -MAX_WINDOW_LOGICAL_COORDINATE,
                MAX_WINDOW_LOGICAL_COORDINATE,
                0.0,
            ),
            finite_clamped(
                y,
                -MAX_WINDOW_LOGICAL_COORDINATE,
                MAX_WINDOW_LOGICAL_COORDINATE,
                0.0,
            ),
        );
        self
    }

    pub const fn constraint_adjustment(mut self, constraints: PopoverConstraintAdjustment) -> Self {
        self.constraints = constraints;
        self
    }

    pub const fn row_height(self, kind: PopoverMenuItemKind) -> f32 {
        match kind {
            PopoverMenuItemKind::Separator => self.separator_height,
            PopoverMenuItemKind::GroupLabel => self.group_label_height,
            PopoverMenuItemKind::Action
            | PopoverMenuItemKind::Checkbox
            | PopoverMenuItemKind::Radio
            | PopoverMenuItemKind::Link
            | PopoverMenuItemKind::Submenu => self.item_height,
        }
    }

    pub fn popover_size(self, menu: &PopoverMenu) -> Size {
        let height = menu
            .items()
            .iter()
            .fold(f64::from(self.vertical_padding) * 2.0, |height, item| {
                height + f64::from(self.row_height(item.kind()))
            });
        Size::new(
            self.width,
            finite_clamped(height as f32, 1.0, MAX_WINDOW_LOGICAL_DIMENSION, 1.0),
        )
    }
}

impl Default for ContextMenuLayout {
    fn default() -> Self {
        Self::new(224.0, 36.0)
    }
}

/// Application-owned lifecycle state for an unstyled cursor-point context menu.
///
/// Closed state owns no native window, renderer, timer, task, observer, or scheduler source. An
/// open state owns exactly one direct child popover handle; nested submenu handles are owned by their
/// immediate popover parent and are torn down child-first with the root.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContextMenuState {
    popover: Option<WindowHandle>,
}

impl ContextMenuState {
    pub const fn new() -> Self {
        Self { popover: None }
    }

    pub const fn popover_window(self) -> Option<WindowHandle> {
        self.popover
    }

    pub const fn is_open(self) -> bool {
        self.popover.is_some()
    }

    /// Decorate a caller-owned target with context-menu semantics without changing appearance,
    /// focusability, pointer cursor, drag-region behavior, layout, or ordinary click handling.
    pub fn target_with(self, id: impl Into<ElementId>, target: Element) -> Element {
        target
            .id(id)
            .accessibility_has_popover(AccessibilityPopover::Menu)
            .accessibility_expanded(self.is_open())
    }
    /// Create the unstyled target part. Use [`Self::target_with`] to supply an existing element.
    pub fn target(self, id: impl Into<ElementId>) -> Element {
        self.target_with(id, crate::div())
    }

    /// Decorate a caller-owned target, Base UI's `ContextMenu.Trigger`.
    ///
    /// This is the Base UI-named alias of [`Self::target_with`]; both names decorate the same
    /// element identically.
    pub fn trigger_with(self, id: impl Into<ElementId>, trigger: Element) -> Element {
        self.target_with(id, trigger)
    }
    /// Create the unstyled trigger part. Use [`Self::trigger_with`] to supply an existing element.
    pub fn trigger(self, id: impl Into<ElementId>) -> Element {
        self.trigger_with(id, crate::button())
    }

    /// Decorate an optional caller-painted backdrop, Base UI's `ContextMenu.Backdrop`.
    ///
    /// The native popover surface already takes the pointer grab, so this layer exists only for a
    /// caller-painted dimming pass inside the owner window. It is hidden from assistive technology
    /// and carries no appearance of its own. Mount it only while [`Self::is_open`] is true.
    pub fn backdrop_with(self, id: impl Into<ElementId>, backdrop: Element) -> Element {
        backdrop
            .id(id)
            .overlay()
            .inset_0()
            .size_full()
            .app_region_no_drag()
            .cursor_default()
            .accessibility_hidden(true)
    }
    /// Create the unstyled backdrop part. Use [`Self::backdrop_with`] to supply an existing element.
    pub fn backdrop(self, id: impl Into<ElementId>) -> Element {
        self.backdrop_with(id, crate::div())
    }

    /// The Base UI-named render snapshot for one row of an open context menu.
    ///
    /// The native popover surface resolves its own placement against the display work area, so a
    /// context menu publishes row state rather than the popup's side and alignment.
    pub fn item_state(
        menu: &PopoverMenu,
        index: usize,
        submenu_open: bool,
    ) -> Option<MenuItemPartState> {
        menu.item_render_state(index, submenu_open)
    }

    /// Attach a complete cursor-point popover-menu interaction to a caller-owned target.
    ///
    /// `build_menu` runs only on a secondary click and may return `None` to suppress the popover.
    /// `render_root` and `render_item` own all appearance. The framework owns only bounded model
    /// behavior, structural geometry, point placement, separate-surface lifetime, keyboard focus,
    /// dismissal, submenu parenting, and exact native-close synchronization.
    #[allow(clippy::too_many_arguments)]
    pub fn element<V, BuildMenu, RenderRoot, RenderItem>(
        self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        access: fn(&mut V) -> &mut ContextMenuState,
        target: Element,
        layout: ContextMenuLayout,
        build_menu: BuildMenu,
        render_root: RenderRoot,
        render_item: RenderItem,
    ) -> Element
    where
        V: 'static,
        BuildMenu: Fn(&mut V, &ContextMenuEvent) -> Option<PopoverMenu> + 'static,
        RenderRoot: Fn() -> Element + 'static,
        RenderItem: Fn(&PopoverMenuItem, PopoverMenuItemState) -> Element + 'static,
    {
        self.element_with(
            cx,
            id,
            StateAccessor::from(access),
            target,
            layout,
            build_menu,
            render_root,
            render_item,
        )
    }

    /// Attach the cursor-point popover-menu interaction against a per-instance state accessor.
    ///
    /// A host that owns one [`ContextMenuState`] per declared target passes an accessor that
    /// captures which state this target opens into.
    #[allow(clippy::too_many_arguments)]
    pub fn element_with<V, BuildMenu, RenderRoot, RenderItem>(
        self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        access_source: StateAccessor<V, ContextMenuState>,
        target: Element,
        layout: ContextMenuLayout,
        build_menu: BuildMenu,
        render_root: RenderRoot,
        render_item: RenderItem,
    ) -> Element
    where
        V: 'static,
        BuildMenu: Fn(&mut V, &ContextMenuEvent) -> Option<PopoverMenu> + 'static,
        RenderRoot: Fn() -> Element + 'static,
        RenderItem: Fn(&PopoverMenuItem, PopoverMenuItemState) -> Element + 'static,
    {
        let id = id.into();
        let access = access_source.clone();
        cx.on_any_child_window_closed(move |view, closed, cx| {
            if access.get(view).popover == Some(closed) {
                access.get(view).popover = None;
                cx.invalidate();
            }
        });

        let root_renderer: ContextMenuRootRenderer = Rc::new(render_root);
        let item_renderer: ContextMenuItemRenderer = Rc::new(render_item);
        let open_root_renderer = Rc::clone(&root_renderer);
        let open_item_renderer = Rc::clone(&item_renderer);
        let menu_id = context_menu_surface_id(id);
        let access = access_source;
        let open = cx.context_menu_listener(id, move |view, event, cx| {
            let menu = build_menu(view, event);
            if let Some(previous) = access.get(view).popover.take() {
                cx.close_window_handle(previous);
            }
            let Some(menu) = menu else {
                cx.invalidate();
                return;
            };
            let size = layout.popover_size(&menu);
            let popover = ContextMenuPopoverView::new(
                menu,
                menu_id,
                layout,
                Rc::clone(&open_root_renderer),
                Rc::clone(&open_item_renderer),
            );
            let handle = cx.open_window(root_window_options(layout, event.position, size), popover);
            access.get(view).popover = Some(handle);
            cx.invalidate();
        });

        self.target_with(id, target).on_context_menu(open)
    }

    /// Close the current root context-menu surface synchronously from application state.
    pub fn close(&mut self, cx: &mut EventContext) -> bool {
        let Some(popover) = self.popover.take() else {
            return false;
        };
        cx.close_window_handle(popover);
        true
    }
}

type ContextMenuRootRenderer = Rc<dyn Fn() -> Element>;
type ContextMenuItemRenderer = Rc<dyn Fn(&PopoverMenuItem, PopoverMenuItemState) -> Element>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingHoverKind {
    Open,
    Switch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingHover {
    index: usize,
    kind: PendingHoverKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SubmenuSurfaceSignal {
    Geometry { window: WindowHandle, bounds: Rect },
    Entered { window: WindowHandle },
}

struct ContextMenuPopoverView {
    menu: PopoverMenu,
    menu_id: ElementId,
    layout: ContextMenuLayout,
    render_root: ContextMenuRootRenderer,
    render_item: ContextMenuItemRenderer,
    submenu: Option<WindowHandle>,
    submenu_anchor: Option<usize>,
    submenu_bounds: Option<Rect>,
    window_bounds: Rect,
    pointer: Option<Point>,
    hovered_item: Option<usize>,
    corridor_origin: Option<Point>,
    pending_hover: Option<PendingHover>,
    hover_generation: u64,
    hover_task: Option<Task<()>>,
    report_to_parent: bool,
    reported_bounds: Option<Rect>,
}

impl ContextMenuPopoverView {
    fn new(
        menu: PopoverMenu,
        menu_id: ElementId,
        layout: ContextMenuLayout,
        render_root: ContextMenuRootRenderer,
        render_item: ContextMenuItemRenderer,
    ) -> Self {
        Self::with_parent_reporting(menu, menu_id, layout, render_root, render_item, false)
    }

    fn submenu(
        menu: PopoverMenu,
        menu_id: ElementId,
        layout: ContextMenuLayout,
        render_root: ContextMenuRootRenderer,
        render_item: ContextMenuItemRenderer,
    ) -> Self {
        Self::with_parent_reporting(menu, menu_id, layout, render_root, render_item, true)
    }

    fn with_parent_reporting(
        menu: PopoverMenu,
        menu_id: ElementId,
        layout: ContextMenuLayout,
        render_root: ContextMenuRootRenderer,
        render_item: ContextMenuItemRenderer,
        report_to_parent: bool,
    ) -> Self {
        Self {
            menu,
            menu_id,
            layout,
            render_root,
            render_item,
            submenu: None,
            submenu_anchor: None,
            submenu_bounds: None,
            window_bounds: Rect::ZERO,
            pointer: None,
            hovered_item: None,
            corridor_origin: None,
            pending_hover: None,
            hover_generation: 0,
            hover_task: None,
            report_to_parent,
            reported_bounds: None,
        }
    }

    fn menu(view: &mut Self) -> &mut PopoverMenu {
        &mut view.menu
    }

    fn global_pointer(&self, local: Point) -> Point {
        Point::new(
            self.window_bounds.x + local.x,
            self.window_bounds.y + local.y,
        )
    }

    fn note_pointer(&mut self, local: Point) -> Point {
        let pointer = self.global_pointer(local);
        self.pointer = Some(pointer);
        if self.hovered_item == self.submenu_anchor && self.pending_hover.is_none() {
            self.corridor_origin = Some(pointer);
        }
        pointer
    }

    fn cancel_pending_hover(&mut self) {
        self.hover_generation = self.hover_generation.wrapping_add(1).max(1);
        self.pending_hover = None;
        if let Some(task) = self.hover_task.take() {
            task.cancel();
        }
    }

    fn schedule_hover(&mut self, pending: PendingHover, delay: Duration, cx: &mut EventContext) {
        self.cancel_pending_hover();
        self.pending_hover = Some(pending);
        let generation = self.hover_generation;
        match cx.spawn(|task_cx: AsyncViewContext<Self>| async move {
            if task_cx.sleep(delay).await.is_err() {
                return;
            }
            let _ = task_cx
                .update(move |view, cx| {
                    if view.hover_generation != generation || view.pending_hover != Some(pending) {
                        return;
                    }
                    view.pending_hover = None;
                    view.commit_hover(pending.index, cx);
                })
                .await;
        }) {
            Ok(task) => self.hover_task = Some(task),
            Err(_) => {
                self.pending_hover = None;
                self.commit_hover(pending.index, cx);
            }
        }
    }

    fn close_submenu(&mut self, cx: &mut EventContext) -> bool {
        let Some(submenu) = self.submenu.take() else {
            self.submenu_anchor = None;
            self.submenu_bounds = None;
            self.corridor_origin = None;
            return false;
        };
        self.submenu_anchor = None;
        self.submenu_bounds = None;
        self.corridor_origin = None;
        cx.close_window_handle(submenu);
        true
    }

    fn open_submenu(
        &mut self,
        index: usize,
        anchor: ElementId,
        menu: PopoverMenu,
        cx: &mut EventContext,
    ) {
        if self.submenu.is_some() && self.submenu_anchor == Some(index) {
            return;
        }
        self.close_submenu(cx);
        let size = self.layout.popover_size(&menu);
        let popover = ContextMenuPopoverView::submenu(
            menu,
            self.menu_id,
            self.layout,
            Rc::clone(&self.render_root),
            Rc::clone(&self.render_item),
        );
        let handle = SystemPopover::new(size.width, size.height)
            .placement(AnchorPlacement::RightStart)
            .gap(self.layout.submenu_gap)
            .constraint_adjustment(self.layout.constraints)
            // The root owns key-focus dismissal and the one AppKit event monitor. Attached
            // descendants stay inside that chain without multiplying native monitoring work.
            .grab(false)
            .open(cx, anchor, "Context submenu", popover)
            .ok();
        self.submenu = handle;
        self.submenu_anchor = handle.map(|_| index);
        self.submenu_bounds = None;
        self.corridor_origin = self.pointer;
        cx.invalidate();
    }

    fn commit_hover(&mut self, index: usize, cx: &mut EventContext) {
        let changed = self.menu.highlight(index);
        let submenu = self
            .menu
            .items()
            .get(index)
            .and_then(PopoverMenuItem::submenu_menu)
            .cloned();
        if let Some(menu) = submenu {
            if let Some(anchor) = self.menu.item_element_id(self.menu_id, index) {
                self.open_submenu(index, anchor, menu, cx);
            }
        } else {
            self.close_submenu(cx);
        }
        if changed {
            cx.invalidate();
        }
    }

    fn handle_hover(&mut self, index: usize, hovered: bool, cx: &mut EventContext) {
        if !hovered {
            if self.hovered_item == Some(index) {
                self.hovered_item = None;
            }
            if self.pending_hover
                == Some(PendingHover {
                    index,
                    kind: PendingHoverKind::Open,
                })
            {
                self.cancel_pending_hover();
            }
            // The row goes dark once the pointer has left it, as a native menu's does, unless it
            // anchors an open submenu the pointer is on its way to.
            let anchors_open_submenu = self.submenu.is_some() && self.submenu_anchor == Some(index);
            if !anchors_open_submenu && self.menu.unhighlight(index) {
                cx.invalidate();
            }
            return;
        }

        self.hovered_item = Some(index);
        if let Some(local) = cx.pointer_position() {
            self.pointer = Some(self.global_pointer(local));
        }
        if self.submenu.is_some() && self.submenu_anchor == Some(index) {
            self.cancel_pending_hover();
            self.corridor_origin = self.pointer;
            if self.menu.highlight(index) {
                cx.invalidate();
            }
            return;
        }

        let protected = self
            .corridor_origin
            .zip(self.pointer)
            .zip(self.submenu_bounds)
            .is_some_and(|((origin, pointer), submenu)| {
                submenu_corridor_contains(origin, pointer, submenu)
            });
        if self.submenu.is_some() && protected {
            self.schedule_hover(
                PendingHover {
                    index,
                    kind: PendingHoverKind::Switch,
                },
                CONTEXT_MENU_SUBMENU_AIM_DELAY,
                cx,
            );
            return;
        }

        self.cancel_pending_hover();
        self.close_submenu(cx);
        let changed = self.menu.highlight(index);
        let has_submenu = self
            .menu
            .items()
            .get(index)
            .is_some_and(|item| item.submenu_menu().is_some());
        if has_submenu {
            self.schedule_hover(
                PendingHover {
                    index,
                    kind: PendingHoverKind::Open,
                },
                CONTEXT_MENU_SUBMENU_HOVER_DELAY,
                cx,
            );
        }
        if changed {
            cx.invalidate();
        }
    }

    fn handle_mouse_move(&mut self, event: &MouseMoveEvent) {
        self.note_pointer(event.position);
    }

    fn handle_mouse_exit(&mut self, event: &MouseExitEvent) {
        let pointer = self.note_pointer(event.position);
        self.hovered_item = None;
        match self.pending_hover {
            Some(PendingHover {
                kind: PendingHoverKind::Open,
                ..
            }) => self.cancel_pending_hover(),
            Some(PendingHover {
                kind: PendingHoverKind::Switch,
                ..
            }) => {
                let protected = self.corridor_origin.zip(self.submenu_bounds).is_some_and(
                    |(origin, submenu)| submenu_corridor_contains(origin, pointer, submenu),
                );
                if !protected {
                    self.cancel_pending_hover();
                }
            }
            None => {}
        }
    }

    fn handle_surface_signal(&mut self, signal: SubmenuSurfaceSignal) {
        match signal {
            SubmenuSurfaceSignal::Geometry { window, bounds } if self.submenu == Some(window) => {
                self.submenu_bounds = Some(bounds);
            }
            SubmenuSurfaceSignal::Entered { window } if self.submenu == Some(window) => {
                self.cancel_pending_hover();
            }
            SubmenuSurfaceSignal::Geometry { .. } | SubmenuSurfaceSignal::Entered { .. } => {}
        }
    }

    fn report_surface_geometry(&mut self, cx: &ViewContext<'_, Self>, bounds: Rect) {
        if !self.report_to_parent || self.reported_bounds == Some(bounds) {
            return;
        }
        let window = cx.window_handle();
        if let Ok(task) = cx.spawn(|task_cx: AsyncViewContext<Self>| async move {
            let _ = task_cx
                .update(move |_view, cx| {
                    cx.dispatch_action_to_parent(SubmenuSurfaceSignal::Geometry { window, bounds });
                })
                .await;
        }) {
            self.reported_bounds = Some(bounds);
            task.detach();
        }
    }
}

impl View for ContextMenuPopoverView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl crate::IntoElement {
        let bounds = cx.window_state().bounds.bounds();
        self.window_bounds = bounds;
        self.report_surface_geometry(cx, bounds);

        cx.on_any_child_window_closed(|view, closed, cx| {
            if view.submenu == Some(closed) {
                view.submenu = None;
                view.submenu_anchor = None;
                view.submenu_bounds = None;
                view.corridor_origin = None;
                view.cancel_pending_hover();
                cx.invalidate();
            }
        });

        let layout = self.layout;
        let item_renderer = Rc::clone(&self.render_item);
        let menu_id = self.menu_id;
        let surface_signal =
            cx.action_listener(menu_id, |view, signal: &SubmenuSurfaceSignal, _cx| {
                view.handle_surface_signal(*signal);
            });
        let mouse_move = cx.mouse_move_listener(menu_id, |view, event, _cx| {
            view.handle_mouse_move(event);
        });
        let mouse_exit = cx.mouse_exit_listener(menu_id, |view, event, _cx| {
            view.handle_mouse_exit(event);
        });
        let surface_hover = cx.hover_listener(menu_id, |view, hovered, cx| {
            if *hovered
                && view.report_to_parent
                && let Some(window) = cx.window_handle()
            {
                cx.dispatch_action_to_parent(SubmenuSurfaceSignal::Entered { window });
            }
        });
        self.menu
            .element_with_submenus_and_hover(
                cx,
                menu_id,
                Self::menu,
                (self.render_root)().size_full(),
                move |item, state| {
                    item_renderer(item, state)
                        .h(layout.row_height(item.kind()))
                        .flex_none()
                },
                |_view, cx| {
                    cx.close_popover_chain();
                },
                move |view, anchor, menu, cx| {
                    view.cancel_pending_hover();
                    let index = view.menu.active_index().unwrap_or(0);
                    view.open_submenu(index, anchor, menu, cx);
                },
                |view, index, hovered, cx| view.handle_hover(index, hovered, cx),
            )
            .on_action(surface_signal)
            .on_mouse_move(mouse_move)
            .on_mouse_exit(mouse_exit)
            .on_hover(surface_hover)
    }
}

fn root_window_options(layout: ContextMenuLayout, position: Point, size: Size) -> WindowOptions {
    WindowOptions::new("Context menu")
        .size(size.width, size.height)
        .background(Color::TRANSPARENT)
        .window_background(WindowBackgroundAppearance::Transparent)
        .system_popover(
            PopoverOptions::new(Rect::new(position.x, position.y, 0.0, 0.0))
                .constraint_adjustment(layout.constraints)
                .offset(layout.offset.x, layout.offset.y),
        )
}

fn context_menu_surface_id(target: ElementId) -> ElementId {
    let mut value = target.as_u64() ^ CONTEXT_MENU_SURFACE_ID_TAG;
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    if value == 0 || value == target.as_u64() || value == u64::MAX {
        value ^= CONTEXT_MENU_SURFACE_ID_TAG.rotate_left(19);
    }
    ElementId::new(value)
}

fn submenu_corridor_contains(origin: Point, pointer: Point, submenu: Rect) -> bool {
    if ![
        origin.x,
        origin.y,
        pointer.x,
        pointer.y,
        submenu.x,
        submenu.y,
        submenu.width,
        submenu.height,
    ]
    .into_iter()
    .all(f32::is_finite)
        || submenu.is_empty()
    {
        return false;
    }
    if submenu.contains(pointer) {
        return true;
    }
    let opens_right = submenu.x + submenu.width * 0.5 >= origin.x;
    if (opens_right && pointer.x < origin.x) || (!opens_right && pointer.x > origin.x) {
        return false;
    }
    let near_x = if opens_right {
        submenu.x
    } else {
        submenu.right()
    };
    point_in_triangle(
        pointer,
        origin,
        Point::new(near_x, submenu.y - SUBMENU_CORRIDOR_VERTICAL_TOLERANCE),
        Point::new(
            near_x,
            submenu.bottom() + SUBMENU_CORRIDOR_VERTICAL_TOLERANCE,
        ),
    )
}

fn point_in_triangle(point: Point, first: Point, second: Point, third: Point) -> bool {
    fn side(point: Point, first: Point, second: Point) -> f32 {
        (point.x - second.x) * (first.y - second.y) - (first.x - second.x) * (point.y - second.y)
    }

    let first_side = side(point, first, second);
    let second_side = side(point, second, third);
    let third_side = side(point, third, first);
    let has_negative = first_side < 0.0 || second_side < 0.0 || third_side < 0.0;
    let has_positive = first_side > 0.0 || second_side > 0.0 || third_side > 0.0;
    !(has_negative && has_positive)
}

fn finite_clamped(value: f32, minimum: f32, maximum: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        fallback.clamp(minimum, maximum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Application, Color, IntoElement, Modifiers, TestAppContext, View, WindowKind,
        WindowOptions, div, popover_menu_key_bindings, text,
    };

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Command {
        Open,
        More,
    }

    fn menu() -> PopoverMenu {
        PopoverMenu::new([
            PopoverMenuItem::group_label("File"),
            PopoverMenuItem::action("open", "Open", Command::Open),
            PopoverMenuItem::separator(),
            PopoverMenuItem::action("more", "More", Command::More),
        ])
        .unwrap()
    }

    fn popover_root() -> Element {
        div().bg(Color::BLACK)
    }

    fn popover_item(item: &PopoverMenuItem, state: PopoverMenuItemState) -> Element {
        div()
            .child(text(item.label().clone()))
            .opacity(if state.highlighted { 1.0 } else { 0.8 })
    }

    #[test]
    fn layout_is_structural_bounded_and_kind_aware() {
        let layout = ContextMenuLayout::new(f32::INFINITY, f32::NAN)
            .separator_height(7.0)
            .group_label_height(19.0)
            .vertical_padding(5.0);
        assert_eq!(layout.width(), 224.0);
        assert_eq!(layout.item_height(), 36.0);
        assert_eq!(layout.row_height(PopoverMenuItemKind::Separator), 7.0);
        assert_eq!(layout.row_height(PopoverMenuItemKind::GroupLabel), 19.0);
        assert_eq!(layout.popover_size(&menu()), Size::new(224.0, 108.0));
    }

    #[test]
    fn submenu_corridor_tracks_real_right_and_left_popover_edges() {
        let right = Rect::new(100.0, 20.0, 80.0, 100.0);
        assert!(submenu_corridor_contains(
            Point::new(20.0, 60.0),
            Point::new(60.0, 88.0),
            right,
        ));
        assert!(!submenu_corridor_contains(
            Point::new(20.0, 60.0),
            Point::new(10.0, 60.0),
            right,
        ));

        let left = Rect::new(0.0, 20.0, 80.0, 100.0);
        assert!(submenu_corridor_contains(
            Point::new(160.0, 60.0),
            Point::new(120.0, 88.0),
            left,
        ));
        assert!(!submenu_corridor_contains(
            Point::new(160.0, 60.0),
            Point::new(170.0, 60.0),
            left,
        ));
        assert!(!submenu_corridor_contains(
            Point::ZERO,
            Point::ZERO,
            Rect::new(0.0, 0.0, f32::NAN, 10.0),
        ));
    }

    #[test]
    fn target_part_adds_semantics_without_appearance_or_cursor_policy() {
        let state = ContextMenuState::new();
        let target = state.target_with("target", div());
        assert_eq!(
            target.accessibility.has_popover,
            Some(AccessibilityPopover::Menu)
        );
        assert_eq!(target.accessibility.expanded, Some(false));
        assert_eq!(target.visual.background, None);
        assert_eq!(target.visual.border_color, None);
        assert_eq!(target.cursor_style, None);
        assert_eq!(target.app_region, None);
    }

    #[derive(Default)]
    struct Owner {
        context_menu: ContextMenuState,
        opened_at: Option<Point>,
        received: Option<Command>,
    }

    impl Owner {
        fn context_menu(view: &mut Self) -> &mut ContextMenuState {
            &mut view.context_menu
        }
    }

    impl View for Owner {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let command = cx.action_listener("owner", |view, command: &Command, cx| {
                view.received = Some(command.clone());
                cx.invalidate();
            });
            let target = self.context_menu.element(
                cx,
                "target",
                Self::context_menu,
                div().size(320.0, 180.0),
                ContextMenuLayout::new(220.0, 32.0)
                    .group_label_height(20.0)
                    .separator_height(8.0)
                    .vertical_padding(4.0),
                |view, event| {
                    view.opened_at = Some(event.position);
                    Some(menu())
                },
                popover_root,
                popover_item,
            );
            div()
                .focus_scope(cx.focus_handle("owner"))
                .on_action(command)
                .child(target)
        }
    }

    #[test]
    fn secondary_click_opens_at_the_pointer_and_commands_close_exactly() {
        let (mut cx, owner) = Application::new()
            .bind_keys(popover_menu_key_bindings())
            .into_test_context(WindowOptions::default(), Owner::default())
            .unwrap();
        let owner_window = owner.window_handle();
        let point = Point::new(120.0, 140.0);
        cx.simulate_context_menu(owner_window, "target", point, Modifiers::SHIFT)
            .unwrap();

        let popover = cx
            .read(owner, |view| view.context_menu.popover_window().unwrap())
            .unwrap();
        let state = cx.window_state(popover).unwrap();
        assert_eq!(state.kind, WindowKind::SystemPopover);
        assert_eq!(state.viewport_size, Size::new(220.0, 100.0));
        assert_eq!(cx.read(owner, |view| view.opened_at).unwrap(), Some(point));

        let open = menu()
            .item_element_id(context_menu_surface_id("target".into()), 1)
            .unwrap();
        cx.click(popover, open).unwrap();
        assert_eq!(
            cx.read(owner, |view| view.received.clone()).unwrap(),
            Some(Command::Open)
        );
        assert!(!cx.is_window_open(popover));
        assert_eq!(
            cx.read(owner, |view| view.context_menu.popover_window())
                .unwrap(),
            None
        );

        cx.simulate_context_menu(owner_window, "target", point, Modifiers::empty())
            .unwrap();
        let popover = cx
            .read(owner, |view| view.context_menu.popover_window().unwrap())
            .unwrap();
        cx.update(owner, |_view, cx| cx.close_window_handle(popover))
            .unwrap();
        assert_eq!(
            cx.read(owner, |view| view.context_menu.popover_window())
                .unwrap(),
            None
        );
        let renders = cx.render_count(owner_window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(owner_window).unwrap(), renders);
    }

    #[test]
    fn no_menu_result_closes_an_existing_surface_without_replacement() {
        struct ConditionalOwner {
            state: ContextMenuState,
            enabled: bool,
        }

        impl ConditionalOwner {
            fn state(view: &mut Self) -> &mut ContextMenuState {
                &mut view.state
            }
        }

        impl View for ConditionalOwner {
            fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
                self.state.element(
                    cx,
                    "conditional",
                    Self::state,
                    div().size(100.0, 100.0),
                    ContextMenuLayout::default(),
                    |view, _event| view.enabled.then(menu),
                    popover_root,
                    popover_item,
                )
            }
        }

        let (mut cx, owner) = TestAppContext::new(ConditionalOwner {
            state: ContextMenuState::new(),
            enabled: true,
        })
        .unwrap();
        let window = owner.window_handle();
        cx.simulate_context_menu(window, "conditional", Point::ZERO, Modifiers::empty())
            .unwrap();
        let popover = cx
            .read(owner, |view| view.state.popover_window().unwrap())
            .unwrap();
        cx.update(owner, |view, cx| {
            view.enabled = false;
            cx.invalidate();
        })
        .unwrap();
        cx.simulate_context_menu(window, "conditional", Point::ZERO, Modifiers::empty())
            .unwrap();
        assert!(!cx.is_window_open(popover));
        assert_eq!(
            cx.read(owner, |view| view.state.popover_window()).unwrap(),
            None
        );
    }

    fn hover_menu() -> PopoverMenu {
        let child = PopoverMenu::new([
            PopoverMenuItem::action("child-one", "Child one", Command::Open),
            PopoverMenuItem::action("child-two", "Child two", Command::Open),
            PopoverMenuItem::action("child-three", "Child three", Command::Open),
        ])
        .unwrap();
        PopoverMenu::new([
            PopoverMenuItem::submenu("more", "More", child),
            PopoverMenuItem::action("ordinary", "Ordinary", Command::More),
        ])
        .unwrap()
    }

    #[derive(Default)]
    struct HoverOwner {
        state: ContextMenuState,
    }

    impl HoverOwner {
        fn state(view: &mut Self) -> &mut ContextMenuState {
            &mut view.state
        }
    }

    impl View for HoverOwner {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            self.state.element(
                cx,
                "hover-target",
                Self::state,
                div().size(320.0, 180.0),
                ContextMenuLayout::new(220.0, 32.0),
                |_view, _event| Some(hover_menu()),
                popover_root,
                popover_item,
            )
        }
    }

    #[test]
    fn hover_open_and_safe_corridor_use_one_exact_deadline_and_cancel_on_child_entry() {
        let (mut cx, owner) = TestAppContext::new(HoverOwner::default()).unwrap();
        let owner_window = owner.window_handle();
        cx.simulate_context_menu(
            owner_window,
            "hover-target",
            Point::new(100.0, 100.0),
            Modifiers::empty(),
        )
        .unwrap();
        let popover = cx
            .read(owner, |view| view.state.popover_window().unwrap())
            .unwrap();
        let popover_view = cx.typed_window::<ContextMenuPopoverView>(popover).unwrap();
        assert_eq!(cx.popover_grabs_focus(popover).unwrap(), Some(true));

        cx.visual(popover)
            .unwrap()
            .move_pointer(Point::new(10.0, 16.0))
            .unwrap();
        assert_eq!(
            cx.read(popover_view, |view| view.pending_hover).unwrap(),
            Some(PendingHover {
                index: 0,
                kind: PendingHoverKind::Open,
            })
        );
        cx.advance_time(CONTEXT_MENU_SUBMENU_HOVER_DELAY - Duration::from_millis(1))
            .unwrap();
        assert_eq!(cx.read(popover_view, |view| view.submenu).unwrap(), None);
        cx.advance_time(Duration::from_millis(1)).unwrap();

        let child = cx.read(popover_view, |view| view.submenu.unwrap()).unwrap();
        assert!(cx.is_window_open(child));
        assert_eq!(cx.popover_grabs_focus(child).unwrap(), Some(false));
        let child_bounds = cx.window_state(child).unwrap().bounds.bounds();
        assert_eq!(
            cx.read(popover_view, |view| view.submenu_bounds).unwrap(),
            Some(child_bounds)
        );

        let parent_bounds = cx.window_state(popover).unwrap().bounds.bounds();
        let opens_right = child_bounds.x + child_bounds.width * 0.5
            >= parent_bounds.x + parent_bounds.width * 0.5;
        let ordinary_id = hover_menu()
            .item_element_id(context_menu_surface_id("hover-target".into()), 1)
            .unwrap();
        let ordinary_bounds = cx.element_bounds(popover, ordinary_id).unwrap();
        let corridor_point = Point::new(
            if opens_right {
                ordinary_bounds.right() - 1.0
            } else {
                ordinary_bounds.x + 1.0
            },
            ordinary_bounds.y + ordinary_bounds.height * 0.5,
        );
        let (corridor_origin, reported_child_bounds) = cx
            .read(popover_view, |view| {
                (view.corridor_origin.unwrap(), view.submenu_bounds.unwrap())
            })
            .unwrap();
        let global_corridor_point = Point::new(
            parent_bounds.x + corridor_point.x,
            parent_bounds.y + corridor_point.y,
        );
        assert!(
            submenu_corridor_contains(
                corridor_origin,
                global_corridor_point,
                reported_child_bounds,
            ),
            "origin={corridor_origin:?} pointer={global_corridor_point:?} child={reported_child_bounds:?} parent={parent_bounds:?}",
        );
        cx.visual(popover)
            .unwrap()
            .move_pointer(corridor_point)
            .unwrap();
        assert_eq!(
            cx.read(popover_view, |view| view.pending_hover).unwrap(),
            Some(PendingHover {
                index: 1,
                kind: PendingHoverKind::Switch,
            })
        );
        cx.advance_time(CONTEXT_MENU_SUBMENU_AIM_DELAY - Duration::from_millis(1))
            .unwrap();
        assert_eq!(
            cx.read(popover_view, |view| view.submenu).unwrap(),
            Some(child)
        );

        cx.visual(child)
            .unwrap()
            .move_pointer(Point::new(10.0, 16.0))
            .unwrap();
        assert_eq!(
            cx.read(popover_view, |view| view.pending_hover).unwrap(),
            None
        );
        cx.advance_time(Duration::from_millis(1)).unwrap();
        assert_eq!(
            cx.read(popover_view, |view| view.submenu).unwrap(),
            Some(child)
        );

        cx.visual(popover)
            .unwrap()
            .move_pointer(Point::new(10.0, 16.0))
            .unwrap();
        cx.visual(popover)
            .unwrap()
            .move_pointer(corridor_point)
            .unwrap();
        cx.advance_time(CONTEXT_MENU_SUBMENU_AIM_DELAY).unwrap();
        assert!(!cx.is_window_open(child));
        assert_eq!(cx.read(popover_view, |view| view.submenu).unwrap(), None);
        assert_eq!(
            cx.read(popover_view, |view| view.menu.active_index())
                .unwrap(),
            Some(1)
        );

        let more_id = hover_menu()
            .item_element_id(context_menu_surface_id("hover-target".into()), 0)
            .unwrap();
        let more_bounds = cx.element_bounds(popover, more_id).unwrap();
        let more_point = Point::new(
            more_bounds.x + 10.0,
            more_bounds.y + more_bounds.height * 0.5,
        );
        cx.visual(popover)
            .unwrap()
            .move_pointer(more_point)
            .unwrap();
        assert!(
            cx.read(popover_view, |view| view.pending_hover.is_some())
                .unwrap()
        );
        cx.simulate_mouse_exit(
            popover,
            context_menu_surface_id("hover-target".into()),
            MouseExitEvent {
                position: more_point,
                pressed_button: None,
                modifiers: Modifiers::empty(),
            },
        )
        .unwrap();
        assert_eq!(
            cx.read(popover_view, |view| view.pending_hover).unwrap(),
            None
        );
        cx.advance_time(CONTEXT_MENU_SUBMENU_HOVER_DELAY).unwrap();
        assert_eq!(cx.read(popover_view, |view| view.submenu).unwrap(), None);

        let renders = cx.render_count(popover).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(popover).unwrap(), renders);
    }
}
