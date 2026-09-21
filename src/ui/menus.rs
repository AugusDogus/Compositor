//! Linux application menus. QuickGUI owns focus, typeahead and popup dismissal;
//! these declarations map the desktop commands to the existing editor actions.
use super::*;
use quickgui::{
    Menubar, MenubarState, Popover, PopoverKind, PopoverMenu, PopoverMenuItem, PopoverMenuItemKind,
};

const TITLES: [&str; 8] = [
    "File", "Edit", "View", "Select", "Image", "Filter", "Layer", "Help",
];
#[derive(Clone, Copy, PartialEq)]
struct Horizontal(bool);
pub(crate) fn key_bindings() -> [quickgui::KeyBinding; 2] {
    [
        quickgui::KeyBinding::new(
            "left",
            Horizontal(false),
            Some(quickgui::POPOVER_MENU_KEY_CONTEXT),
        ),
        quickgui::KeyBinding::new(
            "right",
            Horizontal(true),
            Some(quickgui::POPOVER_MENU_KEY_CONTEXT),
        ),
    ]
}
#[derive(Clone, Copy, PartialEq)]
struct Invoke {
    menu: usize,
    item: usize,
}
#[derive(Clone, Copy)]
enum Command {
    Edit(Action),
    About,
    Updates,
    Quit,
    Handles,
    Visibility,
    ResizeSelection { expand: bool },
}
enum Entry {
    Item(&'static str, &'static str, Command),
    Separator,
    Submenu(&'static str, usize),
}
mod context;
mod entries;
mod popup;
mod row;
pub(super) mod style;
use entries::entries;

#[derive(Clone, Copy)]
enum MenuInput {
    Pointer,
    Keyboard,
}
impl MenuInput {
    fn prepare(self, menu: &mut PopoverMenu) {
        match self {
            Self::Keyboard => {
                menu.select_first();
            }
            Self::Pointer => {
                if let Some(index) = menu.active_index() {
                    menu.unhighlight(index);
                }
            }
        }
    }
}

pub(super) struct Menus {
    bar: MenubarState,
    popup: PopoverMenu,
    loaded: Option<usize>,
    input: MenuInput,
    auxiliary: Option<usize>,
    row: Option<row::RowContext>,
    submenu: PopoverMenu,
    submenu_anchor: Option<quickgui::ElementId>,
}
impl Menus {
    pub(super) fn is_open(&self) -> bool {
        self.bar.open_menu().is_some() || self.auxiliary.is_some()
    }

