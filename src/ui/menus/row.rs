//! Row commands capture the clicked layer without changing selection on dismissal.
use super::*;
use uuid::Uuid;

#[derive(Clone, Copy)]
pub(super) struct RowContext {
    session: Uuid,
    layer: Uuid,
    pub position: quickgui::Point,
}

impl Editor {
    pub(in crate::ui) fn open_layer_context(
        &mut self,
        layer: Uuid,
        position: quickgui::Point,
        cx: &mut EventContext,
    ) {
        if self.pending || self.gesture.is_some() || self.modal.is_some() {
            return;
        }
        self.menus.close();
        self.menus.row = Some(RowContext {
            session: self.session().id,
            layer,
            position,
        });
        self.menus.auxiliary = Some(8);
        cx.focus(quickgui::FocusHandle::new("application-menu-items"));
        cx.invalidate();
    }

    pub(super) fn row_menu_label(&self, default: &str, command: Command) -> String {
        let Some(layer) = self
            .menus
            .row
            .and_then(|row| self.current_document()?.layer(row.layer))
        else {
            return default.into();
        };
        let multiple = self.session().document.selected.contains(&layer.id)
            && self.session().document.selected.len() > 1;
        match command {
            Command::Visibility => {
                if layer.visible {
                    "Hide Layer"
                } else {
                    "Show Layer"
                }
            }
            Command::Edit(Action::Clip) => {
                if layer.clip_source.is_some() {
                    "Release Clipping Mask"
                } else {
                    "Create Clipping Mask"
                }
            }
            Command::Edit(Action::ToggleMask) => {
                if layer.mask.as_ref().is_some_and(|m| m.enabled) {
                    "Disable Mask"
                } else {
                    "Enable Mask"
                }
            }
            Command::Edit(Action::LinkMask) => {
                if layer.mask.as_ref().is_some_and(|m| m.linked) {
                    "Unlink Mask"
                } else {
                    "Link Mask"
                }
            }
            Command::Edit(Action::Merge) => {
                if multiple {
                    "Merge Layers"
                } else if layer.is_group() {
                    "Merge Group"
                } else {
                    "Merge Down"
                }
            }
            Command::Edit(Action::DeleteLayer) => {
                if multiple {
                    "Delete Layers"
                } else if layer.is_group() {
                    "Delete Folder"
                } else {
                    "Delete Layer"
                }
            }
            _ => default,
        }
        .into()
    }

    pub(super) fn row_menu_available(&self, command: Command) -> bool {
        let Some(row) = self.menus.row else {
            return false;
        };
        if !self.has_document() || !self.can_edit_layers() || self.session().id != row.session {
            return false;
        }
        let Some(layer) = self.session().document.layer(row.layer) else {
            return false;
        };
        match command {
            Command::Edit(Action::DevelopRaw | Action::RasterizeRaw) => layer.raw.is_some(),
            Command::Edit(Action::AddMask | Action::HideMask) => layer.mask.is_none(),
            Command::Edit(Action::ToggleMask | Action::DeleteMask) => layer.mask.is_some(),
            Command::Edit(Action::Clip) => {
                compositor::clipping::change(&self.session().document, row.layer).is_some()
            }
            Command::Edit(Action::LinkMask) => {
                layer.mask.is_some()
                    && !layer.is_group()
                    && !matches!(
                        layer.content,
                        compositor::document::LayerContent::Adjustment(_)
                    )
            }
            Command::Edit(Action::Duplicate) => true,
            Command::Edit(Action::Group) => self.session().document.layers.len() < 10_000,
            Command::Edit(Action::Merge) => {
                let doc = &self.session().document;
                let multiple = doc.selected.contains(&row.layer) && doc.selected.len() > 1;
                if multiple || layer.is_group() {
                    let ids = if multiple {
                        compositor::transform::selected_ids(doc)
                    } else {
                        doc.descendants(row.layer)
                    };
                    doc.layers
                        .iter()
                        .any(|l| ids.contains(&l.id) && !l.is_group())
                } else {
                    doc.layers
                        .iter()
                        .position(|l| l.id == row.layer)
                        .is_some_and(|index| {
                            doc.layers[..index]
                                .iter()
                                .rev()
                                .find(|l| l.parent == layer.parent)
                                .is_some_and(|l| !l.is_group())
                        })
                }
            }
            Command::Edit(Action::MoveOutOfGroup) => layer.parent.is_some(),
            Command::Edit(Action::Rename) => {
                let doc = &self.session().document;
                !doc.selected.contains(&row.layer) || doc.selected.len() == 1
            }
            Command::Edit(Action::DeleteLayer) | Command::Visibility => true,
            _ => false,
        }
    }

