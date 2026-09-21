use std::{
    collections::{HashMap, HashSet},
    fmt,
    rc::Rc,
    sync::Arc,
};
use web_time::{Duration, Instant};

use thiserror::Error;

use crate::{
    AccessibilityOrientation, AccessibilityPopover, AccessibilityRole, Action, AnchorAlign,
    AnchorPlacement, AnchorPlacementHandle, AnchorSide, AnyAction, AsyncViewContext,
    DismissListener, Element, ElementId, EventContext, FocusHandle, HoverListener, Key, KeyBinding,
    MAX_POPOVER_HOVER_DELAY, Modifiers, Popover, PopoverKind, StateAccessor, Task, ViewContext,
};

/// Maximum entries retained by one popover-menu tree, including nested submenus.
pub const MAX_POPOVER_MENU_ITEMS: usize = 2_048;
/// Maximum nested popover-menu levels retained by one model.
pub const MAX_POPOVER_MENU_DEPTH: usize = 8;
/// Maximum UTF-8 bytes retained by one item label, shortcut, or typeahead label.
pub const MAX_POPOVER_MENU_ITEM_TEXT_BYTES: usize = 16 * 1024;
/// Maximum text retained by one complete popover-menu tree.
pub const MAX_POPOVER_MENU_TEXT_BYTES: usize = 4 * 1024 * 1024;
/// Maximum normalized text-navigation prefix retained between key presses.
pub const MAX_POPOVER_MENU_TYPEAHEAD_BYTES: usize = 256;
/// Inactivity interval after which the next printable key starts a fresh search.
pub const POPOVER_MENU_TYPEAHEAD_TIMEOUT: Duration = Duration::from_millis(500);

/// Context installed on an interactive vertical [`PopoverMenu`] root.
pub const POPOVER_MENU_KEY_CONTEXT: &str = "PopoverMenu";
/// Context installed on an interactive horizontal [`PopoverMenu`] root, such as a menubar row.
pub const POPOVER_MENU_HORIZONTAL_KEY_CONTEXT: &str = "PopoverMenuHorizontal";

/// Maximum UTF-8 bytes retained by one menu link item's destination URL.
pub const MAX_POPOVER_MENU_LINK_BYTES: usize = 8 * 1024;

const ITEM_ID_TAG: u64 = 0x09b5_05a4_420a_f4d1;
const GROUP_LABEL_ID_TAG: u64 = 0xd4c3_7b1a_91e5_208f;

/// Highlight the previous enabled popover-menu item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PopoverMenuPrevious;
/// Highlight the next enabled popover-menu item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PopoverMenuNext;
/// Highlight the first enabled popover-menu item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PopoverMenuFirst;
/// Highlight the final enabled popover-menu item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PopoverMenuLast;
/// Activate the highlighted popover-menu item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PopoverMenuActivate;
/// Open the highlighted submenu without activating an ordinary command.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PopoverMenuOpenSubmenu;
/// Dismiss the current popover-menu level.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PopoverMenuClose;

/// Contextual keyboard behavior for an open popover menu.
pub fn popover_menu_key_bindings() -> [KeyBinding; 9] {
    [
        KeyBinding::new("up", PopoverMenuPrevious, Some(POPOVER_MENU_KEY_CONTEXT)),
        KeyBinding::new("down", PopoverMenuNext, Some(POPOVER_MENU_KEY_CONTEXT)),
        KeyBinding::new("home", PopoverMenuFirst, Some(POPOVER_MENU_KEY_CONTEXT)),
        KeyBinding::new("end", PopoverMenuLast, Some(POPOVER_MENU_KEY_CONTEXT)),
        KeyBinding::new("enter", PopoverMenuActivate, Some(POPOVER_MENU_KEY_CONTEXT)),
        KeyBinding::new("space", PopoverMenuActivate, Some(POPOVER_MENU_KEY_CONTEXT)),
        KeyBinding::new(
            "right",
            PopoverMenuOpenSubmenu,
            Some(POPOVER_MENU_KEY_CONTEXT),
        ),
        KeyBinding::new("left", PopoverMenuClose, Some(POPOVER_MENU_KEY_CONTEXT)),
        KeyBinding::new("escape", PopoverMenuClose, Some(POPOVER_MENU_KEY_CONTEXT)),
    ]
}

/// Contextual keyboard behavior for an open horizontal menu, Base UI's Root `orientation`.
///
/// A menubar row moves its highlight with Left and Right, and opens the highlighted submenu with
/// Down. Home/End, Enter/Space, Escape, and typeahead behave exactly as in a vertical menu, so a
/// menubar and its dropped-down menus share one model.
pub fn popover_menu_horizontal_key_bindings() -> [KeyBinding; 9] {
    [
        KeyBinding::new(
            "left",
            PopoverMenuPrevious,
            Some(POPOVER_MENU_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "right",
            PopoverMenuNext,
            Some(POPOVER_MENU_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "home",
            PopoverMenuFirst,
            Some(POPOVER_MENU_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "end",
            PopoverMenuLast,
            Some(POPOVER_MENU_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "enter",
            PopoverMenuActivate,
            Some(POPOVER_MENU_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "space",
            PopoverMenuActivate,
            Some(POPOVER_MENU_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "down",
            PopoverMenuOpenSubmenu,
            Some(POPOVER_MENU_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "up",
            PopoverMenuClose,
            Some(POPOVER_MENU_HORIZONTAL_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "escape",
            PopoverMenuClose,
            Some(POPOVER_MENU_HORIZONTAL_KEY_CONTEXT),
        ),
    ]
}

/// Which axis an unstyled menu's roving highlight moves along, Base UI's Root `orientation`.
///
/// The orientation selects the installed key context, so one [`PopoverMenu`] model drives both a
/// vertical dropdown and a horizontal menubar row without a second navigation implementation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MenuOrientation {
    #[default]
    Vertical,
    Horizontal,
}

impl MenuOrientation {
    /// The key context an open menu of this orientation installs.
    pub const fn key_context(self) -> &'static str {
        match self {
            Self::Vertical => POPOVER_MENU_KEY_CONTEXT,
            Self::Horizontal => POPOVER_MENU_HORIZONTAL_KEY_CONTEXT,
        }
    }

    /// The orientation projected into the native accessibility tree.
    pub const fn accessibility_orientation(self) -> AccessibilityOrientation {
        match self {
            Self::Vertical => AccessibilityOrientation::Vertical,
            Self::Horizontal => AccessibilityOrientation::Horizontal,
        }
    }

    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::Horizontal)
    }
}

/// The structural role of one unstyled popover-menu entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PopoverMenuItemKind {
    Action,
    Checkbox,
    Radio,
    /// A command whose typed action carries a destination URL, Base UI's `Menu.LinkItem`.
    Link,
    Submenu,
    Separator,
    GroupLabel,
}

#[derive(Clone)]
enum PopoverMenuCommand {
    Fixed(AnyAction),
    Toggle(Rc<dyn Fn(bool) -> AnyAction>),
}

impl PopoverMenuCommand {
    fn resolve(&self, checked: Option<bool>) -> AnyAction {
        match self {
            Self::Fixed(action) => action.clone(),
            Self::Toggle(action) => action(checked.unwrap_or(false)),
        }
    }
}

/// A typed action dispatched when an unstyled menu link item is activated.
///
/// Base UI's `Menu.LinkItem` renders an anchor and lets the browser navigate. QuickGUI has no
/// document to navigate, so the same intent arrives as an ordinary menu command: the item keeps
/// `menuitem` semantics and its activation dispatches this action through the normal typed-action
/// path, where the application decides what opening the destination means — usually
/// [`EventContext::open_url`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenMenuLink {
    /// The destination the activated item declared, bounded by [`MAX_POPOVER_MENU_LINK_BYTES`].
    pub url: Arc<str>,
}

#[derive(Clone)]
enum PopoverMenuItemContent {
    Action(PopoverMenuCommand),
    Link {
        url: Arc<str>,
        command: PopoverMenuCommand,
    },
    Checkbox {
        checked: bool,
        command: PopoverMenuCommand,
    },
    Radio {
        group: ElementId,
        selected: bool,
        command: PopoverMenuCommand,
    },
    Submenu(Box<PopoverMenu>),
    Separator,
    GroupLabel,
}

/// One retained, appearance-free entry in a [`PopoverMenu`].
///
/// Interactive entries require a stable ID. The ID is namespaced by the menu root when converted
/// into an element, so the same application command ID can safely appear in different submenus.
#[derive(Clone)]
pub struct PopoverMenuItem {
    id: Option<ElementId>,
    label: Arc<str>,
    search_label: Option<Arc<str>>,
    shortcut: Option<Arc<str>>,
    disabled: bool,
    close_on_activate: bool,
    content: PopoverMenuItemContent,
}

impl fmt::Debug for PopoverMenuItem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PopoverMenuItem")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("shortcut", &self.shortcut)
            .field("disabled", &self.disabled)
            .field("close_on_activate", &self.close_on_activate)
            .field("kind", &self.kind())
            .finish_non_exhaustive()
    }
}

impl PopoverMenuItem {
    /// Create an ordinary command item that closes the menu after dispatch by default.
    pub fn action<A: Action>(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        action: A,
    ) -> Self {
        Self::action_any(id, label, AnyAction::new(action))
    }

    /// Create an ordinary command item from a heterogeneous command registry.
    pub fn action_any(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        action: AnyAction,
    ) -> Self {
        Self::interactive(
            id.into(),
            label.into(),
            true,
            PopoverMenuItemContent::Action(PopoverMenuCommand::Fixed(action)),
        )
    }

    /// Create a link item whose activation dispatches [`OpenMenuLink`], Base UI's `Menu.LinkItem`.
    ///
    /// The row keeps ordinary `menuitem` semantics and closes the menu after dispatch, exactly
    /// like a command item. QuickGUI never opens the destination itself: the typed action reaches
    /// the same owner path as every other menu command, so one application listener decides
    /// whether a link opens in the browser, in a document window, or not at all.
    pub fn link(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        url: impl Into<Arc<str>>,
    ) -> Self {
        let url: Arc<str> = url.into();
        Self::interactive(
            id.into(),
            label.into(),
            true,
            PopoverMenuItemContent::Link {
                url: Arc::clone(&url),
                command: PopoverMenuCommand::Fixed(AnyAction::new(OpenMenuLink { url })),
            },
        )
    }

    /// Create a checkbox item. Checkbox items remain open after activation by default.
    pub fn checkbox<A: Action>(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        checked: bool,
        action: A,
    ) -> Self {
        Self::checkbox_any(id, label, checked, AnyAction::new(action))
    }

    /// Create a checkbox item from a heterogeneous command registry.
    pub fn checkbox_any(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        checked: bool,
        action: AnyAction,
    ) -> Self {
        Self::interactive(
            id.into(),
            label.into(),
            false,
            PopoverMenuItemContent::Checkbox {
                checked,
                command: PopoverMenuCommand::Fixed(action),
            },
        )
    }

    /// Create a checkbox whose typed action contains the newly toggled value.
    pub fn checkbox_with<A, F>(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        checked: bool,
        action: F,
    ) -> Self
    where
        A: Action,
        F: Fn(bool) -> A + 'static,
    {
        Self::interactive(
            id.into(),
            label.into(),
            false,
            PopoverMenuItemContent::Checkbox {
                checked,
                command: PopoverMenuCommand::Toggle(Rc::new(move |checked| {
                    AnyAction::new(action(checked))
                })),
            },
        )
    }

    /// Create one radio item. Radio items remain open after activation by default.
    pub fn radio<A: Action>(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        group: impl Into<ElementId>,
        selected: bool,
        action: A,
    ) -> Self {
        let group = group.into();
        Self::interactive(
            id.into(),
            label.into(),
            false,
            PopoverMenuItemContent::Radio {
                group,
                selected,
                command: PopoverMenuCommand::Fixed(AnyAction::new(action)),
            },
        )
    }

    /// Create an item that opens another validated popover-menu model.
    pub fn submenu(
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        submenu: PopoverMenu,
    ) -> Self {
        Self::interactive(
            id.into(),
            label.into(),
            false,
            PopoverMenuItemContent::Submenu(Box::new(submenu)),
        )
    }

    /// Create a visual separator. The caller supplies its complete appearance.
    pub fn separator() -> Self {
        Self {
            id: None,
            label: Arc::from(""),
            search_label: None,
            shortcut: None,
            disabled: false,
            close_on_activate: false,
            content: PopoverMenuItemContent::Separator,
        }
    }

    /// Create a non-interactive label for an application-composed semantic group.
    pub fn group_label(label: impl Into<Arc<str>>) -> Self {
        Self {
            id: None,
            label: label.into(),
            search_label: None,
            shortcut: None,
            disabled: false,
            close_on_activate: false,
            content: PopoverMenuItemContent::GroupLabel,
        }
    }

