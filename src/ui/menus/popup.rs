use super::*;
impl Editor {
    pub(in crate::ui) fn menu_popup(&mut self, cx: &mut ViewContext<'_, Self>) -> Element {
        self.sync_menu();
        let Some(index) = self.menus.bar.open_menu().or(self.menus.auxiliary) else {
            return div();
        };
        let bar = Menubar::new("application-menu");
        let trigger = if index < TITLES.len() {
            bar.item_id(index)
        } else {
            quickgui::ElementId::from(if index == 8 {
                "layer-context-anchor"
            } else {
                "layer-adjustment-menu"
            })
        };
        let popup = Popover::new(trigger, "application-menu-popup", true)
            .kind(PopoverKind::Menu)
            .side(if index < TITLES.len() || index == 8 {
                quickgui::AnchorSide::Bottom
            } else {
                quickgui::AnchorSide::Top
            })
            .align(quickgui::AnchorAlign::Start)
            .side_offset(2.)
            .initial_focus("application-menu-items");
        let action = cx.action_listener("application-menu-popup", |this, command: &Invoke, cx| {
            let is_row = command.menu == 8;
            let entries = entries(command.menu);
            let Some(Entry::Item(_, _, command)) = entries.get(command.item) else {
                return;
            };
            let command = *command;
            if is_row {
                this.invoke_row_menu(command, cx);
                return;
            }
            if !this.menu_available(command) {
                cx.invalidate();
                return;
            }
            this.menus.close();
            cx.focus(quickgui::FocusHandle::new("workspace"));
            match command {
                Command::Edit(action) => this.action(action, cx),
                Command::About => this.open_about(cx),
                Command::Updates => this.open_updates(cx),
                Command::Quit => this.request_close(CloseIntent::Window, cx),
                Command::ResizeSelection { expand } => {
                    let amount = if expand {
                        this.tools.selection_expand_amount
                    } else {
                        this.tools.selection_contract_amount
                    };
                    let result = this.resize_selection(expand, amount);
                    this.operation_result(alerts::Operation::Paint, result, cx);
                }
                Command::Visibility => {
                    let result = this.finish_pending_edits().and_then(|()| {
                        let target = this.session().document.active.ok_or_else(|| {
                            compositor::invalid("Select a layer to change its visibility.")
                        })?;
                        this.toggle_layer_visibility(target)
                    });
                    this.result(result, cx);
                }
                Command::Handles => {
                    this.tools.show_transform_controls = !this.tools.show_transform_controls;
                    cx.invalidate();
                }
            }
        });
        let dismiss = cx.dismiss_listener("application-menu-popup", |this, cx| {
            this.menus.close();
            cx.focus(quickgui::FocusHandle::new("workspace"));
            cx.invalidate();
        });
        let maximum_height = cx.size().height - 60.;
        let content = self.menus.popup.element_with_submenus_and_hover(
            cx,
            "application-menu-items",
            |e| &mut e.menus.popup,
            div()
                .auto_focus()
                .min_w(225.)
                .max_h(maximum_height)
                .overflow_y_scroll()
                .flex_col()
                .p(5.)
                .gap(1.),
            menu_row,
            |this, cx| {
                this.menus.close();
                cx.focus(quickgui::FocusHandle::new("workspace"));
                cx.invalidate();
            },
            |this, anchor, menu, cx| {
                this.menus
                    .open_submenu(anchor, menu, MenuInput::Keyboard, cx)
            },
            |this, index, hovered, cx| {
                if !hovered {
                    // Keep a submenu's parent lit while the pointer crosses into its child.
                    let anchor = this
                        .menus
                        .popup
                        .item_element_id("application-menu-items", index);
                    if anchor != this.menus.submenu_anchor && this.menus.popup.unhighlight(index) {
                        cx.invalidate();
                    }
                    return;
                }
                this.menus.popup.highlight(index);
                let child = this
                    .menus
                    .popup
                    .items()
                    .get(index)
                    .and_then(|item| item.submenu_menu())
                    .cloned();
                if let Some(child) = child {
                    if let Some(anchor) = this
                        .menus
                        .popup
                        .item_element_id("application-menu-items", index)
                        && this.menus.submenu_anchor != Some(anchor)
                    {
                        this.menus
                            .open_submenu(anchor, child, MenuInput::Pointer, cx);
                    }
                } else if this.menus.submenu_anchor.is_some() {
                    this.menus.close_submenu(cx);
                }
                cx.invalidate();
            },
        );
        let submenu = self.submenu_popup(cx);
        let surface = popup.surface_with(
            // Keep the submenu outside the parent's material group: it must sample the
            // canvas behind it, while retaining popover ancestry for dismissal and commands.
            div()
                .child(style::surface().child(content))
                .child(submenu)
                .on_action(action)
                .on_action(cx.action_listener(
                    "application-menu-popup",
                    |this, direction: &Horizontal, cx| {
                        if direction.0 {
                            let index = this.menus.popup.active_index();
                            if let Some(index) = index
                                && let Some(child) = this
                                    .menus
                                    .popup
                                    .items()
                                    .get(index)
                                    .and_then(|item| item.submenu_menu())
                                    .cloned()
                                && let Some(anchor) = this
                                    .menus
                                    .popup
                                    .item_element_id("application-menu-items", index)
                            {
                                this.menus
                                    .open_submenu(anchor, child, MenuInput::Keyboard, cx);
                                return;
                            }
                        }
                        if let Some(index) = this.menus.bar.open_menu() {
                            let next = (index + if direction.0 { 1 } else { TITLES.len() - 1 })
                                % TITLES.len();
                            this.menus.activate(next, cx);
                        } else if !direction.0 {
                            this.menus.close();
                            cx.focus(quickgui::FocusHandle::new("workspace"));
                            cx.invalidate();
                        }
                    },
                ))
                .on_dismiss(dismiss),
        );
        if let Some(row) = &self.menus.row {
            div()
                .absolute()
                .inset_0()
                .child(
                    div()
                        .id("layer-context-anchor")
                        .absolute()
                        .left(row.position.x)
                        .top(row.position.y)
                        .size(1., 1.),
                )
                .child(surface)
        } else {
            surface
        }
    }
    fn submenu_popup(&mut self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(anchor) = self.menus.submenu_anchor else {
            return div();
        };
        let menu = self.menus.submenu.element(
            cx,
            "application-submenu-items",
            |this| &mut this.menus.submenu,
            div().min_w(225.).flex_col().p(5.).gap(1.),
            menu_row,
            |this, cx| this.menus.close_submenu(cx),
        );
        Popover::new(anchor, "application-submenu-popup", true)
            .kind(PopoverKind::Menu)
            .side(quickgui::AnchorSide::Right)
            .align(quickgui::AnchorAlign::Start)
            .side_offset(0.)
            .initial_focus("application-submenu-items")
            .surface_with(
                style::surface()
                    .child(menu)
                    .on_action(cx.action_listener(
                        "application-submenu-popup",
                        |this, direction: &Horizontal, cx| {
                            if !direction.0 {
                                this.menus.close_submenu(cx);
                            }
                        },
                    ))
                    .on_dismiss(
                        cx.dismiss_listener("application-submenu-popup", |this, cx| {
                            this.menus.close_submenu(cx)
                        }),
                    ),
            )
            .restore_focus_to(quickgui::FocusHandle::new("application-menu-items"))
    }
}

fn menu_row(item: &PopoverMenuItem, state: quickgui::PopoverMenuItemState) -> Element {
    if item.kind() == PopoverMenuItemKind::Separator {
        return div().h(1.).my(4.).mx(6.).bg(Color::rgb8(80, 80, 80));
    }
    let trailing = if item.kind() == PopoverMenuItemKind::Submenu {
        Icon::ChevronRight.element(12.)
    } else {
        text(item.shortcut_text().cloned().unwrap_or_default())
            .text_size(12.)
            .font_normal()
            .flex_shrink_0()
            .whitespace_nowrap()
            .text_color(if state.disabled {
                Color::rgb8(117, 117, 117)
            } else if state.highlighted {
                Color::rgb8(231, 231, 231)
            } else {
                Color::rgb8(166, 166, 166)
            })
    };
    style::choice(item.label().clone(), state)
        .font_semibold()
        .child(trailing)
}
