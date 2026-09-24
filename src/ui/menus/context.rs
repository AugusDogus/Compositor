use super::*;
use compositor::document::LayerContent;

impl Editor {
    pub(in crate::ui) fn build_menu(&self, index: usize) -> PopoverMenu {
        let items = entries(index).into_iter().enumerate().map(|(item, entry)| {
            let id = format!("menu-command-{index}-{item}");
            match entry {
                Entry::Separator => PopoverMenuItem::separator(),
                Entry::Submenu(label, child) => {
                    PopoverMenuItem::submenu(id, label, self.build_menu(child))
                        .disabled(!self.can_edit_layers())
                }
                Entry::Item(label, shortcut, command) => {
                    let invoke = Invoke { menu: index, item };
                    let label = if index == 8 {
                        label.into()
                    } else {
                        self.menu_label(label, command)
                    };
                    let checked = match command {
                        Command::Handles => Some(self.tools.show_transform_controls),
                        Command::Edit(Action::PixelGrid) => Some(self.tools.pixel_grid),
                        Command::Edit(action) => self.layout_checked(action),
                        _ => None,
                    };
                    let item = match checked {
                        Some(value) => PopoverMenuItem::checkbox(id, label, value, invoke),
                        None => PopoverMenuItem::action(id, label, invoke),
                    };
                    item.close_on_activate(false)
                        .shortcut(self.keymap.menu_label(shortcut))
                        .disabled(!if index == 8 {
                            self.row_menu_available(command)
                        } else {
                            self.menu_available(command)
                        })
                }
            }
        });
        PopoverMenu::new(items).expect("Application menu declarations must be valid")
    }

    fn menu_label(&self, default: &str, command: Command) -> String {
        if let Command::Edit(action @ (Action::Undo | Action::Redo)) = command {
            let label = match action {
                Action::Undo => self.tabs[self.current].undo_label(),
                _ => self.tabs[self.current].redo_label(),
            };
            return label
                .filter(|_| self.can_use_history())
                .map(|label| format!("{default} {label}"))
                .unwrap_or_else(|| default.into());
        }
        if !self.has_document() {
            return default.into();
        }
        let doc = &self.session().document;
        let layer = doc.active_layer();
        match command {
            Command::ResizeSelection { expand } => format!(
                "{} by {} px",
                if expand { "Expand" } else { "Contract" },
                if expand {
                    self.tools.selection_expand_amount
                } else {
                    self.tools.selection_contract_amount
                }
            ),
            Command::Edit(Action::Transform) if self.can_float_selection() => {
                "Transform Selection".into()
            }
            Command::Edit(Action::Duplicate) if doc.selection.is_some() => "Layer via Copy".into(),
            Command::Edit(Action::Clip) if layer.is_some_and(|l| l.clip_source.is_some()) => {
                "Release Clipping Mask".into()
            }
            Command::Visibility if layer.is_some_and(|l| !l.visible) => "Show Layer".into(),
            Command::Edit(Action::Merge) if doc.selected.len() > 1 => "Merge Layers".into(),
            Command::Edit(Action::Merge) if layer.is_some_and(|l| l.is_group()) => {
                "Merge Group".into()
            }
            Command::Edit(Action::DeleteLayer)
                if self.tools.mask_target
                    && doc.selected.len() <= 1
                    && layer.is_some_and(|l| l.mask.is_some()) =>
            {
                "Delete Layer Mask".into()
            }
            Command::Edit(Action::DeleteLayer) if doc.selected.len() > 1 => "Delete Layers".into(),
            Command::Edit(Action::InvertPixels) if self.tools.mask_target => "Invert Mask".into(),
            _ => default.into(),
        }
    }