    fn interactive(
        id: ElementId,
        label: Arc<str>,
        close_on_activate: bool,
        content: PopoverMenuItemContent,
    ) -> Self {
        let search_label = Arc::from(label.to_lowercase());
        Self {
            id: Some(id),
            label,
            search_label: Some(search_label),
            shortcut: None,
            disabled: false,
            close_on_activate,
            content,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn shortcut(mut self, shortcut: impl Into<Arc<str>>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// Override the label used for alphanumeric keyboard navigation.
    pub fn typeahead_label(mut self, label: impl Into<Arc<str>>) -> Self {
        if self.is_interactive() {
            let label: Arc<str> = label.into();
            self.search_label = Some(Arc::from(label.to_lowercase()));
        }
        self
    }

    pub const fn close_on_activate(mut self, close: bool) -> Self {
        self.close_on_activate = close;
        self
    }

    pub const fn id(&self) -> Option<ElementId> {
        self.id
    }

    pub fn label(&self) -> &Arc<str> {
        &self.label
    }

    pub fn shortcut_text(&self) -> Option<&Arc<str>> {
        self.shortcut.as_ref()
    }

    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub const fn closes_on_activate(&self) -> bool {
        self.close_on_activate
    }

    pub const fn kind(&self) -> PopoverMenuItemKind {
        match self.content {
            PopoverMenuItemContent::Action(_) => PopoverMenuItemKind::Action,
            PopoverMenuItemContent::Link { .. } => PopoverMenuItemKind::Link,
            PopoverMenuItemContent::Checkbox { .. } => PopoverMenuItemKind::Checkbox,
            PopoverMenuItemContent::Radio { .. } => PopoverMenuItemKind::Radio,
            PopoverMenuItemContent::Submenu(_) => PopoverMenuItemKind::Submenu,
            PopoverMenuItemContent::Separator => PopoverMenuItemKind::Separator,
            PopoverMenuItemContent::GroupLabel => PopoverMenuItemKind::GroupLabel,
        }
    }

    pub const fn is_interactive(&self) -> bool {
        matches!(
            self.content,
            PopoverMenuItemContent::Action(_)
                | PopoverMenuItemContent::Link { .. }
                | PopoverMenuItemContent::Checkbox { .. }
                | PopoverMenuItemContent::Radio { .. }
                | PopoverMenuItemContent::Submenu(_)
        )
    }

    pub const fn checked(&self) -> Option<bool> {
        match self.content {
            PopoverMenuItemContent::Checkbox { checked, .. } => Some(checked),
            PopoverMenuItemContent::Radio { selected, .. } => Some(selected),
            _ => None,
        }
    }

    pub fn submenu_menu(&self) -> Option<&PopoverMenu> {
        match &self.content {
            PopoverMenuItemContent::Submenu(menu) => Some(menu),
            _ => None,
        }
    }

    /// The destination declared by a link item, or `None` for every other kind.
    pub fn link_url(&self) -> Option<&Arc<str>> {
        match &self.content {
            PopoverMenuItemContent::Link { url, .. } => Some(url),
            _ => None,
        }
    }

    /// The radio group this item belongs to, or `None` for every other kind.
    pub const fn radio_group(&self) -> Option<ElementId> {
        match self.content {
            PopoverMenuItemContent::Radio { group, .. } => Some(group),
            _ => None,
        }
    }
}

/// A construction error that prevents unbounded or ambiguous retained menu state.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PopoverMenuError {
    #[error("a popover menu tree cannot retain more than {MAX_POPOVER_MENU_ITEMS} entries")]
    TooManyItems,
    #[error("a popover menu tree cannot nest deeper than {MAX_POPOVER_MENU_DEPTH} levels")]
    TooDeep,
    #[error(
        "one popover-menu label, shortcut, or typeahead label cannot exceed {MAX_POPOVER_MENU_ITEM_TEXT_BYTES} UTF-8 bytes"
    )]
    ItemTextTooLong,
    #[error("a popover menu tree cannot retain more than {MAX_POPOVER_MENU_TEXT_BYTES} text bytes")]
    TooMuchText,
    #[error("interactive popover-menu item ID {0:?} appears more than once at one menu level")]
    DuplicateItemId(ElementId),
    #[error("radio group {0:?} contains more than one initially selected item")]
    MultipleSelectedRadioItems(ElementId),
    #[error(
        "one popover-menu link destination cannot exceed {MAX_POPOVER_MENU_LINK_BYTES} UTF-8 bytes"
    )]
    LinkUrlTooLong,
}

/// Result of activating one enabled popover-menu item.
#[derive(Clone, Debug)]
pub enum PopoverMenuActivation {
    Command {
        item_index: usize,
        action: AnyAction,
        close_menu: bool,
    },
    Submenu {
        item_index: usize,
        menu: PopoverMenu,
    },
}

/// Render-state supplied to an application's unstyled item renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PopoverMenuItemState {
    pub highlighted: bool,
    pub disabled: bool,
    pub checked: Option<bool>,
    pub has_submenu: bool,
}

impl PopoverMenuItemState {
    /// The Base UI-named snapshot for this row, given whether its submenu is currently open.
    ///
    /// `submenu_open` is ignored for a row that has no submenu, so a caller that tracks one open
    /// submenu index can pass the same comparison for every row.
    pub const fn part_state(self, submenu_open: bool) -> MenuItemPartState {
        MenuItemPartState {
            highlighted: self.highlighted,
            disabled: self.disabled,
            checked: self.checked,
            open: self.has_submenu && submenu_open,
        }
    }
}

/// A copyable render-state snapshot for one unstyled menu row.
///
/// Base UI publishes this as `data-highlighted`, `data-disabled`, `data-checked`, and — on a
/// submenu trigger — `data-popup-open`. QuickGUI has no style sheet, so the same facts arrive as
/// fields the application styles from. Build one with [`PopoverMenu::item_render_state`] or
/// [`PopoverMenuItemState::part_state`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MenuItemPartState {
    /// Whether the roving highlight is on this row.
    pub highlighted: bool,
    /// Whether the row refuses activation.
    pub disabled: bool,
    /// The checked value of a checkbox or radio row, and `None` for every other kind.
    pub checked: Option<bool>,
    /// Whether this row's submenu is currently open.
    pub open: bool,
}

/// Bounded, zero-idle interaction state for an unstyled popover menu.
///
/// The model owns no window, renderer, timer, task, observer, or theme. Typeahead expiration is
/// evaluated only when the next key arrives. Use [`crate::SystemPopover`] as its native surface
/// when content must extend past the owner window.
#[derive(Clone, Debug)]
pub struct PopoverMenu {
    items: Vec<PopoverMenuItem>,
    active: Option<usize>,
    loop_focus: bool,
    orientation: MenuOrientation,
    typeahead: String,
    typeahead_at: Option<Instant>,
}

impl PopoverMenu {
    pub fn new(items: impl IntoIterator<Item = PopoverMenuItem>) -> Result<Self, PopoverMenuError> {
        let items = items.into_iter().collect::<Vec<_>>();
        validate_menu_tree(&items)?;
        let active = items.iter().position(selectable);
        Ok(Self {
            items,
            active,
            loop_focus: true,
            orientation: MenuOrientation::Vertical,
            typeahead: String::new(),
            typeahead_at: None,
        })
    }

    pub fn items(&self) -> &[PopoverMenuItem] {
        &self.items
    }

    pub const fn active_index(&self) -> Option<usize> {
        self.active
    }

    pub fn active_item(&self) -> Option<&PopoverMenuItem> {
        self.active.and_then(|index| self.items.get(index))
    }

    pub const fn loops_focus(&self) -> bool {
        self.loop_focus
    }

    pub const fn loop_focus(mut self, enabled: bool) -> Self {
        self.loop_focus = enabled;
        self
    }

    /// The axis this menu's roving highlight moves along, Base UI's Root `orientation`.
    pub const fn orientation_value(&self) -> MenuOrientation {
        self.orientation
    }

