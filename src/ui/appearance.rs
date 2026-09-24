use super::*;
use compositor::{blend::Blend, invalid};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq)]
struct ChooseBlend {
    session: Uuid,
    layer: Uuid,
    mode: Blend,
}

pub(super) struct BlendPicker {
    target: Option<(Uuid, Uuid)>,
    menu: quickgui::PopoverMenu,
}
impl BlendPicker {
    pub(super) fn new() -> Result<Self> {
        Ok(Self {
            target: None,
            menu: quickgui::PopoverMenu::new([]).map_err(|e| invalid(e.to_string()))?,
        })
    }
    pub(super) fn close(&mut self) {
        self.target = None;
    }
}

impl Editor {
    pub(super) fn preview_document(&self) -> Document {
        let session = self.session();
        let mut doc = self
            .color_text_preview()
            .unwrap_or(&session.document)
            .clone();
        if matches!(self.modal, Some(Form::Blend))
            && let Some((project, layer)) = self.blend_picker.target
            && project == session.id
            && doc.active == Some(layer)
            && self.can_edit_appearance()
            && let Some(mode) = self
                .blend_picker
                .menu
                .active_index()
                .and_then(|i| Blend::ALL.get(i))
            && let Some(layer) = doc.active_layer_mut()
        {
            layer.blend = *mode;
        }
        doc
    }

    pub(super) fn open_blend(&mut self) -> Result<()> {
        let doc = &self.session().document;
        if doc.selected.len() != 1 || doc.active_layer().is_none_or(|l| l.is_group()) {
            return Err(invalid(
                "Select one pixel or adjustment layer to change its blend mode.",
            ));
        }
        let layer = doc
            .active_layer()
            .ok_or_else(|| invalid("Select a layer to change its blend mode."))?;
        let (layer_id, selected) = (layer.id, layer.blend);
        let session = self.session().id;
        self.blend_picker.target = Some((session, layer_id));
        self.blend_picker.menu =
            quickgui::PopoverMenu::new(Blend::ALL.into_iter().enumerate().map(|(index, mode)| {
                quickgui::PopoverMenuItem::radio(
                    format!("blend-mode-{index}"),
                    mode.label(),
                    "blend-modes",
                    mode == selected,
                    ChooseBlend {
                        session,
                        layer: layer_id,
                        mode,
                    },
                )
                .close_on_activate(false)
            }))
            .map_err(|e| invalid(e.to_string()))?;
        if let Some(index) = Blend::ALL.iter().position(|mode| *mode == selected) {
            self.blend_picker.menu.highlight(index);
        }
        self.modal = Some(Form::Blend);
        Ok(())
    }

    fn set_blend(&mut self, mode: Blend) -> Result<()> {
        self.blend_picker.close();
        if self.session().document.selected.len() != 1 {
            return Ok(());
        }
        self.session_mut().edit("Layer Blend Mode", |doc| {
            if let Some(layer) = doc.active_layer_mut()
                && !layer.is_group()
            {
                layer.blend = mode;
            }
            Ok(())
        })
    }

    pub(super) fn step_blend(&mut self, forward: bool) -> Result<()> {
        if !self.can_edit_appearance() {
            return Ok(());
        }
        let Some(layer) = self.session().document.active_layer() else {
            return Ok(());
        };
        let index = Blend::ALL
            .iter()
            .position(|mode| *mode == layer.blend)
            .unwrap_or(0);
        self.set_blend(
            Blend::ALL[(index + if forward { 1 } else { Blend::ALL.len() - 1 }) % Blend::ALL.len()],
        )
    }

    pub(super) fn can_edit_opacity(&self) -> bool {
        self.can_edit_layers()
            && self.session().document.selected.len() == 1
            && self.session().document.active_layer().is_some()
    }

    pub(super) fn can_edit_appearance(&self) -> bool {
        self.can_edit_layers()
            && self.session().document.selected.len() == 1
            && self
                .session()
                .document
                .active_layer()
                .is_some_and(|l| !l.is_group())
    }