    pub(super) fn invoke_row_menu(&mut self, command: Command, cx: &mut EventContext) {
        let row = self.menus.row.filter(|_| self.row_menu_available(command));
        self.menus.close();
        cx.focus(quickgui::FocusHandle::new("workspace"));
        let Some(row) = row else {
            cx.invalidate();
            return;
        };
        match command {
            Command::Visibility => {
                let result = self.toggle_layer_visibility(row.layer);
                self.result(result, cx);
            }
            Command::Edit(Action::Clip) => {
                let result = self.toggle_clipping(row.layer);
                self.result(result, cx);
            }
            Command::Edit(action) => {
                let doc = &mut self.session_mut().document;
                if matches!(action, Action::Rename) {
                    doc.active = Some(row.layer);
                } else if !matches!(
                    action,
                    Action::DeleteLayer | Action::Duplicate | Action::Group | Action::Merge
                ) || !doc.selected.contains(&row.layer)
                {
                    doc.select(row.layer, false);
                }
                self.tools.mask_target = false;
                if matches!(action, Action::Duplicate) {
                    let result = self.session_mut().edit(
                        "Duplicate Layers",
                        compositor::layer_ops::duplicate_selected,
                    );
                    self.result(result, cx);
                } else {
                    self.action(action, cx);
                }
            }
            _ => cx.invalidate(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Point, WindowOptions};

    #[test]
    fn row_commands_duplicate_selected_layers_group_and_toggle_mask_links() {
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(Document::new(4, 4).unwrap(), None).into()];
        let first = editor.session().document.active.unwrap();
        compositor::edits::add_mask(&mut editor.session_mut().document, false).unwrap();
        editor.session_mut().duplicate().unwrap();
        editor.session_mut().document.select(first, true);
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Native row commands").size(1280., 900.),
                editor,
            )
            .unwrap();
        cx.update(view, |e, cx| {
            e.open_layer_context(first, Point::new(1100., 300.), cx);
            assert!(e.row_menu_available(Command::Edit(Action::Duplicate)));
            e.invoke_row_menu(Command::Edit(Action::Duplicate), cx);
            assert_eq!(e.session().document.layers.len(), 4);
            assert_eq!(e.session().document.selected.len(), 2);
            e.session_mut().undo();
            e.open_layer_context(first, Point::new(1100., 300.), cx);
            e.invoke_row_menu(Command::Edit(Action::LinkMask), cx);
            assert!(
                !e.session()
                    .document
                    .layer(first)
                    .unwrap()
                    .mask
                    .as_ref()
                    .unwrap()
                    .linked
            );
            e.session_mut().document.selected =
                e.session().document.layers.iter().map(|l| l.id).collect();
            e.open_layer_context(first, Point::new(1100., 300.), cx);
            e.invoke_row_menu(Command::Edit(Action::Group), cx);
            assert_eq!(e.session().document.layers.len(), 3);
            assert!(e.session().document.active_layer().unwrap().is_group());
        })
        .unwrap();
    }

    #[test]
    fn context_menu_preserves_selection_on_cancel_and_targets_clicked_layer() {
        let mut editor = Editor::with_test_document();
        let first = editor.session().document.layers[0].id;
        editor.session_mut().duplicate().unwrap();
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .bind_keys(quickgui::popover_menu_key_bindings())
            .into_test_context(WindowOptions::new("Row menu").size(1280., 900.), editor)
            .unwrap();
        let window = view.window_handle();
        let id = format!("layer-row-{first}");
        let point = Point::new(1120., 340.);
        cx.simulate_context_menu(window, id.clone(), point, Modifiers::empty())
            .unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
        cx.simulate_keystrokes(window, "escape").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
        cx.simulate_context_menu(window, id.clone(), point, Modifiers::empty())
            .unwrap();
        let hide = cx.read(view, |e| e.menus.command_id("Hide Layer")).unwrap();
        cx.click(window, hide).unwrap();
        cx.read(view, |e| {
            let doc = &e.session().document;
            assert!(!doc.layer(first).unwrap().visible);
            assert_eq!(doc.selected, original.selected);
            assert_eq!(doc.active, original.active);
            assert!(doc.active_layer().unwrap().visible);
        })
        .unwrap();
        cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
        cx.simulate_context_menu(window, id, point, Modifiers::empty())
            .unwrap();
        let mask = cx
            .read(view, |e| e.menus.command_id("Add White Mask"))
            .unwrap();
        cx.click(window, mask).unwrap();
        cx.read(view, |e| {
            let doc = &e.session().document;
            assert!(doc.layer(first).unwrap().mask.is_some());
            assert!(doc.layer(original.active.unwrap()).unwrap().mask.is_none());
            assert_eq!(doc.active, Some(first));
            assert!(e.tools.mask_target);
        })
        .unwrap();
    }

    #[test]
    fn context_delete_keeps_multiselection_and_deletes_layers_even_with_a_mask_target() {
        let mut editor = Editor::with_test_document();
        let first = editor.session().document.layers[0].id;
        compositor::edits::add_mask(&mut editor.session_mut().document, false).unwrap();
        editor.session_mut().duplicate().unwrap();
        editor.session_mut().document.select(first, true);
        editor.tools.mask_target = true;
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Delete row").size(1280., 900.), editor)
            .unwrap();
        let window = view.window_handle();
        cx.simulate_context_menu(
            window,
            format!("layer-row-{first}"),
            Point::new(1120., 340.),
            Modifiers::empty(),
        )
        .unwrap();
        let delete = cx
            .read(view, |e| e.menus.command_id("Delete Layers"))
            .unwrap();
        cx.click(window, delete).unwrap();
        assert!(
            cx.read(view, |e| e.session().document.layers.is_empty())
                .unwrap()
        );
        cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
    }
}