    /// Move the roving highlight along the other axis, Base UI's Root `orientation`.
    ///
    /// A horizontal menu installs [`POPOVER_MENU_HORIZONTAL_KEY_CONTEXT`] instead of
    /// [`POPOVER_MENU_KEY_CONTEXT`], so Left and Right move between top-level triggers and Down
    /// opens the highlighted submenu. Nothing else about the model changes.
    pub const fn orientation(mut self, orientation: MenuOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Replace the orientation in place, reporting whether it changed.
    pub const fn set_orientation(&mut self, orientation: MenuOrientation) -> bool {
        if matches!(
            (self.orientation, orientation),
            (MenuOrientation::Vertical, MenuOrientation::Vertical)
                | (MenuOrientation::Horizontal, MenuOrientation::Horizontal)
        ) {
            return false;
        }
        self.orientation = orientation;
        true
    }

    /// The item currently checked in one radio group, Base UI's `Menu.RadioGroup` `value`.
    pub fn radio_value(&self, group: impl Into<ElementId>) -> Option<ElementId> {
        let group = group.into();
        self.items
            .iter()
            .find(|item| item.radio_group() == Some(group) && item.checked() == Some(true))
            .and_then(PopoverMenuItem::id)
    }

    /// Check exactly one item in a radio group, Base UI's `Menu.RadioGroup` `onValueChange`.
    ///
    /// Every other item in the group is unchecked in the same update, so a group can never retain
    /// two checked values. Returns whether anything changed; an item ID that is not a radio item
    /// in `group` changes nothing.
    pub fn set_radio_value(
        &mut self,
        group: impl Into<ElementId>,
        value: impl Into<ElementId>,
    ) -> bool {
        let group = group.into();
        let value = value.into();
        if !self
            .items
            .iter()
            .any(|item| item.radio_group() == Some(group) && item.id() == Some(value))
        {
            return false;
        }
        let mut changed = false;
        for item in &mut self.items {
            let item_id = item.id;
            if let PopoverMenuItemContent::Radio {
                group: item_group,
                selected,
                ..
            } = &mut item.content
                && *item_group == group
            {
                let wanted = item_id == Some(value);
                changed |= *selected != wanted;
                *selected = wanted;
            }
        }
        changed
    }

    /// Atomically replace the menu contents while preserving the highlighted stable ID.
    pub fn set_items(
        &mut self,
        items: impl IntoIterator<Item = PopoverMenuItem>,
    ) -> Result<bool, PopoverMenuError> {
        let items = items.into_iter().collect::<Vec<_>>();
        validate_menu_tree(&items)?;
        let previous_id = self.active_item().and_then(PopoverMenuItem::id);
        self.items = items;
        self.active = previous_id
            .and_then(|id| self.items.iter().position(|item| item.id() == Some(id)))
            .filter(|index| selectable(&self.items[*index]))
            .or_else(|| self.items.iter().position(selectable));
        self.clear_typeahead();
        Ok(previous_id != self.active_item().and_then(PopoverMenuItem::id))
    }

    pub fn highlight(&mut self, index: usize) -> bool {
        if self.items.get(index).is_none_or(|item| !selectable(item)) || self.active == Some(index)
        {
            return false;
        }
        self.active = Some(index);
        self.clear_typeahead();
        true
    }

    /// Drop the highlight from `index` when the pointer leaves it, returning whether it changed.
    ///
    /// A native menu shows no highlighted row once the pointer has left the surface, so a row
    /// the pointer lit up goes dark when it leaves; a highlight the keyboard moved elsewhere in
    /// the meantime is left alone.
    pub fn unhighlight(&mut self, index: usize) -> bool {
        if self.active != Some(index) {
            return false;
        }
        self.active = None;
        self.clear_typeahead();
        true
    }

    pub fn select_previous(&mut self) -> bool {
        self.move_active(false)
    }

    pub fn select_next(&mut self) -> bool {
        self.move_active(true)
    }

    pub fn select_first(&mut self) -> bool {
        self.select_boundary(false)
    }

    pub fn select_last(&mut self) -> bool {
        self.select_boundary(true)
    }

    fn select_boundary(&mut self, last: bool) -> bool {
        let next = if last {
            self.items.iter().rposition(selectable)
        } else {
            self.items.iter().position(selectable)
        };
        if next == self.active {
            return false;
        }
        self.active = next;
        self.clear_typeahead();
        true
    }

    fn move_active(&mut self, forward: bool) -> bool {
        let count = self.items.len();
        if count == 0 {
            return false;
        }
        let Some(current) = self.active else {
            return self.select_boundary(!forward);
        };
        for step in 1..=count {
            let candidate = if forward {
                current.saturating_add(step)
            } else {
                current.checked_sub(step).unwrap_or(count)
            };
            let candidate = if candidate >= count {
                if !self.loop_focus {
                    break;
                }
                if forward {
                    candidate % count
                } else {
                    count - (step - current) % count
                }
            } else {
                candidate
            };
            let candidate = candidate % count;
            if selectable(&self.items[candidate]) {
                if candidate == current {
                    return false;
                }
                self.active = Some(candidate);
                self.clear_typeahead();
                return true;
            }
        }
        false
    }

    /// Update bounded typeahead state and highlight the next prefix match.
    ///
    /// `now` is supplied by the caller so deterministic tests do not depend on wall-clock sleeps.
    /// The return value reports whether the highlighted item changed.
    pub fn typeahead(&mut self, value: &str, now: Instant) -> bool {
        let input = normalized_typeahead_input(value);
        if input.is_empty() {
            return false;
        }
        if self.typeahead_at.is_none_or(|previous| {
            now.saturating_duration_since(previous) > POPOVER_MENU_TYPEAHEAD_TIMEOUT
        }) {
            self.typeahead.clear();
        }
        self.typeahead_at = Some(now);
        push_bounded(
            &mut self.typeahead,
            &input,
            MAX_POPOVER_MENU_TYPEAHEAD_BYTES,
        );
        if self.typeahead.is_empty() {
            return false;
        }

        let first = self.typeahead.chars().next();
        let repeated_character = first.is_some()
            && self
                .typeahead
                .chars()
                .all(|character| Some(character) == first);
        let repeated_prefix;
        let prefix = if repeated_character && self.typeahead.chars().count() > 1 {
            repeated_prefix = first
                .expect("a non-empty typeahead has one character")
                .to_string();
            repeated_prefix.as_str()
        } else {
            self.typeahead.as_str()
        };

        let count = self.items.len();
        let start = self.active.map_or(0, |active| (active + 1) % count.max(1));
        for step in 0..count {
            let index = (start + step) % count;
            let item = &self.items[index];
            if !selectable(item)
                || !item
                    .search_label
                    .as_deref()
                    .is_some_and(|label| label.starts_with(prefix))
            {
                continue;
            }
            if self.active == Some(index) {
                return false;
            }
            self.active = Some(index);
            return true;
        }
        false
    }

    pub fn activate_active(&mut self) -> Option<PopoverMenuActivation> {
        self.active.and_then(|index| self.activate(index))
    }

    pub fn activate(&mut self, index: usize) -> Option<PopoverMenuActivation> {
        if self.items.get(index).is_none_or(|item| !selectable(item)) {
            return None;
        }

        let active_item_id = self.items[index].id;
        let radio_group = match self.items[index].content {
            PopoverMenuItemContent::Radio { group, .. } => Some(group),
            _ => None,
        };
        if let Some(group) = radio_group {
            for item in &mut self.items {
                if let PopoverMenuItemContent::Radio {
                    group: item_group,
                    selected,
                    ..
                } = &mut item.content
                    && *item_group == group
                {
                    *selected = item.id == active_item_id;
                }
            }
        }

        let item = &mut self.items[index];
        let checked = match &mut item.content {
            PopoverMenuItemContent::Checkbox { checked, .. } => {
                *checked = !*checked;
                Some(*checked)
            }
            PopoverMenuItemContent::Radio { selected, .. } => Some(*selected),
            _ => None,
        };
        let activation = match &item.content {
            PopoverMenuItemContent::Action(command)
            | PopoverMenuItemContent::Link { command, .. }
            | PopoverMenuItemContent::Checkbox { command, .. }
            | PopoverMenuItemContent::Radio { command, .. } => PopoverMenuActivation::Command {
                item_index: index,
                action: command.resolve(checked),
                close_menu: item.close_on_activate,
            },
            PopoverMenuItemContent::Submenu(menu) => PopoverMenuActivation::Submenu {
                item_index: index,
                menu: menu.as_ref().clone(),
            },
            PopoverMenuItemContent::Separator | PopoverMenuItemContent::GroupLabel => return None,
        };
        self.active = Some(index);
        self.clear_typeahead();
        Some(activation)
    }

    pub fn item_state(&self, index: usize) -> Option<PopoverMenuItemState> {
        let item = self.items.get(index)?;
        Some(PopoverMenuItemState {
            highlighted: self.active == Some(index),
            disabled: item.disabled,
            checked: item.checked(),
            has_submenu: item.kind() == PopoverMenuItemKind::Submenu,
        })
    }

    /// Resolve the retained element identity for an interactive item or structural group label.
    ///
    /// Interactive identity follows the application's stable item ID. A group label has no
    /// command identity, so its accessibility-only identity is derived from the menu root and
    /// bounded structural index. Separators intentionally remain anonymous.
    pub fn item_element_id(
        &self,
        menu_id: impl Into<ElementId>,
        index: usize,
    ) -> Option<ElementId> {
        let item = self.items.get(index)?;
        let menu_id = menu_id.into();
        match (item.id, item.kind()) {
            (Some(item_id), _) => Some(derived_item_id(menu_id, item_id)),
            (None, PopoverMenuItemKind::GroupLabel) => Some(derived_group_label_id(menu_id, index)),
            (None, _) => None,
        }
    }

    /// Decorate a caller-owned menu root with focus, keyboard, drag-region, and accessibility
    /// semantics without adding any appearance.
    pub fn root_with(&self, id: impl Into<ElementId>, root: Element) -> Element {
        let id = id.into();
        let mut root = root
            .id(id)
            .track_focus(FocusHandle::new(id))
            .auto_focus()
            .tab_index(-1)
            .key_context(self.orientation.key_context())
            .accessibility_role(AccessibilityRole::Menu)
            .accessibility_orientation(self.orientation.accessibility_orientation())
            .app_region_no_drag()
            .user_select_none()
            .cursor_default();
        if let Some(active) = self.active
            && let Some(item) = self.item_element_id(id, active)
        {
            root = root.accessibility_active_descendant(item);
        }
        root
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(&self, id: impl Into<ElementId>) -> Element {
        self.root_with(id, crate::div())
    }

    /// Decorate one caller-owned row with its exact menu-item semantics and stable derived ID.
    pub fn item_with(
        &self,
        menu_id: impl Into<ElementId>,
        index: usize,
        item_root: Element,
    ) -> Option<Element> {
        let item = self.items.get(index)?;
        let menu_id = menu_id.into();
        let mut root = item_root
            .app_region_no_drag()
            .user_select_none()
            .cursor_default();
        if item.is_interactive() {
            root = root
                .id(self.item_element_id(menu_id, index)?)
                .clickable()
                .tab_index(-1)
                .disabled(item.disabled)
                .accessibility_label(item.label.clone())
                .cursor_default();
        }
        root = match item.kind() {
            PopoverMenuItemKind::Action | PopoverMenuItemKind::Link => {
                root.accessibility_role(AccessibilityRole::MenuItem)
            }
            PopoverMenuItemKind::Checkbox => root
                .accessibility_role(AccessibilityRole::MenuItemCheckBox)
                .checked(item.checked().unwrap_or(false)),
            PopoverMenuItemKind::Radio => root
                .accessibility_role(AccessibilityRole::MenuItemRadio)
                .checked(item.checked().unwrap_or(false)),
            PopoverMenuItemKind::Submenu => root
                .accessibility_role(AccessibilityRole::MenuItem)
                .accessibility_has_popover(AccessibilityPopover::Menu),
            PopoverMenuItemKind::Separator => root.accessibility_role(AccessibilityRole::Separator),
            PopoverMenuItemKind::GroupLabel => root
                .id(self.item_element_id(menu_id, index)?)
                .accessibility_role(AccessibilityRole::Label)
                .accessibility_label(item.label.clone()),
        };
        Some(root)
    }
    /// Create the unstyled item part. Use [`Self::item_with`] to supply an existing element.
    pub fn item(&self, menu_id: impl Into<ElementId>, index: usize) -> Option<Element> {
        self.item_with(menu_id, index, crate::div())
    }

    /// Decorate an application-composed wrapper around a related item group.
    pub fn group_with(group: Element) -> Element {
        group
            .accessibility_role(AccessibilityRole::Group)
            .app_region_no_drag()
            .cursor_default()
    }
    /// Create the unstyled group part. Use [`Self::group_with`] to supply an existing element.
    pub fn group() -> Element {
        Self::group_with(crate::div())
    }

    /// Decorate a caller-composed item group and name it from one mounted group-label row.
    ///
    /// Returns `None` when `label_index` is not a group label. The caller owns visual nesting and
    /// layout; this helper adds only the native group role and mounted `labelled-by` relationship.
    pub fn labeled_group_with(
        &self,
        menu_id: impl Into<ElementId>,
        label_index: usize,
        group: Element,
    ) -> Option<Element> {
        if self.items.get(label_index)?.kind() != PopoverMenuItemKind::GroupLabel {
            return None;
        }
        let label = self.item_element_id(menu_id, label_index)?;
        Some(Self::group_with(group).accessibility_labelled_by(label))
    }
    /// Create the unstyled labeled group part. Use [`Self::labeled_group_with`] to supply an existing element.
    pub fn labeled_group(
        &self,
        menu_id: impl Into<ElementId>,
        label_index: usize,
    ) -> Option<Element> {
        self.labeled_group_with(menu_id, label_index, crate::div())
    }

    /// The Base UI-named render snapshot for one row.
    ///
    /// Returns `None` for an index outside the retained model. `submenu_open` reports whether this
    /// row's submenu is currently mounted; it is ignored for rows that have none.
    pub fn item_render_state(&self, index: usize, submenu_open: bool) -> Option<MenuItemPartState> {
        Some(self.item_state(index)?.part_state(submenu_open))
    }

    /// Decorate a checkbox row, Base UI's `Menu.CheckboxItem`.
    ///
    /// This is the kind-checked Base UI-named alias of [`Self::item_with`]: it returns `None` when
    /// `index` is not a checkbox row, so a composition that renders parts by name cannot silently
    /// mount the wrong semantics.
    pub fn checkbox_item_with(
        &self,
        menu_id: impl Into<ElementId>,
        index: usize,
        item_root: Element,
    ) -> Option<Element> {
        self.kind_checked_part(menu_id, index, PopoverMenuItemKind::Checkbox, item_root)
    }
    /// Create the unstyled checkbox item part. Use [`Self::checkbox_item_with`] to supply an existing element.
    pub fn checkbox_item(&self, menu_id: impl Into<ElementId>, index: usize) -> Option<Element> {
        self.checkbox_item_with(menu_id, index, crate::div())
    }

    /// Decorate a radio row, Base UI's `Menu.RadioItem`.
    pub fn radio_item_with(
        &self,
        menu_id: impl Into<ElementId>,
        index: usize,
        item_root: Element,
    ) -> Option<Element> {
        self.kind_checked_part(menu_id, index, PopoverMenuItemKind::Radio, item_root)
    }
    /// Create the unstyled radio item part. Use [`Self::radio_item_with`] to supply an existing element.
    pub fn radio_item(&self, menu_id: impl Into<ElementId>, index: usize) -> Option<Element> {
        self.radio_item_with(menu_id, index, crate::div())
    }

    /// Decorate a link row, Base UI's `Menu.LinkItem`.
    pub fn link_item_with(
        &self,
        menu_id: impl Into<ElementId>,
        index: usize,
        item_root: Element,
    ) -> Option<Element> {
        self.kind_checked_part(menu_id, index, PopoverMenuItemKind::Link, item_root)
    }
    /// Create the unstyled link item part. Use [`Self::link_item_with`] to supply an existing element.
    pub fn link_item(&self, menu_id: impl Into<ElementId>, index: usize) -> Option<Element> {
        self.link_item_with(menu_id, index, crate::div())
    }

    /// Decorate a submenu row, Base UI's `Menu.SubmenuTrigger`.
    ///
    /// The row already declares `has-popup` and `menuitem`; `open` adds the expanded state that
    /// Base UI publishes as `data-popup-open` while the child level is mounted.
    pub fn submenu_trigger_with(
        &self,
        menu_id: impl Into<ElementId>,
        index: usize,
        open: bool,
        item_root: Element,
    ) -> Option<Element> {
        let part =
            self.kind_checked_part(menu_id, index, PopoverMenuItemKind::Submenu, item_root)?;
        Some(part.accessibility_expanded(open))
    }
    /// Create the unstyled submenu trigger part. Use [`Self::submenu_trigger_with`] to supply an existing element.
    pub fn submenu_trigger(
        &self,
        menu_id: impl Into<ElementId>,
        index: usize,
        open: bool,
    ) -> Option<Element> {
        self.submenu_trigger_with(menu_id, index, open, crate::button())
    }

    /// Decorate a separator row, Base UI's `Menu.Separator`.
    pub fn separator_with(
        &self,
        menu_id: impl Into<ElementId>,
        index: usize,
        item_root: Element,
    ) -> Option<Element> {
        self.kind_checked_part(menu_id, index, PopoverMenuItemKind::Separator, item_root)
    }
    /// Create the unstyled separator part. Use [`Self::separator_with`] to supply an existing element.
    pub fn separator(&self, menu_id: impl Into<ElementId>, index: usize) -> Option<Element> {
        self.separator_with(menu_id, index, crate::div())
    }

    /// Decorate a group-label row, Base UI's `Menu.GroupLabel`.
    pub fn group_label_with(
        &self,
        menu_id: impl Into<ElementId>,
        index: usize,
        item_root: Element,
    ) -> Option<Element> {
        self.kind_checked_part(menu_id, index, PopoverMenuItemKind::GroupLabel, item_root)
    }
    /// Create the unstyled group label part. Use [`Self::group_label_with`] to supply an existing element.
    pub fn group_label(&self, menu_id: impl Into<ElementId>, index: usize) -> Option<Element> {
        self.group_label_with(menu_id, index, crate::div())
    }

    fn kind_checked_part(
        &self,
        menu_id: impl Into<ElementId>,
        index: usize,
        kind: PopoverMenuItemKind,
        item_root: Element,
    ) -> Option<Element> {
        if self.items.get(index)?.kind() != kind {
            return None;
        }
        self.item_with(menu_id, index, item_root)
    }

    /// Decorate a caller-composed radio group, Base UI's `Menu.RadioGroup`.
    ///
    /// The group carries the native radio-group role so assistive technology reports "one of
    /// three" for its items. Membership itself stays in the model: every row built with
    /// [`PopoverMenuItem::radio`] names the group it belongs to, and
    /// [`Self::set_radio_value`] keeps exactly one of them checked.
    pub fn radio_group_with(group: Element) -> Element {
        group
            .accessibility_role(AccessibilityRole::RadioGroup)
            .app_region_no_drag()
            .cursor_default()
    }
    /// Create the unstyled radio group part. Use [`Self::radio_group_with`] to supply an existing element.
    pub fn radio_group() -> Element {
        Self::radio_group_with(crate::div())
    }

    /// Decorate a caller-composed radio group and name it from one mounted group-label row.
    ///
    /// Returns `None` when `label_index` is not a group label.
    pub fn labeled_radio_group_with(
        &self,
        menu_id: impl Into<ElementId>,
        label_index: usize,
        group: Element,
    ) -> Option<Element> {
        if self.items.get(label_index)?.kind() != PopoverMenuItemKind::GroupLabel {
            return None;
        }
        let label = self.item_element_id(menu_id, label_index)?;
        Some(Self::radio_group_with(group).accessibility_labelled_by(label))
    }
    /// Create the unstyled labeled radio group part. Use [`Self::labeled_radio_group_with`] to supply an existing element.
    pub fn labeled_radio_group(
        &self,
        menu_id: impl Into<ElementId>,
        label_index: usize,
    ) -> Option<Element> {
        self.labeled_radio_group_with(menu_id, label_index, crate::div())
    }

    /// Decorate a caller-owned checkbox indicator, Base UI's `Menu.CheckboxItemIndicator`.
    ///
    /// The indicator is decoration: the row it sits in already carries the checked state, so
    /// mounting a second announcement would make assistive technology read the value twice. Mount
    /// it only while the row is checked, exactly as Base UI does.
    pub fn checkbox_item_indicator_with(indicator: Element) -> Element {
        indicator.accessibility_hidden(true).app_region_no_drag()
    }
    /// Create the unstyled checkbox item indicator part. Use [`Self::checkbox_item_indicator_with`] to supply an existing element.
    pub fn checkbox_item_indicator() -> Element {
        Self::checkbox_item_indicator_with(crate::div())
    }

    /// Decorate a caller-owned radio indicator, Base UI's `Menu.RadioItemIndicator`.
    pub fn radio_item_indicator_with(indicator: Element) -> Element {
        Self::checkbox_item_indicator_with(indicator)
    }
    /// Create the unstyled radio item indicator part. Use [`Self::radio_item_indicator_with`] to supply an existing element.
    pub fn radio_item_indicator() -> Element {
        Self::radio_item_indicator_with(crate::div())
    }

    /// Build the complete unstyled interaction layer for one menu level.
    ///
    /// `render_item` owns all row geometry, colors, indicators, icons, and typography. Commands
    /// dispatch through the popover owner's ordinary typed-action path before `dismiss` runs.
    pub fn element<V, Render, Dismiss>(
        &self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        access: fn(&mut V) -> &mut PopoverMenu,
        root: Element,
        render_item: Render,
        dismiss: Dismiss,
    ) -> Element
    where
        V: 'static,
        Render: Fn(&PopoverMenuItem, PopoverMenuItemState) -> Element,
        Dismiss: Fn(&mut V, &mut EventContext) + Clone + 'static,
    {
        self.element_with_submenus(
            cx,
            id,
            access,
            root,
            render_item,
            dismiss,
            |_view, _anchor, _menu, _cx| {},
        )
    }

    /// Build the complete unstyled interaction layer and expose submenu activation to the owner.
    #[allow(clippy::too_many_arguments)]
    pub fn element_with_submenus<V, Render, Dismiss, OpenSubmenu>(
        &self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        access: fn(&mut V) -> &mut PopoverMenu,
        root: Element,
        render_item: Render,
        dismiss: Dismiss,
        open_submenu: OpenSubmenu,
    ) -> Element
    where
        V: 'static,
        Render: Fn(&PopoverMenuItem, PopoverMenuItemState) -> Element,
        Dismiss: Fn(&mut V, &mut EventContext) + Clone + 'static,
        OpenSubmenu: Fn(&mut V, ElementId, PopoverMenu, &mut EventContext) + Clone + 'static,
    {
        self.element_with_submenus_and_hover(
            cx,
            id,
            access,
            root,
            render_item,
            dismiss,
            open_submenu,
            move |view, index, hovered, cx| {
                let menu = access(view);
                let changed = if hovered {
                    menu.highlight(index)
                } else {
                    menu.unhighlight(index)
                };
                if changed {
                    cx.invalidate();
                }
            },
        )
    }

    /// Build the complete unstyled interaction layer with owner-controlled hover behavior.
    ///
    /// This is the native-overflow adapter hook for delayed submenu opening and menu-aim safe
    /// corridors. `hover_item` receives entry and exit transitions for enabled interactive rows;
    /// it owns highlighting and any delayed work. The ordinary [`Self::element_with_submenus`]
    /// method preserves immediate-highlight behavior without allocating a timer.
    #[allow(clippy::too_many_arguments)]
    pub fn element_with_submenus_and_hover<V, Render, Dismiss, OpenSubmenu, HoverItem>(
        &self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        access: fn(&mut V) -> &mut PopoverMenu,
        root: Element,
        render_item: Render,
        dismiss: Dismiss,
        open_submenu: OpenSubmenu,
        hover_item: HoverItem,
    ) -> Element
    where
        V: 'static,
        Render: Fn(&PopoverMenuItem, PopoverMenuItemState) -> Element,
        Dismiss: Fn(&mut V, &mut EventContext) + Clone + 'static,
        OpenSubmenu: Fn(&mut V, ElementId, PopoverMenu, &mut EventContext) + Clone + 'static,
        HoverItem: Fn(&mut V, usize, bool, &mut EventContext) + Clone + 'static,
    {
        let id = id.into();
        let focus = FocusHandle::new(id);

        let previous = cx.action_listener(id, move |view, _: &PopoverMenuPrevious, cx| {
            if access(view).select_previous() {
                cx.invalidate();
            }
        });
        let next = cx.action_listener(id, move |view, _: &PopoverMenuNext, cx| {
            if access(view).select_next() {
                cx.invalidate();
            }
        });
        let first = cx.action_listener(id, move |view, _: &PopoverMenuFirst, cx| {
            if access(view).select_first() {
                cx.invalidate();
            }
        });
        let last = cx.action_listener(id, move |view, _: &PopoverMenuLast, cx| {
            if access(view).select_last() {
                cx.invalidate();
            }
        });

        let activate_dismiss = dismiss.clone();
        let activate_submenu = open_submenu.clone();
        let activate = cx.action_listener(id, move |view, _: &PopoverMenuActivate, cx| {
            if let Some(index) = access(view).active_index() {
                handle_activation(
                    view,
                    index,
                    PopoverMenuActivationHandler {
                        menu_id: id,
                        focus,
                        access,
                        dismiss: &activate_dismiss,
                        open_submenu: &activate_submenu,
                    },
                    cx,
                );
            }
        });

        let keyboard_submenu = open_submenu.clone();
        let open = cx.action_listener(id, move |view, _: &PopoverMenuOpenSubmenu, cx| {
            let Some(index) = access(view).active_index() else {
                return;
            };
            let Some(PopoverMenuActivation::Submenu { menu, .. }) = access(view).activate(index)
            else {
                return;
            };
            let Some(anchor) = access(view).item_element_id(id, index) else {
                return;
            };
            keyboard_submenu(view, anchor, menu, cx);
        });

        let close_dismiss = dismiss.clone();
        let close = cx.action_listener(id, move |view, _: &PopoverMenuClose, cx| {
            close_dismiss(view, cx);
            cx.invalidate();
        });

        let typeahead = cx.key_down_listener(id, move |view, event, cx| {
            if event
                .modifiers
                .intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER)
            {
                return;
            }
            let key = event.key_char.as_ref().unwrap_or(&event.key);
            let Key::Character(value) = key else {
                return;
            };
            if normalized_typeahead_input(value).is_empty() {
                return;
            }
            if access(view).typeahead(value, Instant::now()) {
                cx.invalidate();
            }
            cx.prevent_default();
            cx.stop_propagation();
        });

        let mut children = Vec::with_capacity(self.items.len());
        for (index, item) in self.items.iter().enumerate() {
            let state = self
                .item_state(index)
                .expect("a retained popover-menu item has render state");
            let mut row = render_item(item, state);
            if item.is_interactive() && !item.disabled {
                let row_id = self
                    .item_element_id(id, index)
                    .expect("an interactive popover-menu item has a stable ID");
                let hover_item = hover_item.clone();
                let hover = cx.hover_listener(row_id, move |view, hovered, cx| {
                    hover_item(view, index, *hovered, cx);
                });
                let click_dismiss = dismiss.clone();
                let click_submenu = open_submenu.clone();
                let click = cx.listener(row_id, move |view, cx| {
                    handle_activation(
                        view,
                        index,
                        PopoverMenuActivationHandler {
                            menu_id: id,
                            focus,
                            access,
                            dismiss: &click_dismiss,
                            open_submenu: &click_submenu,
                        },
                        cx,
                    );
                });
                row = row.on_hover(hover).on_click(click);
            }
            children.push(
                self.item_with(id, index, row)
                    .expect("a retained popover-menu index remains valid during rendering"),
            );
        }

        self.root_with(id, root)
            .on_action(previous)
            .on_action(next)
            .on_action(first)
            .on_action(last)
            .on_action(activate)
            .on_action(open)
            .on_action(close)
            .on_key_down(typeahead)
            .children(children)
    }

    fn clear_typeahead(&mut self) {
        self.typeahead.clear();
        self.typeahead_at = None;
    }
}

struct PopoverMenuActivationHandler<'a, V, Dismiss, OpenSubmenu> {
    menu_id: ElementId,
    focus: FocusHandle,
    access: fn(&mut V) -> &mut PopoverMenu,
    dismiss: &'a Dismiss,
    open_submenu: &'a OpenSubmenu,
}

