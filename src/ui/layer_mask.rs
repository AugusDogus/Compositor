//! The source footer adds a mask directly; mask management belongs to the row menu.
use super::*;
use compositor::invalid;

impl Editor {
    pub(super) fn mask_link_slot(
        &self,
        cx: &mut ViewContext<'_, Self>,
        layer: &compositor::document::Layer,
        matte: &compositor::document::Mask,
    ) -> Element {
        let id = layer.id;
        div()
            .w(13.)
            .h(36.)
            .flex_shrink_0()
            .flex_row()
            .items_center()
            .justify_center()
            .child(
                button()
                    .w(9.)
                    .h(20.)
                    .p(0.)
                    .flex_shrink_0()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .bg(Color::TRANSPARENT)
                    .text_color(Color::rgb8(155, 155, 155))
                    .accessibility_label(format!(
                        "{} mask: {}",
                        if matte.linked { "Unlink" } else { "Link" },
                        layer.name
                    ))
                    .tooltip(if matte.linked {
                        "Unlink layer and mask to move or transform them separately"
                    } else {
                        "Link layer and mask so they move together"
                    })
                    .child(if matte.linked {
                        Icon::Link
                            .element(13.)
                            .rotate_degrees(-45.)
                            .id(format!("mask-link-glyph-{id}"))
                    } else {
                        div()
                    })
                    .on_mouse_down(
                        quickgui::MouseButton::Left,
                        cx.mouse_down_listener(format!("mask-link-{id}"), |this, _, cx| {
                            this.pending_layer_click = None;
                            cx.stop_propagation();
                        }),
                    )
                    .on_click(cx.listener(format!("mask-link-{id}"), move |this, cx| {
                        cx.stop_propagation();
                        if !this.can_edit_layers() {
                            return;
                        }
                        let result = this
                            .finish_pending_edits()
                            .and_then(|()| this.toggle_mask_link(id));
                        this.result(result, cx);
                    })),
            )
    }

    pub(super) fn toggle_mask_link(&mut self, id: uuid::Uuid) -> Result<()> {
        let label = if self
            .session()
            .document
            .layer(id)
            .and_then(|layer| layer.mask.as_ref())
            .is_some_and(|mask| mask.linked)
        {
            "Unlink Layer Mask"
        } else {
            "Link Layer Mask"
        };
        self.session_mut().edit(label, |doc| {
            let layer = doc
                .layers
                .iter_mut()
                .find(|layer| layer.id == id)
                .ok_or_else(|| invalid("The mask layer was removed."))?;
            let mask = layer
                .mask
                .as_mut()
                .ok_or_else(|| invalid("The layer mask was removed."))?;
            mask.linked = !mask.linked;
            if !mask.linked && mask.placement.is_none() {
                mask.placement = Some(layer.transform);
            }
            Ok(())
        })
    }

    pub(super) fn retain_mask_target(&mut self, previous: Option<uuid::Uuid>) {
        let doc = &self.session().document;
        self.tools.mask_target &=
            doc.active == previous && doc.active_layer().is_some_and(|layer| layer.mask.is_some());
    }

    pub(super) fn layer_mask_button(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let doc = self.current_document();
        Icon::Mask
            .button(if doc.is_some_and(|doc| doc.selection.is_some()) {
                "Add layer mask (the selection becomes black)"
            } else {
                "Add layer mask"
            })
            .id("layer-add-mask")
            .accessibility_label("Add layer mask")
            .disabled(
                !self.can_edit_layers()
                    || doc.is_none_or(|doc| {
                        doc.selected.len() != 1
                            || doc.active_layer().is_none_or(|l| l.mask.is_some())
                    }),
            )
            .on_click(cx.listener("layer-add-mask", |this, cx| {
                this.action(Action::AddMask, cx);
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn structural_commands_keep_a_mask_target_only_when_its_layer_stays_active() {
        for (label, action, changes_active) in [
            ("group", Action::Group, true),
            ("merge", Action::Merge, true),
            ("delete", Action::DeleteLayerUnlinked, true),
            ("reorder", Action::Lower, false),
            ("toggle mask", Action::ToggleMask, false),
        ] {
            let mut editor = Editor::with_test_document();
            let mut doc = Document::new(16, 16).unwrap();
            compositor::edits::fill(&mut doc, [120, 30, 20, 255], false, false).unwrap();
            compositor::edits::add_mask(&mut doc, false).unwrap();
            doc.add(compositor::document::Layer::blank("Second", 16, 16))
                .unwrap();
            compositor::edits::fill(&mut doc, [20, 120, 80, 255], false, false).unwrap();
            compositor::edits::add_mask(&mut doc, false).unwrap();
            let original = doc.clone();
            editor.tabs = vec![Session::new(doc, None).into()];
            editor.tools.mask_target = true;
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Structural mask target").size(1280., 900.),
                    editor,
                )
                .unwrap();
            cx.update(view, |e, cx| e.action(action, cx)).unwrap();
            cx.read(view, |e| {
                assert_eq!(
                    e.session().document.active != original.active,
                    changes_active
                );
                assert_eq!(e.tools.mask_target, !changes_active, "{label}");
            })
            .unwrap();
            cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
            assert_eq!(
                cx.read(view, |e| e.session().document.clone()).unwrap(),
                original
            );
        }
    }

    #[test]
    fn footer_adds_selection_mask_immediately_and_preserves_it_on_second_click() {
        let mut editor = Editor::with_test_document();
        editor.session_mut().document.selection = Some(
            compositor::selection::Selection::rectangle(4, 4, [0., 0.], [2., 4.], false),
        );
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Mask footer").size(1280., 900.), editor)
            .unwrap();
        let window = view.window_handle();
        cx.click(window, "layer-add-mask").unwrap();
        let masked = cx
            .read(view, |e| {
                assert!(e.tools.mask_target);
                assert!(e.modal.is_none());
                assert!(e.session().document.selection.is_none());
                assert!(e.session().document.layers[0].mask.is_some());
                e.session().document.clone()
            })
            .unwrap();
        assert!(matches!(
            cx.click(window, "layer-add-mask"),
            Err(quickgui::TestAppError::NotClickable { .. })
        ));
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            masked
        );
        cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
    }
}