    pub(super) fn menu_available(&self, command: Command) -> bool {
        if self.develop.is_some()
            || !self.errors.is_empty()
            || self.pending
            || self.gesture.is_some()
        {
            return false;
        }
        if matches!(
            command,
            Command::Quit
                | Command::Edit(
                    Action::New
                        | Action::Open
                        | Action::OpenRaw
                        | Action::OpenPsd
                        | Action::OpenClipboard
                        | Action::CloseTab
                )
        ) {
            return self.can_switch_projects();
        }
        if let Command::Edit(action) = command
            && action.is_project_operation()
            && !self.can_start_project_operation()
        {
            return false;
        }
        if matches!(
            command,
            Command::About
                | Command::Shortcuts
                | Command::Updates
                | Command::Quit
                | Command::Edit(
                    Action::New
                        | Action::Open
                        | Action::OpenRaw
                        | Action::OpenPsd
                        | Action::OpenClipboard
                        | Action::Import
                        | Action::Paste
                )
        ) {
            return true;
        }
        match command {
            Command::Edit(Action::Undo) => return self.can_undo(),
            Command::Edit(Action::Redo) => return self.can_redo(),
            _ => {}
        }
        if !self.has_document() {
            return false;
        }
        if (matches!(command, Command::Edit(action) if action.requires_layer_edit())
            || matches!(command, Command::Visibility))
            && !self.can_edit_layers()
        {
            return false;
        }
        if matches!(command, Command::Edit(action) if action.edits_selection() && !matches!(action, Action::SelectAll))
            && !self.can_edit_layers()
        {
            return false;
        }
        let doc = &self.session().document;
        let layer = doc.active_layer();
        match command {
            Command::Edit(Action::DevelopRaw | Action::RasterizeRaw) => {
                self.can_edit_layers() && layer.is_some_and(|layer| layer.raw.is_some())
            }
            Command::Edit(Action::InvertPixels) => self.can_invert(),
            Command::Edit(Action::Fill | Action::FillBackground) => self.can_edit_pixels(),
            Command::Edit(Action::Clear) => self.can_edit_pixels() && doc.selection.is_some(),
            Command::Edit(Action::CopyMerged) => self.can_copy_merged(),
            Command::Edit(Action::Duplicate) => self.can_duplicate_layer(),
            Command::Handles => self.tools.tool == Tool::Move,
            Command::Edit(Action::LoadAlpha) => layer.is_some_and(|l| l.raster().is_some()),
            Command::ResizeSelection { .. } => self.can_modify_selection(),
            Command::Edit(Action::FeatherSelection) => self.can_modify_selection(),
            Command::Edit(Action::Clip) => {
                layer.is_some_and(|active| compositor::clipping::change(doc, active.id).is_some())
            }
            Command::Edit(Action::EditAdjustment) => {
                layer.is_some_and(|l| matches!(l.content, LayerContent::Adjustment(_)))
            }
            Command::Edit(Action::MoveOutOfGroup) => layer.is_some_and(|l| l.parent.is_some()),
            Command::Edit(Action::LoadMask | Action::DeleteMask | Action::ToggleMask) => {
                layer.is_some_and(|l| l.mask.is_some())
            }
            Command::Edit(Action::Deselect | Action::InvertSelection) => doc.selection.is_some(),
            Command::Edit(Action::Rename | Action::DeleteLayer) | Command::Visibility => {
                layer.is_some()
            }
            Command::Edit(Action::Transform | Action::FlipX | Action::FlipY) => {
                compositor::transform::selection_bounds(doc, self.tools.mask_target).is_some()
            }
            Command::Edit(Action::Filter(compositor::filters::Filter::Vignette(_))) => {
                !self.tools.mask_target && layer.is_some_and(|l| l.raw.is_none() && matches!(l.content, LayerContent::Raster(_)))
            }
            Command::Edit(Action::Filter(compositor::filters::Filter::ContentFill)) => {
                self.can_content_aware_fill()
            }
            Command::Edit(
                Action::CameraRaw | Action::AdjustPixels(_) | Action::Filter(_) | Action::RemoveBackground,
            ) => self.can_adjust_colors(),
            Command::Edit(Action::Raise | Action::Lower) => layer.is_some_and(|active| {
                let siblings: Vec<_> = doc
                    .layers
                    .iter()
                    .filter(|l| l.parent == active.parent)
                    .collect();
                siblings
                    .iter()
                    .position(|l| l.id == active.id)
                    .is_some_and(|i| {
                        if matches!(command, Command::Edit(Action::Raise)) {
                            i + 1 < siblings.len()
                        } else {
                            i > 0
                        }
                    })
            }),
            Command::Edit(Action::Merge) => layer.is_some_and(|active| {
                if doc.selected.len() > 1 || active.is_group() {
                    let ids = compositor::transform::selected_ids(doc);
                    doc.layers
                        .iter()
                        .any(|l| ids.contains(&l.id) && !l.is_group())
                } else {
                    doc.layers
                        .iter()
                        .position(|l| l.id == active.id)
                        .is_some_and(|i| {
                            doc.layers[..i]
                                .iter()
                                .rev()
                                .find(|l| l.parent == active.parent)
                                .is_some_and(|l| !l.is_group())
                        })
                }
            }),
            _ => true,
        }
    }
}