fn handle_activation<V, Dismiss, OpenSubmenu>(
    view: &mut V,
    index: usize,
    handler: PopoverMenuActivationHandler<'_, V, Dismiss, OpenSubmenu>,
    cx: &mut EventContext,
) where
    Dismiss: Fn(&mut V, &mut EventContext),
    OpenSubmenu: Fn(&mut V, ElementId, PopoverMenu, &mut EventContext),
{
    let Some(activation) = (handler.access)(view).activate(index) else {
        return;
    };
    match activation {
        PopoverMenuActivation::Command {
            action, close_menu, ..
        } => {
            let delivered = if cx.popover_owner_window_handle().is_some() {
                cx.dispatch_any_action_to_popover_owner(action)
            } else {
                cx.dispatch_any_action(action);
                true
            };
            if delivered && close_menu {
                (handler.dismiss)(view, cx);
            } else {
                cx.focus(handler.focus);
            }
            cx.invalidate();
        }
        PopoverMenuActivation::Submenu { menu, .. } => {
            let Some(anchor) = (handler.access)(view).item_element_id(handler.menu_id, index)
            else {
                return;
            };
            (handler.open_submenu)(view, anchor, menu, cx);
        }
    }
}

fn selectable(item: &PopoverMenuItem) -> bool {
    item.is_interactive() && !item.disabled
}

fn validate_menu_tree(items: &[PopoverMenuItem]) -> Result<(), PopoverMenuError> {
    let mut count = 0_usize;
    let mut text_bytes = 0_usize;
    validate_menu_level(items, 1, &mut count, &mut text_bytes)
}

