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
            Command::Edit(Action::AddMask | Action::HideMask) => layer.mask.is_none(),
            Command::Edit(Action::ToggleMask | Action::DeleteMask) => layer.mask.is_some(),
            Command::Edit(Action::Clip) => layer.clip_source.is_some(),
            Command::Edit(Action::MoveOutOfGroup) => layer.parent.is_some(),
            Command::Edit(Action::Rename | Action::DeleteLayer) | Command::Visibility => true,
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
                } else if !matches!(action, Action::DeleteLayer)
                    || !doc.selected.contains(&row.layer)
                {
                    doc.select(row.layer, false);
                }
                self.tools.mask_target = false;
                self.action(action, cx);
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
        let hide = cx
            .read(view, |e| e.menus.command_id("Hide/Show Layer"))
            .unwrap();
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
            .read(view, |e| e.menus.command_id("Delete Layer / Folder"))
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