#[cfg(test)]
mod pending_tests {
    use super::*;

    #[test]
    fn edit_menu_disables_pixel_mutations_but_keeps_native_clipboard_commands_available() {
        let mut e = Editor::with_test_document();
        assert!(e.menu_available(Command::Edit(Action::Fill)));
        assert!(!e.menu_available(Command::Edit(Action::Clear)));
        assert!(!e.menu_available(Command::Edit(Action::CopyMerged)));
        compositor::edits::fill(
            &mut e.session_mut().document,
            [50, 120, 200, 255],
            false,
            false,
        )
        .unwrap();
        assert!(e.menu_available(Command::Edit(Action::CopyMerged)));
        e.session_mut().document.selection = Some(compositor::selection::Selection::rectangle(
            20,
            20,
            [2., 2.],
            [10., 10.],
            false,
        ));
        assert!(e.menu_available(Command::Edit(Action::Clear)));
        e.begin_gradient([0., 0.]).unwrap();
        for action in [
            Action::Fill,
            Action::FillBackground,
            Action::Clear,
            Action::CopyMerged,
        ] {
            assert!(!e.menu_available(Command::Edit(action)));
        }
        // Swift keeps these enabled for native text editing and checks pixel eligibility on dispatch.
        for action in [Action::Cut, Action::Copy, Action::Paste] {
            assert!(e.menu_available(Command::Edit(action)));
        }
        e.undo_document();
        e.session_mut().document.layers[0].visible = false;
        assert!(!e.menu_available(Command::Edit(Action::Fill)));
        assert!(!e.menu_available(Command::Edit(Action::CopyMerged)));
        assert!(e.menu_available(Command::Edit(Action::Copy)));
    }

    #[test]
    fn image_and_filter_menus_follow_pixel_targets_and_drafts() {
        let mut e = Editor::with_test_document();
        compositor::edits::fill(
            &mut e.session_mut().document,
            [50, 120, 200, 255],
            false,
            false,
        )
        .unwrap();
        let commands = [
            Command::Edit(Action::AdjustPixels(Kind::Levels)),
            Command::Edit(Action::Filter(compositor::filters::Filter::Gaussian {
                radius: 1.,
            })),
            Command::Edit(Action::RemoveBackground),
        ];
        for command in commands {
            assert!(e.menu_available(command));
        }
        e.session_mut().document.layers[0].visible = false;
        for command in commands {
            assert!(!e.menu_available(command));
        }
        e.session_mut().document.layers[0].visible = true;
        e.tools.mask_target = true;
        for command in commands {
            assert!(!e.menu_available(command));
        }
        e.tools.mask_target = false;
        e.start_toolbar_transform().unwrap();
        for command in commands {
            assert!(e.menu_available(command));
        }
        e.finish_toolbar_transform(false).unwrap();
        e.begin_gradient([0., 0.]).unwrap();
        for command in commands {
            assert!(e.menu_available(command));
        }
        assert!(!e.menu_available(Command::Edit(Action::Filter(
            compositor::filters::Filter::ContentFill
        ))));
        e.undo_document();
        e.session_mut().document.selection = Some(compositor::selection::Selection::rectangle(
            20,
            20,
            [2., 2.],
            [10., 10.],
            false,
        ));
        assert!(e.menu_available(Command::Edit(Action::Filter(
            compositor::filters::Filter::ContentFill
        ))));
        e.open_pixel_adjustment(Kind::Levels).unwrap();
        for command in commands {
            assert!(!e.menu_available(command));
        }
    }