fn validate_menu_level(
    items: &[PopoverMenuItem],
    depth: usize,
    count: &mut usize,
    text_bytes: &mut usize,
) -> Result<(), PopoverMenuError> {
    if depth > MAX_POPOVER_MENU_DEPTH {
        return Err(PopoverMenuError::TooDeep);
    }
    *count = count.saturating_add(items.len());
    if *count > MAX_POPOVER_MENU_ITEMS {
        return Err(PopoverMenuError::TooManyItems);
    }

    let mut ids = HashSet::with_capacity(items.len());
    let mut selected_radios = HashMap::new();
    for item in items {
        if let Some(id) = item.id
            && !ids.insert(id)
        {
            return Err(PopoverMenuError::DuplicateItemId(id));
        }
        add_text_bytes(item.label.len(), text_bytes)?;
        if let Some(label) = &item.search_label {
            add_text_bytes(label.len(), text_bytes)?;
        }
        if let Some(shortcut) = &item.shortcut {
            add_text_bytes(shortcut.len(), text_bytes)?;
        }
        if let PopoverMenuItemContent::Link { url, .. } = &item.content {
            if url.len() > MAX_POPOVER_MENU_LINK_BYTES {
                return Err(PopoverMenuError::LinkUrlTooLong);
            }
            add_text_bytes(url.len(), text_bytes)?;
        }
        if let PopoverMenuItemContent::Radio {
            group,
            selected: true,
            ..
        } = item.content
            && selected_radios.insert(group, item.id).is_some()
        {
            return Err(PopoverMenuError::MultipleSelectedRadioItems(group));
        }
        if let PopoverMenuItemContent::Submenu(menu) = &item.content {
            validate_menu_level(&menu.items, depth + 1, count, text_bytes)?;
        }
    }
    Ok(())
}

fn add_text_bytes(bytes: usize, total: &mut usize) -> Result<(), PopoverMenuError> {
    if bytes > MAX_POPOVER_MENU_ITEM_TEXT_BYTES {
        return Err(PopoverMenuError::ItemTextTooLong);
    }
    *total = total.saturating_add(bytes);
    if *total > MAX_POPOVER_MENU_TEXT_BYTES {
        return Err(PopoverMenuError::TooMuchText);
    }
    Ok(())
}