    pub(super) fn blend_popover(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let content = self.blend_controls(cx);
        quickgui::Popover::new(500_u64, "blend-popup", true)
            .kind(quickgui::PopoverKind::Menu)
            .side(quickgui::AnchorSide::Bottom)
            .align(quickgui::AnchorAlign::Start)
            .initial_focus("blend-menu-items")
            .surface_with(
                super::menus::style::surface()
                    .child(content)
                    .on_action(cx.action_listener(
                        "blend-popup",
                        |this, choice: &ChooseBlend, cx| {
                            if this.blend_picker.target != Some((choice.session, choice.layer))
                                || !this.can_edit_appearance()
                                || this.session().id != choice.session
                                || this.session().document.active != Some(choice.layer)
                            {
                                this.close_blend(cx);
                                return;
                            }
                            let result = this.set_blend(choice.mode);
                            this.modal = None;
                            this.result(result, cx);
                        },
                    ))
                    .on_dismiss(
                        cx.dismiss_listener("blend-popup", |this, cx| this.close_blend(cx)),
                    ),
            )
    }

    pub(super) fn blend_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let max_height = (cx.size().height - 60.).max(80.);
        self.blend_picker.menu.element(
            cx,
            "blend-menu-items",
            |this| &mut this.blend_picker.menu,
            div()
                .w(190.)
                .max_h(max_height)
                .overflow_y_scroll()
                .flex_col()
                .p(5.)
                .gap(1.),
            |item, state| super::menus::style::choice(item.label().clone(), state),
            |this, cx| this.close_blend(cx),
        )
    }

    fn close_blend(&mut self, cx: &mut EventContext) {
        self.modal = None;
        self.blend_picker.close();
        cx.focus(quickgui::FocusHandle::new(500_u64));
        self.changed(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blend_preview_leaves_project_history_and_saved_state_unchanged() {
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(Document::new(4, 4).unwrap(), None).into()];
        e.session_mut().mark_saved("example.comp".into());
        let original = e.session().document.clone();
        e.open_blend().unwrap();
        for (index, mode) in Blend::ALL.into_iter().enumerate() {
            e.blend_picker.menu.highlight(index);
            assert_eq!(e.preview_document().active_layer().unwrap().blend, mode);
            assert_eq!(e.session().document, original);
            assert!(!e.session().dirty());
            assert!(e.session().undo_label().is_none());
        }
        e.set_blend(Blend::Multiply).unwrap();
        assert!(e.blend_picker.target.is_none());
        e.session_mut().undo();
        assert_eq!(e.session().document, original);
        e.step_blend(false).unwrap();
        assert_eq!(
            e.session().document.active_layer().unwrap().blend,
            Blend::Luminosity
        );
        e.step_blend(true).unwrap();
        assert_eq!(
            e.session().document.active_layer().unwrap().blend,
            Blend::Normal
        );
    }

    #[test]
    fn keyboard_blend_preview_cancels_without_history_and_selection_commits() {
        use quickgui::{Application, Keystroke, WindowOptions};
        let (mut cx, view) = Application::new()
            .bind_keys(quickgui::popover_menu_key_bindings())
            .into_test_context(
                WindowOptions::new("Blend menu").size(1280., 900.),
                Editor::with_test_document(),
            )
            .unwrap();
        let window = view.window_handle();
        let original = cx.read(view, |e| e.session().document.clone()).unwrap();
        cx.click(window, 500_u64).unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::ArrowDown, Modifiers::empty()))
            .unwrap();
        cx.read(view, |e| {
            assert_eq!(
                e.preview_document().active_layer().unwrap().blend,
                Blend::Darken
            );
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::Escape, Modifiers::empty()))
            .unwrap();
        assert_eq!(cx.read(view, |e| e.preview_document()).unwrap(), original);
        cx.click(window, 500_u64).unwrap();
        for key in [Key::ArrowDown, Key::ArrowDown, Key::Enter] {
            cx.simulate_keystroke(window, Keystroke::new(key, Modifiers::empty()))
                .unwrap();
        }
        cx.read(view, |e| {
            assert_eq!(
                e.session().document.active_layer().unwrap().blend,
                Blend::Multiply
            );
            assert!(e.modal.is_none());
            assert_eq!(e.session().undo_label(), Some("Layer Blend Mode"));
        })
        .unwrap();
        cx.update(view, |e, cx| {
            e.session_mut().undo();
            cx.invalidate();
        })
        .unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
    }
}