    #[test]
    fn selection_and_handle_menu_availability_follows_the_current_edit_and_tool() {
        let mut editor = Editor::with_test_document();
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [50, 120, 200, 255],
            false,
            false,
        )
        .unwrap();
        compositor::edits::add_mask(&mut editor.session_mut().document, true).unwrap();
        editor.session_mut().document.selection = Some(
            compositor::selection::Selection::rectangle(20, 20, [2., 2.], [10., 10.], false),
        );
        let selection_commands = [
            Command::Edit(Action::Deselect),
            Command::Edit(Action::InvertSelection),
            Command::Edit(Action::LoadAlpha),
            Command::Edit(Action::LoadMask),
            Command::ResizeSelection { expand: true },
        ];
        for command in selection_commands {
            assert!(editor.menu_available(command));
        }
        editor.begin_gradient([0., 0.]).unwrap();
        for command in selection_commands {
            assert!(!editor.menu_available(command));
        }
        assert!(editor.menu_available(Command::Edit(Action::SelectAll)));
        editor.undo_document();
        for command in selection_commands {
            assert!(editor.menu_available(command));
        }
        editor.tools.tool = Tool::Move;
        assert!(editor.menu_available(Command::Handles));
        editor.tools.tool = Tool::Rectangle;
        assert!(!editor.menu_available(Command::Handles));
        compositor::layer_ops::group(&mut editor.session_mut().document).unwrap();
        assert!(!editor.menu_available(Command::Edit(Action::LoadAlpha)));
    }

    #[test]
    fn history_menu_follows_pending_gradients_and_persistent_transforms() {
        let mut editor = Editor::with_test_document();
        editor
            .session_mut()
            .edit("Fill", |doc| {
                compositor::edits::fill(doc, [50, 120, 200, 255], false, false)
            })
            .unwrap();
        editor.start_toolbar_transform().unwrap();
        let menu = editor.build_menu(1);
        assert_eq!(menu.items()[0].label().as_ref(), "Undo");
        assert!(menu.items()[0].is_disabled());
        assert!(menu.items()[1].is_disabled());
        editor.finish_toolbar_transform(false).unwrap();
        editor.session_mut().undo();
        editor.begin_gradient([0., 0.]).unwrap();
        let menu = editor.build_menu(1);
        assert!(!menu.items()[0].is_disabled());
        assert_eq!(menu.items()[1].label().as_ref(), "Redo Fill");
        assert!(!menu.items()[1].is_disabled());
    }

    #[test]
    fn pending_transform_disables_layer_commands_and_adjustment_submenu() {
        let mut editor = Editor::with_test_document();
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [50, 120, 200, 255],
            false,
            false,
        )
        .unwrap();
        editor.start_toolbar_transform().unwrap();
        let menu = editor.build_menu(6);
        for label in [
            "New Adjustment Layer",
            "New Blank Layer",
            "Delete Layer",
            "Hide Layer",
            "Transform Layer",
        ] {
            let item = menu
                .items()
                .iter()
                .find(|item| item.label().as_ref() == label)
                .unwrap();
            assert!(item.is_disabled(), "{label}");
        }
        assert!(
            editor
                .build_menu(9)
                .items()
                .iter()
                .all(|item| item.is_disabled())
        );
        editor.finish_toolbar_transform(false).unwrap();
        let menu = editor.build_menu(6);
        assert!(
            !menu
                .items()
                .iter()
                .find(|item| item.label().as_ref() == "New Blank Layer")
                .unwrap()
                .is_disabled()
        );
    }
}