fn normalized_typeahead_input(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
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

fn derived_item_id(parent: ElementId, item: ElementId) -> ElementId {
    let mut hash =
        parent.as_u64() ^ ITEM_ID_TAG ^ item.as_u64().wrapping_mul(0x9e37_79b9_7f4a_7c15);
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == parent.as_u64() || hash == u64::MAX {
        hash ^= ITEM_ID_TAG.rotate_left(17);
    }
    ElementId::new(hash)
}

fn derived_group_label_id(parent: ElementId, index: usize) -> ElementId {
    debug_assert!(index < MAX_POPOVER_MENU_ITEMS);
    let mut hash = parent.as_u64()
        ^ GROUP_LABEL_ID_TAG
        ^ (index as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == parent.as_u64() || hash == u64::MAX {
        hash ^= GROUP_LABEL_ID_TAG.rotate_left(13);
    }
    ElementId::new(hash)
}

/// A copyable render-state snapshot for one unstyled menu surface.
///
/// Base UI publishes this as `data-open`, `data-side`, `data-align`, and `data-anchor-hidden` on
/// the popup. QuickGUI has no style sheet, so the same facts arrive as fields the application
/// styles from. Build one with [`MenuState::state`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MenuPartState {
    /// Whether the menu surface is currently open.
    pub open: bool,
    /// The side of the anchor the popup was actually placed on.
    pub side: AnchorSide,
    /// The cross-axis alignment the popup actually used.
    pub align: AnchorAlign,
    /// Whether the anchor left the collision viewport entirely.
    pub anchor_hidden: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuHoverPhase {
    Open,
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuHoverPart {
    Trigger,
    Popup,
}

/// Close the menu level above the surface a dismissal started in.
///
/// This is Base UI's `closeParentOnEsc`. Escape reaches exactly one dismiss region — the topmost
/// one — so a submenu that must also close its parent says so with this typed action, which
/// travels the ordinary focus path to the level above and stops at the first level that is
/// already closed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MenuCloseParent;

/// Default dwell before a hovered menu trigger opens, matching Base UI's 100 ms.
pub const DEFAULT_MENU_HOVER_DELAY: Duration = Duration::from_millis(100);
/// Default grace period when the pointer leaves a hover-opened menu trigger.
///
/// This keeps a submenu mounted while the pointer crosses the structural gap into its popup.
pub const DEFAULT_MENU_CLOSE_DELAY: Duration = Duration::from_millis(100);

/// Bounded controlled state for one unstyled menu surface built from Base UI's compound parts.
///
/// [`PopoverMenu`] is the row model — items, highlighting, typeahead, activation. `MenuState` is
/// the surface around it: the controlled open flag, Base UI's `Menu.Root` props, the anchored
/// in-window placement its popup uses, and the exact hover deadlines an `openOnHover` trigger or a
/// submenu trigger needs. Compose the two by mounting [`Self::popup_with`] and putting the model's
/// [`PopoverMenu::element`] tree inside it.
///
/// QuickGUI owns the trigger's semantics and relationships, portal/positioner anchoring with flip
/// and shift, the resolved-placement report the arrow follows, Escape and outside-press dismissal,
/// focus restoration, and modal focus containment. The application owns every visual declaration.
///
/// A closed menu owns no task, timer, observer, or idle scheduler source. An open one owns at most
/// a single pending hover deadline.
pub struct MenuState {
    popover: Popover,
    orientation: MenuOrientation,
    loop_focus: bool,
    close_parent_on_esc: bool,
    disabled: bool,
    open: bool,
    open_on_hover: bool,
    hoverable_popup: bool,
    delay: Duration,
    close_delay: Duration,
    placement_handle: AnchorPlacementHandle,
    trigger_hovered: bool,
    popup_hovered: bool,
    pending: Option<MenuHoverPhase>,
    generation: u64,
    task: Option<Task<()>>,
}

impl fmt::Debug for MenuState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MenuState")
            .field("trigger_id", &self.trigger_id())
            .field("popup_id", &self.popup_id())
            .field("open", &self.open)
            .field("disabled", &self.disabled)
            .field("modal", &self.is_modal())
            .field("orientation", &self.orientation)
            .finish_non_exhaustive()
    }
}

impl MenuState {
    /// Create a closed menu over caller-owned trigger and popup identities.
    pub fn new(trigger_id: impl Into<ElementId>, popup_id: impl Into<ElementId>) -> Self {
        Self {
            popover: Popover::new(trigger_id, popup_id, false)
                .kind(PopoverKind::Menu)
                .placement(AnchorPlacement::BottomStart),
            orientation: MenuOrientation::Vertical,
            loop_focus: true,
            close_parent_on_esc: false,
            disabled: false,
            open: false,
            open_on_hover: false,
            hoverable_popup: true,
            delay: DEFAULT_MENU_HOVER_DELAY,
            close_delay: DEFAULT_MENU_CLOSE_DELAY,
            placement_handle: AnchorPlacementHandle::new(),
            trigger_hovered: false,
            popup_hovered: false,
            pending: None,
            generation: 0,
            task: None,
        }
    }

    /// Declare the menu open or closed, Base UI's Root `open` prop.
    ///
    /// A disabled menu stays closed.
    pub fn with_open(mut self, open: bool) -> Self {
        self.open = open && !self.disabled;
        self
    }

    /// Contain Tab focus inside the popup and expect a mounted backdrop, Base UI's `modal`.
    pub const fn modal(mut self, modal: bool) -> Self {
        self.popover = self.popover.modal(modal);
        self
    }

    /// Move the highlight along the other axis, Base UI's Root `orientation`.
    pub const fn orientation(mut self, orientation: MenuOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Wrap the highlight at the ends of the list, Base UI's Root `loop`.
    pub const fn loop_focus(mut self, loop_focus: bool) -> Self {
        self.loop_focus = loop_focus;
        self
    }

    /// Close the level above too when Escape dismisses this one, Base UI's `closeParentOnEsc`.
    pub const fn close_parent_on_esc(mut self, close_parent: bool) -> Self {
        self.close_parent_on_esc = close_parent;
        self
    }

    /// Refuse to open at all, Base UI's Root `disabled`.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        if disabled {
            self.open = false;
            self.cancel_pending();
        }
        self
    }

    /// Open the menu when the pointer rests on its trigger, Base UI's Trigger `openOnHover`.
    pub const fn open_on_hover(mut self, open_on_hover: bool) -> Self {
        self.open_on_hover = open_on_hover;
        self
    }

    /// Set how long a hovered trigger waits before opening, Base UI's `delay`.
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay.min(MAX_POPOVER_HOVER_DELAY);
        self
    }

    /// Set how long the menu waits before closing once the pointer leaves, Base UI's `closeDelay`.
    /// Defaults to [`DEFAULT_MENU_CLOSE_DELAY`] so the pointer can cross into a submenu popup.
    pub fn close_delay(mut self, close_delay: Duration) -> Self {
        self.close_delay = close_delay.min(MAX_POPOVER_HOVER_DELAY);
        self
    }

    /// Choose whether hovering the popup itself keeps a hover-opened menu open.
    pub const fn hoverable_popup(mut self, hoverable: bool) -> Self {
        self.hoverable_popup = hoverable;
        self
    }

    /// Prefer a full placement for the popup, Base UI's Positioner `side` plus `align`.
    pub const fn placement(mut self, placement: AnchorPlacement) -> Self {
        self.popover = self.popover.placement(placement);
        self
    }

    /// Prefer one side of the anchor, Base UI's Positioner `side`.
    pub const fn side(mut self, side: AnchorSide) -> Self {
        self.popover = self.popover.side(side);
        self
    }

    /// Prefer a cross-axis alignment, Base UI's Positioner `align`.
    pub const fn align(mut self, align: AnchorAlign) -> Self {
        self.popover = self.popover.align(align);
        self
    }

    /// Set the distance between the anchor and the positioner, Base UI's `sideOffset`.
    pub fn side_offset(mut self, offset: f32) -> Self {
        self.popover = self.popover.side_offset(offset);
        self
    }

    /// Shift the popup along its cross axis before collision handling, Base UI's `alignOffset`.
    pub fn align_offset(mut self, offset: f32) -> Self {
        self.popover = self.popover.align_offset(offset);
        self
    }

    /// Set the collision padding kept inside the viewport, Base UI's `collisionPadding`.
    pub fn collision_padding(mut self, padding: f32) -> Self {
        self.popover = self.popover.collision_padding(padding);
        self
    }

    /// Keep the popup clamped inside the viewport, Base UI's `sticky`.
    pub const fn sticky(mut self, sticky: bool) -> Self {
        self.popover = self.popover.sticky(sticky);
        self
    }

    /// Declare the edge length of a caller-painted arrow so QuickGUI can centre it.
    pub fn arrow_size(mut self, size: f32) -> Self {
        self.popover = self.popover.arrow_size(size);
        self
    }

    /// Keep a centred arrow clear of the popup's corners.
    pub fn arrow_padding(mut self, padding: f32) -> Self {
        self.popover = self.popover.arrow_padding(padding);
        self
    }

    pub const fn is_open(&self) -> bool {
        self.open
    }

    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub const fn is_modal(&self) -> bool {
        self.popover.is_modal()
    }

    pub const fn orientation_value(&self) -> MenuOrientation {
        self.orientation
    }

    pub const fn loops_focus(&self) -> bool {
        self.loop_focus
    }

    pub const fn closes_parent_on_esc(&self) -> bool {
        self.close_parent_on_esc
    }

    pub const fn opens_on_hover(&self) -> bool {
        self.open_on_hover
    }

    /// Whether a hover deadline is currently outstanding.
    pub const fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub const fn trigger_id(&self) -> ElementId {
        self.popover.trigger_id()
    }

    pub const fn popup_id(&self) -> ElementId {
        self.popover.popover_id()
    }

    pub fn positioner_id(&self) -> ElementId {
        self.popover.positioner_id()
    }

    pub fn backdrop_id(&self) -> ElementId {
        self.popover.backdrop_id()
    }

    pub fn arrow_id(&self) -> ElementId {
        self.popover.arrow_id()
    }

    /// The composed popover descriptor, for callers that need its remaining parts directly.
    pub fn popover(&self) -> Popover {
        self.popover
            .open(self.open)
            .track_placement(&self.placement_handle)
    }

    /// The side the popup actually opened on, or the declared preference before the first frame.
    pub fn resolved_side(&self) -> AnchorSide {
        self.popover().resolved_side()
    }

    /// The cross-axis alignment the popup actually used, or the declared preference.
    pub fn resolved_align(&self) -> AnchorAlign {
        self.popover().resolved_align()
    }

    /// A copyable render-state snapshot the application styles from.
    pub fn state(&self) -> MenuPartState {
        MenuPartState {
            open: self.open,
            side: self.resolved_side(),
            align: self.resolved_align(),
            anchor_hidden: self
                .placement_handle
                .resolved()
                .is_some_and(|resolved| resolved.anchor_hidden),
        }
    }

    /// Force the open value, cancelling any outstanding deadline. Returns whether it changed.
    ///
    /// A disabled menu never opens.
    pub fn set_open(&mut self, open: bool) -> bool {
        self.cancel_pending();
        if !open {
            self.trigger_hovered = false;
            self.popup_hovered = false;
        }
        self.apply(open)
    }

    /// Open immediately, cancelling any outstanding deadline.
    pub fn open_now(&mut self) -> bool {
        self.set_open(true)
    }

    /// Close immediately, cancelling any outstanding deadline.
    pub fn close_now(&mut self) -> bool {
        self.set_open(false)
    }

    /// Toggle immediately, cancelling any outstanding deadline.
    pub fn toggle(&mut self) -> bool {
        self.set_open(!self.open)
    }

    /// Drop any outstanding hover deadline without changing the open state.
    pub fn cancel_pending(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        if let Some(task) = self.task.take() {
            task.cancel();
        }
    }

    /// Decorate the optional application-owned structural wrapper, Base UI's `Menu.Root`.
    pub fn root_with(&self, root: Element) -> Element {
        root.app_region_no_drag()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(&self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate a nested level's structural wrapper, Base UI's `Menu.SubmenuRoot`.
    pub fn submenu_root_with(&self, root: Element) -> Element {
        self.root_with(root)
    }
    /// Create the unstyled submenu root part. Use [`Self::submenu_root_with`] to supply an existing element.
    pub fn submenu_root(&self) -> Element {
        self.submenu_root_with(crate::div())
    }

    /// Decorate a caller-owned trigger, Base UI's `Menu.Trigger`.
    ///
    /// Clicking toggles the menu. With [`Self::open_on_hover`] the pointer opens it after
    /// [`Self::delay`] and closes it after [`Self::close_delay`], both exact one-shot deadlines: a
    /// pointer that returns before the deadline cancels it rather than reopening, and a zero delay
    /// applies the change in the same controlled update with no task at all.
    pub fn trigger_with<V: 'static, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut Self,
        on_open_change: Change,
        trigger: Element,
    ) -> Element
    where
        Change: Fn(&mut V, bool, &mut EventContext) + Clone + 'static,
    {
        self.trigger_with_accessor(cx, StateAccessor::from(access), on_open_change, trigger)
    }
    /// Create the unstyled trigger part. Use [`Self::trigger_with`] to supply an existing element.
    pub fn trigger<V: 'static, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut Self,
        on_open_change: Change,
    ) -> Element
    where
        Change: Fn(&mut V, bool, &mut EventContext) + Clone + 'static,
    {
        self.trigger_with(cx, access, on_open_change, crate::button())
    }

    /// Decorate a caller-owned trigger through a per-instance accessor.
    pub fn trigger_with_accessor<V: 'static, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, Self>,
        on_open_change: Change,
        trigger: Element,
    ) -> Element
    where
        Change: Fn(&mut V, bool, &mut EventContext) + Clone + 'static,
    {
        let trigger = self
            .popover()
            .trigger_with(trigger)
            .disabled(self.disabled)
            .accessibility_has_popover(AccessibilityPopover::Menu);
        self.interactive_trigger(cx, access, on_open_change, trigger)
    }

    /// Decorate a caller-owned submenu trigger, Base UI's `Menu.SubmenuTrigger`.
    ///
    /// A submenu trigger is a row of its parent menu, so it keeps `menuitem` semantics rather than
    /// button semantics. Declare [`Self::open_on_hover`] for the native dwell-to-open behavior.
    pub fn submenu_trigger_with<V: 'static, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut Self,
        on_open_change: Change,
        trigger: Element,
    ) -> Element
    where
        Change: Fn(&mut V, bool, &mut EventContext) + Clone + 'static,
    {
        self.submenu_trigger_with_accessor(cx, StateAccessor::from(access), on_open_change, trigger)
    }
    /// Create the unstyled submenu trigger part. Use [`Self::submenu_trigger_with`] to supply an existing element.
    pub fn submenu_trigger<V: 'static, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut Self,
        on_open_change: Change,
    ) -> Element
    where
        Change: Fn(&mut V, bool, &mut EventContext) + Clone + 'static,
    {
        self.submenu_trigger_with(cx, access, on_open_change, crate::button())
    }

    /// Decorate a caller-owned submenu trigger through a per-instance accessor.
    pub fn submenu_trigger_with_accessor<V: 'static, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, Self>,
        on_open_change: Change,
        trigger: Element,
    ) -> Element
    where
        Change: Fn(&mut V, bool, &mut EventContext) + Clone + 'static,
    {
        let mut trigger = trigger
            .id(self.trigger_id())
            .clickable()
            .tab_index(-1)
            .disabled(self.disabled)
            .accessibility_role(AccessibilityRole::MenuItem)
            .accessibility_has_popover(AccessibilityPopover::Menu)
            .accessibility_expanded(self.open)
            .app_region_no_drag()
            .user_select_none()
            .cursor_default();
        if self.open {
            trigger = trigger.accessibility_controls(self.popup_id());
        }
        self.interactive_trigger(cx, access, on_open_change, trigger)
    }

    fn interactive_trigger<V: 'static, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, Self>,
        on_open_change: Change,
        trigger: Element,
    ) -> Element
    where
        Change: Fn(&mut V, bool, &mut EventContext) + Clone + 'static,
    {
        let click_access = access.clone();
        let click_change = on_open_change.clone();
        let click = cx.listener(self.trigger_id(), move |view, cx| {
            if click_access.get(view).is_disabled() {
                return;
            }
            if click_access.get(view).toggle() {
                let open = click_access.get(view).is_open();
                click_change(view, open, cx);
                if open {
                    cx.focus(FocusHandle::new(click_access.get(view).popup_id()));
                }
                cx.invalidate();
            }
        });
        let trigger = trigger.on_click(click);
        if !self.open_on_hover {
            return trigger;
        }
        let hover = self.hover_listener(
            cx,
            self.trigger_id(),
            MenuHoverPart::Trigger,
            access,
            on_open_change,
        );
        trigger.on_hover(hover)
    }

    /// Decorate the caller-owned portal boundary, Base UI's `Menu.Portal`.
    ///
    /// QuickGUI's retained overlay node is itself the portal, so this is the same boundary as
    /// [`Self::positioner_with`]; mount exactly one of them.
    pub fn portal_with(&self, portal: Element) -> Element {
        self.positioner_with(portal)
    }
    /// Create the unstyled portal part. Use [`Self::portal_with`] to supply an existing element.
    pub fn portal(&self) -> Element {
        self.portal_with(crate::div())
    }

    /// Decorate the caller-owned positioner, Base UI's `Menu.Positioner`.
    ///
    /// The positioner publishes the placement it resolved to, so [`Self::arrow_with`] and
    /// [`Self::state`] follow the real side after a flip instead of the declared preference.
    pub fn positioner_with(&self, positioner: Element) -> Element {
        self.popover()
            .tracked_positioner_with(positioner, &self.placement_handle)
    }
    /// Create the unstyled positioner part. Use [`Self::positioner_with`] to supply an existing element.
    pub fn positioner(&self) -> Element {
        self.positioner_with(crate::div())
    }

    /// Decorate an optional caller-painted viewport backdrop, Base UI's `Menu.Backdrop`.
    pub fn backdrop_with(&self, backdrop: Element) -> Element {
        self.popover().backdrop_with(backdrop)
    }
    /// Create the unstyled backdrop part. Use [`Self::backdrop_with`] to supply an existing element.
    pub fn backdrop(&self) -> Element {
        self.backdrop_with(crate::div())
    }

    /// Position a caller-owned arrow on the edge the popup actually opened against.
    pub fn arrow_with(&self, arrow: Element) -> Element {
        self.popover().arrow_with(arrow)
    }
    /// Create the unstyled arrow part. Use [`Self::arrow_with`] to supply an existing element.
    pub fn arrow(&self) -> Element {
        self.arrow_with(crate::div())
    }

    /// Decorate the caller-owned popup, Base UI's `Menu.Popup`.
    pub fn popup_with<V: 'static, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut Self,
        on_open_change: Change,
        popup: Element,
    ) -> Element
    where
        Change: Fn(&mut V, bool, &mut EventContext) + Clone + 'static,
    {
        self.popup_with_accessor(cx, StateAccessor::from(access), on_open_change, popup)
    }
    /// Create the unstyled popup part. Use [`Self::popup_with`] to supply an existing element.
    pub fn popup<V: 'static, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut Self,
        on_open_change: Change,
    ) -> Element
    where
        Change: Fn(&mut V, bool, &mut EventContext) + Clone + 'static,
    {
        self.popup_with(cx, access, on_open_change, crate::div())
    }

    /// Decorate the caller-owned popup through a per-instance accessor.
    ///
    /// The popup dismisses on Escape and on an outside press and restores trigger focus. When
    /// [`Self::close_parent_on_esc`] is set it additionally dispatches [`MenuCloseParent`], which
    /// travels the focus path to the level above and closes it too.
    pub fn popup_with_accessor<V: 'static, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, Self>,
        on_open_change: Change,
        popup: Element,
    ) -> Element
    where
        Change: Fn(&mut V, bool, &mut EventContext) + Clone + 'static,
    {
        let popup_id = self.popup_id();
        let dismiss = self.dismiss_listener_with(cx, access.clone(), on_open_change.clone());
        let mut popup = self.popover().popup_with(popup).on_dismiss(dismiss);

        let parent_access = access.clone();
        let parent_change = on_open_change.clone();
        let close_parent = cx.action_listener(popup_id, move |view, _: &MenuCloseParent, cx| {
            if !parent_access.get(view).close_now() {
                cx.propagate();
                return;
            }
            parent_change(view, false, cx);
            cx.invalidate();
            if parent_access.get(view).closes_parent_on_esc() {
                cx.dispatch_action(MenuCloseParent);
            }
        });
        popup = popup.on_action(close_parent);

        if self.open_on_hover && self.hoverable_popup {
            let hover =
                self.hover_listener(cx, popup_id, MenuHoverPart::Popup, access, on_open_change);
            popup = popup.on_hover(hover);
        }
        popup
    }

    /// Build the popup's dismissal behavior for Escape and outside presses.
    ///
    /// [`Self::popup_with`] attaches this already; build it directly only when the application
    /// composes the popup element itself.
    pub fn dismiss_listener<V: 'static, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut Self,
        on_open_change: Change,
    ) -> DismissListener<V>
    where
        Change: Fn(&mut V, bool, &mut EventContext) + 'static,
    {
        self.dismiss_listener_with(cx, StateAccessor::from(access), on_open_change)
    }

    /// Build the popup's dismissal behavior against a per-instance accessor.
    pub fn dismiss_listener_with<V: 'static, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, Self>,
        on_open_change: Change,
    ) -> DismissListener<V>
    where
        Change: Fn(&mut V, bool, &mut EventContext) + 'static,
    {
        cx.dismiss_listener(self.popup_id(), move |view, cx| {
            if !access.get(view).close_now() {
                return;
            }
            on_open_change(view, false, cx);
            cx.invalidate();
            if access.get(view).closes_parent_on_esc() {
                cx.dispatch_action(MenuCloseParent);
            }
        })
    }

    fn hover_listener<V: 'static, Change>(
        &self,
        cx: &mut ViewContext<'_, V>,
        id: ElementId,
        part: MenuHoverPart,
        access: StateAccessor<V, Self>,
        on_open_change: Change,
    ) -> HoverListener<V>
    where
        Change: Fn(&mut V, bool, &mut EventContext) + Clone + 'static,
    {
        cx.hover_listener(id, move |view, hovered, cx| {
            let hovered = *hovered;
            let Some((phase, delay)) = access.get(view).note_hover(part, hovered) else {
                return;
            };
            let open = phase == MenuHoverPhase::Open;
            if delay.is_zero() {
                apply_menu_hover(view, &access, open, &on_open_change, cx);
                return;
            }
            let generation = {
                let state = access.get(view);
                state.pending = Some(phase);
                state.generation
            };
            let task_access = access.clone();
            let task_change = on_open_change.clone();
            let spawned = cx.spawn::<V, _, _, _>(move |task_cx: AsyncViewContext<V>| async move {
                if task_cx.sleep(delay).await.is_err() {
                    return;
                }
                let _ = task_cx
                    .update(move |view, cx| {
                        {
                            let state = task_access.get(view);
                            if state.generation != generation || state.pending != Some(phase) {
                                return;
                            }
                            state.pending = None;
                            state.task = None;
                        }
                        apply_menu_hover(view, &task_access, open, &task_change, cx);
                    })
                    .await;
            });
            match spawned {
                Ok(task) => access.get(view).task = Some(task),
                Err(_) => {
                    // A window that cannot own another foreground task still behaves correctly;
                    // only the delay is lost.
                    access.get(view).pending = None;
                    apply_menu_hover(view, &access, open, &on_open_change, cx);
                }
            }
        })
    }

    const fn is_hovered(&self) -> bool {
        self.trigger_hovered || (self.hoverable_popup && self.popup_hovered)
    }

    /// Record a hover transition and report the deadline it arms, if any.
    fn note_hover(
        &mut self,
        part: MenuHoverPart,
        hovered: bool,
    ) -> Option<(MenuHoverPhase, Duration)> {
        match part {
            MenuHoverPart::Trigger => self.trigger_hovered = hovered,
            MenuHoverPart::Popup => self.popup_hovered = hovered,
        }
        if self.disabled {
            return None;
        }
        let wanted = if self.is_hovered() {
            MenuHoverPhase::Open
        } else {
            MenuHoverPhase::Close
        };
        let already = match wanted {
            MenuHoverPhase::Open => self.open,
            MenuHoverPhase::Close => !self.open,
        };
        if already {
            // The pointer returned before the deadline expired: drop it rather than reopening.
            if self.pending.is_some() {
                self.cancel_pending();
            }
            return None;
        }
        if self.pending == Some(wanted) {
            return None;
        }
        self.cancel_pending();
        let delay = match wanted {
            MenuHoverPhase::Open => self.delay,
            MenuHoverPhase::Close => self.close_delay,
        };
        Some((wanted, delay))
    }

    fn apply(&mut self, open: bool) -> bool {
        let open = open && !self.disabled;
        if !open {
            self.placement_handle.clear();
        }
        std::mem::replace(&mut self.open, open) != open
    }
}