    pub fn new() -> Result<Self> {
        Ok(Self {
            bar: MenubarState::new(TITLES.len()),
            popup: PopoverMenu::new([]).map_err(|e| compositor::invalid(e.to_string()))?,
            loaded: None,
            input: MenuInput::Pointer,
            auxiliary: None,
            row: None,
            submenu: PopoverMenu::new([]).map_err(|e| compositor::invalid(e.to_string()))?,
            submenu_anchor: None,
        })
    }
    #[cfg(test)]
    pub(super) fn command_id(&self, label: &str) -> quickgui::ElementId {
        let index = self
            .popup
            .items()
            .iter()
            .position(|item| item.label().as_ref() == label)
            .unwrap();
        self.popup
            .item_element_id("application-menu-items", index)
            .unwrap()
    }
    fn move_bar(&mut self, forward: bool, cx: &mut EventContext) {
        self.bar.move_focus(forward);
        let index = self.bar.focused_menu();
        if self.bar.open_menu().is_some() {
            self.activate(index, cx);
        } else {
            cx.focus(quickgui::FocusHandle::new(
                Menubar::new("application-menu").item_id(index),
            ));
            cx.invalidate();
        }
    }
    pub(super) fn activate(&mut self, index: usize, cx: &mut EventContext) {
        self.close();
        self.input = MenuInput::Keyboard;
        self.bar.open_menu_at(index);
        cx.focus(quickgui::FocusHandle::new("application-menu-items"));
        cx.invalidate();
    }
    pub fn close(&mut self) {
        self.bar.close();
        self.auxiliary = None;
        self.row = None;
        self.loaded = None;
        self.input = MenuInput::Pointer;
        self.submenu_anchor = None;
    }
    fn open_submenu(
        &mut self,
        anchor: quickgui::ElementId,
        mut menu: PopoverMenu,
        input: MenuInput,
        cx: &mut EventContext,
    ) {
        input.prepare(&mut menu);
        self.submenu = menu;
        self.submenu_anchor = Some(anchor);
        cx.focus(quickgui::FocusHandle::new("application-submenu-items"));
        cx.invalidate();
    }
    fn close_submenu(&mut self, cx: &mut EventContext) {
        self.submenu_anchor = None;
        cx.focus(quickgui::FocusHandle::new("application-menu-items"));
        cx.invalidate();
    }
}
impl Editor {
    fn sync_menu(&mut self) {
        let Some(index) = self.menus.bar.open_menu().or(self.menus.auxiliary) else {
            return;
        };
        let fresh = self.build_menu(index);
        if self.menus.loaded != Some(index) {
            self.menus.popup = fresh;
            self.menus.input.prepare(&mut self.menus.popup);
            self.menus.loaded = Some(index);
        } else {
            refresh_menu(&mut self.menus.popup, &fresh);
        }
        if let Some(anchor) = self.menus.submenu_anchor {
            let child = self
                .menus
                .popup
                .items()
                .iter()
                .enumerate()
                .find_map(|(index, item)| {
                    (self
                        .menus
                        .popup
                        .item_element_id("application-menu-items", index)
                        == Some(anchor))
                    .then(|| item.submenu_menu())
                    .flatten()
                });
            if let Some(child) = child {
                refresh_menu(&mut self.menus.submenu, child);
            }
        }
    }
    pub(super) fn layer_menu_button(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let (id, index, icon, label) = (
            "layer-adjustment-menu",
            9,
            Icon::Adjustment,
            "New adjustment layer",
        );
        icon.button(label)
            .gap(2.)
            .child(Icon::ChevronDown.element(8.))
            .disabled(!self.can_edit_layers())
            .id(id)
            .on_click(cx.listener(id, move |this, cx| {
                let was_open = this.menus.auxiliary == Some(index);
                this.menus.close();
                if !was_open {
                    this.menus.auxiliary = Some(index);
                    cx.focus(quickgui::FocusHandle::new("application-menu-items"));
                } else {
                    cx.focus(quickgui::FocusHandle::new("workspace"));
                }
                cx.invalidate();
            }))
    }
    pub(super) fn menu_bar(&mut self, cx: &mut ViewContext<'_, Self>) -> Element {
        self.sync_menu();
        let bar = Menubar::new("application-menu");
        let mut root = bar
            .root(self.menus.bar)
            .h(28.)
            .flex_shrink_0()
            .flex_row()
            .items_center()
            .px(8.)
            .bg(Color::rgb8(36, 36, 36));
        for (index, title) in TITLES.into_iter().enumerate() {
            if let Some(item) = bar.item(self.menus.bar, index) {
                let element = item.item_with(
                    div()
                        .h(24.)
                        .px(10.)
                        .items_center()
                        .rounded(12.)
                        .bg(if item.is_open() {
                            Color::rgb8(76, 76, 76)
                        } else {
                            Color::TRANSPARENT
                        })
                        .hover(|s| s.bg(Color::rgb8(62, 62, 62)))
                        .child(text(title).text_size(13.).font_semibold()),
                );
                let id = item.item_id();
                let element = element
                    .on_click(cx.listener(id, move |this, cx| {
                        if this.menus.bar.is_open(index) {
                            this.menus.close();
                            cx.focus(quickgui::FocusHandle::new("workspace"));
                            cx.invalidate();
                        } else {
                            this.menus.activate(index, cx);
                            this.menus.input = MenuInput::Pointer;
                        }
                    }))
                    .on_hover(cx.hover_listener(id, move |this, hovered, cx| {
                        if *hovered
                            && this.menus.bar.open_menu().is_some()
                            && !this.menus.bar.is_open(index)
                        {
                            this.menus.activate(index, cx);
                            this.menus.input = MenuInput::Pointer;
                        }
                    }))
                    .on_action(cx.action_listener(
                        id,
                        move |this, _: &quickgui::MenubarOpen, cx| {
                            this.menus.activate(index, cx);
                        },
                    ));
                let element = element
                    .on_action(
                        cx.action_listener(id, |this, _: &quickgui::MenubarPrevious, cx| {
                            this.menus.move_bar(false, cx);
                        }),
                    )
                    .on_action(
                        cx.action_listener(id, |this, _: &quickgui::MenubarNext, cx| {
                            this.menus.move_bar(true, cx);
                        }),
                    )
                    .on_action(
                        cx.action_listener(id, |this, _: &quickgui::MenubarFirst, cx| {
                            this.menus.activate(0, cx);
                        }),
                    )
                    .on_action(
                        cx.action_listener(id, |this, _: &quickgui::MenubarLast, cx| {
                            this.menus.activate(TITLES.len() - 1, cx);
                        }),
                    )
                    .on_action(
                        cx.action_listener(id, |this, _: &quickgui::MenubarClose, cx| {
                            this.menus.close();
                            cx.focus(quickgui::FocusHandle::new("workspace"));
                            cx.invalidate();
                        }),
                    );
                root = root.child(element);
            }
        }
        root
    }
}

// Actions and shortcuts are fixed by `entries`; only presentation depends on the
// document. Avoid resetting typeahead on ordinary pointer/animation redraws.
fn same_menu_content(left: &PopoverMenu, right: &PopoverMenu) -> bool {
    left.items().len() == right.items().len()
        && left.items().iter().zip(right.items()).all(|(a, b)| {
            a.kind() == b.kind()
                && a.label() == b.label()
                && a.is_disabled() == b.is_disabled()
                && a.checked() == b.checked()
                && match (a.submenu_menu(), b.submenu_menu()) {
                    (Some(a), Some(b)) => same_menu_content(a, b),
                    (None, None) => true,
                    _ => false,
                }
        })
}

fn refresh_menu(current: &mut PopoverMenu, fresh: &PopoverMenu) {
    if !same_menu_content(current, fresh) {
        let was_unhighlighted = current.active_index().is_none();
        current
            .set_items(fresh.items().iter().cloned())
            .expect("Application menu declarations must be valid");
        if was_unhighlighted {
            MenuInput::Pointer.prepare(current);
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "menus/history_tests.rs"]
pub(super) mod history_tests;

#[cfg(test)]
#[path = "menus/paint_history_tests.rs"]
mod paint_history_tests;

#[cfg(test)]
#[path = "menus/layer_operation_history_tests.rs"]
mod layer_operation_history_tests;