fn apply_menu_hover<V: 'static, Change>(
    view: &mut V,
    access: &StateAccessor<V, MenuState>,
    open: bool,
    on_open_change: &Change,
    cx: &mut EventContext,
) where
    Change: Fn(&mut V, bool, &mut EventContext),
{
    if !access.get(view).apply(open) {
        return;
    }
    let after = access.get(view).is_open();
    on_open_change(view, after, cx);
    cx.invalidate();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppRegion, Color, IntoElement, PopoverOptions, Rect, TestAppContext, View, WindowOptions,
        div, text,
    };
    use std::sync::Arc;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Command {
        Open,
        Toggle(bool),
        Theme(&'static str),
    }

    fn sample_menu() -> PopoverMenu {
        PopoverMenu::new([
            PopoverMenuItem::group_label("File"),
            PopoverMenuItem::action("new", "New", Command::Open).shortcut("⌘N"),
            PopoverMenuItem::action("disabled", "Disabled", Command::Open).disabled(true),
            PopoverMenuItem::separator(),
            PopoverMenuItem::checkbox_with("sidebar", "Sidebar", false, Command::Toggle),
            PopoverMenuItem::radio("light", "Light", "theme", true, Command::Theme("light")),
            PopoverMenuItem::radio("dark", "Dark", "theme", false, Command::Theme("dark")),
        ])
        .unwrap()
    }

    #[test]
    fn navigation_skips_structure_and_disabled_items_and_loops_by_default() {
        let mut menu = sample_menu();
        assert_eq!(menu.active_item().unwrap().label().as_ref(), "New");
        assert!(menu.select_next());
        assert_eq!(menu.active_item().unwrap().label().as_ref(), "Sidebar");
        assert!(menu.select_last());
        assert_eq!(menu.active_item().unwrap().label().as_ref(), "Dark");
        assert!(menu.select_next());
        assert_eq!(menu.active_item().unwrap().label().as_ref(), "New");

        let mut no_loop = sample_menu().loop_focus(false);
        assert!(no_loop.select_last());
        assert!(!no_loop.select_next());
        assert_eq!(no_loop.active_item().unwrap().label().as_ref(), "Dark");
    }

    #[test]
    fn activation_preserves_typed_payloads_and_local_toggle_preview() {
        let mut menu = sample_menu();
        let sidebar = menu
            .items()
            .iter()
            .position(|item| item.id() == Some("sidebar".into()))
            .unwrap();
        let PopoverMenuActivation::Command {
            action, close_menu, ..
        } = menu.activate(sidebar).unwrap()
        else {
            panic!("checkbox activates a command")
        };
        assert_eq!(
            action.downcast_ref::<Command>(),
            Some(&Command::Toggle(true))
        );
        assert!(!close_menu);
        assert_eq!(menu.items()[sidebar].checked(), Some(true));

        let dark = menu
            .items()
            .iter()
            .position(|item| item.id() == Some("dark".into()))
            .unwrap();
        menu.activate(dark).unwrap();
        assert_eq!(menu.items()[dark].checked(), Some(true));
        let light = menu
            .items()
            .iter()
            .position(|item| item.id() == Some("light".into()))
            .unwrap();
        assert_eq!(menu.items()[light].checked(), Some(false));
    }

    #[test]
    fn typeahead_is_bounded_expires_without_a_timer_and_cycles_repeated_letters() {
        let mut menu = PopoverMenu::new([
            PopoverMenuItem::action("apple", "Apple", Command::Open),
            PopoverMenuItem::action("apricot", "Apricot", Command::Open),
            PopoverMenuItem::action("banana", "Banana", Command::Open),
        ])
        .unwrap();
        let now = Instant::now();
        assert!(menu.typeahead("a", now));
        assert_eq!(menu.active_item().unwrap().label().as_ref(), "Apricot");
        assert!(menu.typeahead("a", now + Duration::from_millis(20)));
        assert_eq!(menu.active_item().unwrap().label().as_ref(), "Apple");
        assert!(menu.typeahead(
            "b",
            now + Duration::from_millis(20)
                + POPOVER_MENU_TYPEAHEAD_TIMEOUT
                + Duration::from_millis(1)
        ));
        assert_eq!(menu.active_item().unwrap().label().as_ref(), "Banana");
        menu.typeahead(&"z".repeat(MAX_POPOVER_MENU_TYPEAHEAD_BYTES * 2), now);
        assert!(menu.typeahead.len() <= MAX_POPOVER_MENU_TYPEAHEAD_BYTES);
    }

    #[test]
    fn replacement_is_atomic_and_preserves_a_stable_highlight() {
        let mut menu = sample_menu();
        let dark = menu
            .items()
            .iter()
            .position(|item| item.id() == Some("dark".into()))
            .unwrap();
        assert!(menu.highlight(dark));
        assert!(
            !menu
                .set_items([
                    PopoverMenuItem::action("new", "New", Command::Open),
                    PopoverMenuItem::radio("dark", "Dark", "theme", true, Command::Theme("dark")),
                ])
                .unwrap()
        );
        assert_eq!(menu.active_item().unwrap().id(), Some("dark".into()));

        let before = menu.items().len();
        let error = menu.set_items([
            PopoverMenuItem::action("same", "One", Command::Open),
            PopoverMenuItem::action("same", "Two", Command::Open),
        ]);
        assert_eq!(error, Err(PopoverMenuError::DuplicateItemId("same".into())));
        assert_eq!(menu.items().len(), before);
    }

    #[test]
    fn unstyled_parts_add_behavior_without_appearance() {
        let menu = sample_menu();
        let root = menu.root_with("menu", div());
        assert_eq!(root.accessibility.role, AccessibilityRole::Menu);
        assert!(root.focusable);
        assert!(root.auto_focus);
        assert_eq!(root.app_region, Some(AppRegion::NoDrag));
        assert_eq!(root.visual.background, None);
        assert_eq!(root.visual.border_color, None);

        let action = menu.item_with("menu", 1, div()).unwrap();
        assert_eq!(action.accessibility.role, AccessibilityRole::MenuItem);
        assert!(action.clickable);
        assert_eq!(action.visual.background, None);
        assert_eq!(action.visual.border_color, None);

        let checkbox = menu.item_with("menu", 4, div()).unwrap();
        assert_eq!(
            checkbox.accessibility.role,
            AccessibilityRole::MenuItemCheckBox
        );
        assert_eq!(checkbox.accessibility.toggled, Some(false.into()));

        let group_label_id = menu.item_element_id("menu", 0).unwrap();
        let group_label = menu.item_with("menu", 0, div()).unwrap();
        assert_eq!(group_label.explicit_id, Some(group_label_id));
        assert_eq!(group_label.accessibility.role, AccessibilityRole::Label);
        assert_eq!(group_label.accessibility.label.as_deref(), Some("File"));

        let separator = menu.item_with("menu", 3, div()).unwrap();
        assert_eq!(separator.accessibility.role, AccessibilityRole::Separator);
        assert!(!separator.clickable);
        assert_eq!(menu.item_element_id("menu", 3), None);

        let group = menu
            .labeled_group_with("menu", 0, div())
            .expect("the first row is a group label");
        assert_eq!(group.accessibility.role, AccessibilityRole::Group);
        assert_eq!(
            group.accessibility.relations.labelled_by(),
            Some(group_label_id)
        );
        assert_eq!(group.visual.background, None);
        assert!(menu.labeled_group_with("menu", 1, div()).is_none());

        let styled = menu.root_with("styled", div().bg(Color::BLACK));
        assert_eq!(styled.visual.background, Some(Color::BLACK));
    }

    #[test]
    fn nested_validation_counts_the_complete_tree() {
        let mut menu =
            PopoverMenu::new([PopoverMenuItem::action("leaf", "Leaf", Command::Open)]).unwrap();
        for depth in 1..MAX_POPOVER_MENU_DEPTH {
            menu = PopoverMenu::new([PopoverMenuItem::submenu(depth, "More", menu)]).unwrap();
        }
        assert_eq!(
            PopoverMenu::new([PopoverMenuItem::submenu("too-deep", "More", menu)]).unwrap_err(),
            PopoverMenuError::TooDeep
        );
    }

    #[test]
    fn link_items_dispatch_an_open_url_action_and_bound_their_destination() {
        let mut menu = PopoverMenu::new([
            PopoverMenuItem::action("new", "New", Command::Open),
            PopoverMenuItem::link("docs", "Documentation", "https://example.com/docs"),
        ])
        .unwrap();
        let link = &menu.items()[1];
        assert_eq!(link.kind(), PopoverMenuItemKind::Link);
        assert_eq!(
            link.link_url().map(Arc::as_ref),
            Some("https://example.com/docs")
        );
        assert!(link.closes_on_activate());
        assert!(menu.items()[0].link_url().is_none());

        let PopoverMenuActivation::Command {
            action, close_menu, ..
        } = menu.activate(1).unwrap()
        else {
            panic!("a link item activates a command")
        };
        assert!(close_menu);
        assert_eq!(
            action.downcast_ref::<OpenMenuLink>(),
            Some(&OpenMenuLink {
                url: Arc::from("https://example.com/docs")
            })
        );

        let long = "https://example.com/".to_owned() + &"a".repeat(MAX_POPOVER_MENU_LINK_BYTES);
        assert_eq!(
            PopoverMenu::new([PopoverMenuItem::link("long", "Long", long)]).unwrap_err(),
            PopoverMenuError::LinkUrlTooLong
        );

        let part = menu.link_item_with("menu", 1, div()).unwrap();
        assert_eq!(part.accessibility.role, AccessibilityRole::MenuItem);
        assert_eq!(part.visual.background, None);
        assert!(menu.link_item_with("menu", 0, div()).is_none());
    }

    #[test]
    fn radio_groups_retain_exactly_one_checked_value() {
        let mut menu = sample_menu();
        assert_eq!(menu.radio_value("theme"), Some("light".into()));
        assert!(menu.set_radio_value("theme", "dark"));
        assert_eq!(menu.radio_value("theme"), Some("dark".into()));
        assert!(!menu.set_radio_value("theme", "dark"));
        assert_eq!(
            menu.items()
                .iter()
                .filter(|item| item.radio_group() == Some("theme".into())
                    && item.checked() == Some(true))
                .count(),
            1
        );
        assert!(!menu.set_radio_value("theme", "new"));
        assert!(!menu.set_radio_value("other", "dark"));

        let label_id = menu.item_element_id("menu", 0).unwrap();
        let group = menu.labeled_radio_group_with("menu", 0, div()).unwrap();
        assert_eq!(group.accessibility.role, AccessibilityRole::RadioGroup);
        assert_eq!(group.accessibility.relations.labelled_by(), Some(label_id));
        assert_eq!(group.visual.background, None);
        assert!(menu.labeled_radio_group_with("menu", 1, div()).is_none());
    }

    #[test]
    fn horizontal_orientation_installs_the_menubar_key_context() {
        let menu = sample_menu().orientation(MenuOrientation::Horizontal);
        assert_eq!(menu.orientation_value(), MenuOrientation::Horizontal);
        let root = menu.root_with("bar", div());
        assert!(
            root.key_context
                .as_ref()
                .is_some_and(|context| context.contains(POPOVER_MENU_HORIZONTAL_KEY_CONTEXT))
        );
        assert_eq!(
            root.accessibility.orientation,
            Some(AccessibilityOrientation::Horizontal)
        );

        let vertical = sample_menu();
        let vertical_root = vertical.root_with("menu", div());
        assert!(
            vertical_root
                .key_context
                .as_ref()
                .is_some_and(|context| context.contains(POPOVER_MENU_KEY_CONTEXT))
        );
        assert_eq!(
            vertical_root.accessibility.orientation,
            Some(AccessibilityOrientation::Vertical)
        );

        let mut mutable = sample_menu();
        assert!(mutable.set_orientation(MenuOrientation::Horizontal));
        assert!(!mutable.set_orientation(MenuOrientation::Horizontal));

        let bindings = popover_menu_horizontal_key_bindings();
        assert_eq!(bindings.len(), 9);
        assert_eq!(popover_menu_key_bindings().len(), 9);
    }

    #[test]
    fn base_ui_named_item_parts_are_kind_checked_and_indicators_are_hidden() {
        let menu = sample_menu();
        assert!(menu.checkbox_item_with("menu", 4, div()).is_some());
        assert!(menu.checkbox_item_with("menu", 1, div()).is_none());
        assert!(menu.radio_item_with("menu", 5, div()).is_some());
        assert!(menu.radio_item_with("menu", 4, div()).is_none());
        assert!(menu.separator_with("menu", 3, div()).is_some());
        assert!(menu.separator_with("menu", 0, div()).is_none());
        assert!(menu.group_label_with("menu", 0, div()).is_some());
        assert!(menu.group_label_with("menu", 3, div()).is_none());
        assert!(menu.submenu_trigger_with("menu", 1, true, div()).is_none());

        let submenu_menu =
            PopoverMenu::new([PopoverMenuItem::submenu("more", "More", sample_menu())]).unwrap();
        let trigger = submenu_menu
            .submenu_trigger_with("menu", 0, true, div())
            .unwrap();
        assert_eq!(trigger.accessibility.expanded, Some(true));
        assert_eq!(
            trigger.accessibility.has_popover,
            Some(AccessibilityPopover::Menu)
        );

        let indicator = PopoverMenu::checkbox_item_indicator_with(div().bg(Color::BLACK));
        assert!(indicator.accessibility.hidden);
        assert_eq!(indicator.visual.background, Some(Color::BLACK));
        assert!(
            PopoverMenu::radio_item_indicator_with(div())
                .accessibility
                .hidden
        );

        let state = menu.item_render_state(4, false).unwrap();
        assert_eq!(
            state,
            MenuItemPartState {
                highlighted: false,
                disabled: false,
                checked: Some(false),
                open: false,
            }
        );
        assert!(!menu.item_render_state(1, true).unwrap().open);
        assert!(submenu_menu.item_render_state(0, true).unwrap().open);
        assert!(menu.item_render_state(99, false).is_none());
    }

    #[test]
    fn menu_state_declares_base_ui_root_props_without_appearance() {
        let menu = MenuState::new("trigger", "popup")
            .modal(true)
            .orientation(MenuOrientation::Horizontal)
            .loop_focus(false)
            .close_parent_on_esc(true)
            .open_on_hover(true)
            .delay(Duration::from_secs(60))
            .side(AnchorSide::Top)
            .align(AnchorAlign::End)
            .arrow_size(12.0)
            .with_open(true);
        assert!(menu.is_open());
        assert!(menu.is_modal());
        assert!(!menu.loops_focus());
        assert!(menu.closes_parent_on_esc());
        assert!(menu.opens_on_hover());
        assert_eq!(menu.orientation_value(), MenuOrientation::Horizontal);
        assert_eq!(menu.resolved_side(), AnchorSide::Top);
        assert_eq!(menu.resolved_align(), AnchorAlign::End);
        assert_eq!(
            menu.state(),
            MenuPartState {
                open: true,
                side: AnchorSide::Top,
                align: AnchorAlign::End,
                anchor_hidden: false,
            }
        );
        assert!(format!("{menu:?}").contains("MenuState"));

        let positioner = menu.positioner_with(div().bg(Color::BLACK));
        assert!(positioner.reports_anchor_placement());
        assert_eq!(positioner.visual.background, Some(Color::BLACK));
        let arrow = menu.arrow_with(div());
        assert!(arrow.accessibility.hidden);
        let backdrop = menu.backdrop_with(div());
        assert!(backdrop.accessibility.hidden);
        assert_eq!(backdrop.visual.background, None);
        assert_eq!(menu.root_with(div()).app_region, Some(AppRegion::NoDrag));
        assert_eq!(
            menu.submenu_root_with(div()).app_region,
            Some(AppRegion::NoDrag)
        );

        let mut disabled = MenuState::new("trigger", "popup").disabled(true);
        assert!(disabled.is_disabled());
        assert!(!disabled.open_now());
        assert!(!disabled.is_open());

        let mut controlled = MenuState::new("trigger", "popup");
        assert!(controlled.toggle());
        assert!(controlled.is_open());
        assert!(controlled.close_now());
        assert!(!controlled.close_now());
        assert!(!controlled.is_pending());
    }

    struct MenuHost {
        menu: MenuState,
        changes: Vec<bool>,
    }

    impl MenuHost {
        fn new(menu: MenuState) -> Self {
            Self {
                menu,
                changes: Vec::new(),
            }
        }
    }

    impl View for MenuHost {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let trigger = self.menu.trigger_with(
                cx,
                |view| &mut view.menu,
                |view: &mut Self, open, _cx| view.changes.push(open),
                div().w(80.0).h(24.0).child(text("File")),
            );
            let mut root = self
                .menu
                .root_with(div().size_full().flex_col())
                .child(trigger);
            if self.menu.is_open() {
                let popup = self.menu.popup_with(
                    cx,
                    |view| &mut view.menu,
                    |view: &mut Self, open, _cx| view.changes.push(open),
                    div().w(160.0).h(80.0),
                );
                root = root.child(self.menu.positioner_with(div()).child(popup));
            }
            root
        }
    }

    #[test]
    fn menu_trigger_click_toggles_the_controlled_open_flag_and_reports_the_change() {
        let (mut cx, view) =
            TestAppContext::new(MenuHost::new(MenuState::new("menu-trigger", "menu-popup")))
                .unwrap();
        let window = view.window_handle();
        assert!(!cx.read(view, |view| view.menu.is_open()).unwrap());

        cx.click(window, "menu-trigger").unwrap();
        assert!(cx.read(view, |view| view.menu.is_open()).unwrap());
        assert_eq!(
            cx.read(view, |view| view.changes.clone()).unwrap(),
            vec![true]
        );
        assert!(cx.contains_element(window, "menu-popup").unwrap());

        let update = cx.accessibility_update(window).unwrap();
        let trigger = update
            .nodes
            .iter()
            .find_map(|(id, node)| {
                (id.0 == ElementId::from("menu-trigger").as_u64()).then_some(node)
            })
            .expect("the menu trigger has an accessibility node");
        assert_eq!(trigger.is_expanded(), Some(true));

        cx.click(window, "menu-trigger").unwrap();
        assert!(!cx.read(view, |view| view.menu.is_open()).unwrap());
        assert_eq!(
            cx.read(view, |view| view.changes.clone()).unwrap(),
            vec![true, false]
        );
    }

    #[test]
    fn a_pointer_leaving_a_row_drops_its_highlight_but_not_one_the_keyboard_moved() {
        let mut menu = sample_menu();
        let active = |menu: &PopoverMenu| menu.active_item().and_then(PopoverMenuItem::id);
        assert!(menu.highlight(4));
        assert_eq!(active(&menu), Some(ElementId::from("sidebar")));

        // Leaving the lit row leaves nothing highlighted, as a native menu would show.
        assert!(menu.unhighlight(4));
        assert!(active(&menu).is_none());
        assert!(!menu.unhighlight(4));

        // A highlight the keyboard has since moved elsewhere survives the stale row's exit.
        assert!(menu.highlight(4));
        assert!(menu.select_next());
        assert!(!menu.unhighlight(4));
        assert_eq!(active(&menu), Some(ElementId::from("light")));
    }

    #[test]
    fn hover_opening_uses_an_exact_deadline_and_a_settled_menu_sleeps() {
        let menu = MenuState::new("menu-trigger", "menu-popup")
            .open_on_hover(true)
            .delay(Duration::from_millis(100));
        let (mut cx, view) = TestAppContext::new(MenuHost::new(menu)).unwrap();
        let window = view.window_handle();

        cx.visual(window)
            .unwrap()
            .move_pointer(crate::Point::new(10.0, 10.0))
            .unwrap();
        assert!(cx.read(view, |view| view.menu.is_pending()).unwrap());
        assert!(!cx.read(view, |view| view.menu.is_open()).unwrap());

        cx.advance_time(Duration::from_millis(99)).unwrap();
        assert!(!cx.read(view, |view| view.menu.is_open()).unwrap());
        cx.advance_time(Duration::from_millis(1)).unwrap();
        assert!(cx.read(view, |view| view.menu.is_open()).unwrap());
        assert_eq!(
            cx.read(view, |view| view.changes.clone()).unwrap(),
            vec![true]
        );
        assert!(!cx.read(view, |view| view.menu.is_pending()).unwrap());

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn default_close_delay_bridges_the_gap_from_trigger_to_popup() {
        let menu = MenuState::new("menu-trigger", "menu-popup")
            .open_on_hover(true)
            .delay(Duration::ZERO);
        let (mut cx, view) = TestAppContext::new(MenuHost::new(menu)).unwrap();
        let window = view.window_handle();

        cx.visual(window)
            .unwrap()
            .move_pointer(crate::Point::new(10.0, 10.0))
            .unwrap();
        assert!(cx.read(view, |view| view.menu.is_open()).unwrap());

        let popup = cx.element_bounds(window, "menu-popup").unwrap();
        cx.visual(window)
            .unwrap()
            .move_pointer(crate::Point::new(10.0, popup.y - 4.0))
            .unwrap();
        assert!(cx.read(view, |view| view.menu.is_open()).unwrap());
        assert!(cx.read(view, |view| view.menu.is_pending()).unwrap());

        cx.visual(window)
            .unwrap()
            .move_pointer(crate::Point::new(popup.x + 10.0, popup.y + 10.0))
            .unwrap();
        assert!(cx.read(view, |view| view.menu.is_open()).unwrap());
        assert!(!cx.read(view, |view| view.menu.is_pending()).unwrap());
        cx.advance_time(DEFAULT_MENU_CLOSE_DELAY).unwrap();
        assert!(cx.read(view, |view| view.menu.is_open()).unwrap());
    }

    #[derive(Default)]
    struct CommandOwner {
        received: usize,
    }

    impl View for CommandOwner {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let command = cx.action_listener("command-owner", |view, _: &Command, cx| {
                view.received += 1;
                cx.invalidate();
            });
            div()
                .focus_scope(cx.focus_handle("command-owner"))
                .on_action(command)
                .child(div().id("owner-focus").focusable().auto_focus())
        }
    }

    struct IntermediatePopover;

    impl View for IntermediatePopover {
        fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            div().size_full()
        }
    }

    struct MenuPopover {
        menu: PopoverMenu,
    }

    impl MenuPopover {
        fn new() -> Self {
            Self {
                menu: PopoverMenu::new([PopoverMenuItem::action("open", "Open", Command::Open)])
                    .unwrap(),
            }
        }
    }

    impl View for MenuPopover {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            self.menu.element(
                cx,
                "menu-popover",
                |view| &mut view.menu,
                div().size_full().flex_col(),
                |item, state| {
                    div()
                        .h(32.0)
                        .w_full()
                        .child(text(item.label().clone()))
                        .opacity(if state.highlighted { 1.0 } else { 0.9 })
                },
                |_view, cx| {
                    assert!(cx.close_popover_chain());
                },
            )
        }
    }

    #[test]
    fn nested_popover_menu_commands_reach_the_non_popover_owner_before_close() {
        let (mut cx, owner) = TestAppContext::new(CommandOwner::default()).unwrap();
        let first = cx
            .update(owner, |_view, cx| {
                cx.open_window(
                    WindowOptions::new("First popover")
                        .size(160.0, 100.0)
                        .system_popover(PopoverOptions::new(Rect::new(10.0, 10.0, 20.0, 20.0))),
                    IntermediatePopover,
                )
            })
            .unwrap();
        let first = cx.typed_window::<IntermediatePopover>(first).unwrap();
        let second = cx
            .update(first, |_view, cx| {
                cx.open_window(
                    WindowOptions::new("Nested menu")
                        .size(160.0, 100.0)
                        .system_popover(PopoverOptions::new(Rect::new(20.0, 20.0, 20.0, 20.0))),
                    MenuPopover::new(),
                )
            })
            .unwrap();
        let item_id = MenuPopover::new()
            .menu
            .item_element_id("menu-popover", 0)
            .unwrap();

        cx.click(second, item_id).unwrap();

        assert!(!cx.is_window_open(second));
        assert_eq!(cx.read(owner, |view| view.received).unwrap(), 1);
        assert!(!cx.is_window_open(first.window_handle()));
    }
}
