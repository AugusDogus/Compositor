use super::*;

impl Editor {
    fn layer_description(
        &self,
        cx: &mut ViewContext<'_, Self>,
        layer: &compositor::document::Layer,
    ) -> Element {
        let detail = if let Some(source) = layer.clip_source {
            format!(
                "Clipped to {}",
                self.session()
                    .document
                    .layer(source)
                    .map_or("Missing source", |l| l.name.as_str())
            )
        } else if matches!(
            layer.content,
            compositor::document::LayerContent::Adjustment(_)
        ) {
            "Adjustment · Double-click to edit".into()
        } else if layer.text.is_some() {
            "Text · Double-click to edit".into()
        } else if layer.is_group() {
            "Folder".into()
        } else {
            format!(
                "{:.0} × {:.0} px",
                layer.transform.size[0], layer.transform.size[1]
            )
        };
        div()
            .flex_1()
            .min_w(0.)
            .flex_col()
            .gap(3.)
            .py(9.)
            .child(
                if self
                    .rename
                    .as_ref()
                    .is_some_and(|rename| rename.layer == layer.id)
                {
                    self.rename_input(cx)
                } else {
                    text(format!(
                        "{}{}",
                        if layer.clip_source.is_some() {
                            "↳ "
                        } else {
                            ""
                        },
                        layer.name
                    ))
                    .text_size(13.)
                    .truncate()
                },
            )
            .child(
                text(detail)
                    .id(format!("layer-detail-{}", layer.id))
                    .text_size(10.)
                    .text_color(Color::rgb8(155, 155, 155))
                    .truncate(),
            )
    }

    pub(super) fn layer_row(
        &self,
        cx: &mut ViewContext<'_, Self>,
        index: usize,
        depth: usize,
    ) -> Element {
        let doc = &self.session().document;
        let layer = &doc.layers[index];
        let id = layer.id;
        let mut effective_visible = layer.visible;
        let mut parent = layer.parent;
        while let Some(ancestor) = parent.and_then(|id| doc.layer(id)) {
            effective_visible &= ancestor.visible;
            parent = ancestor.parent;
        }
        let payload = super::layer_drag::LayerDrag {
            session: self.session().id,
            layer: id,
            operation: super::layer_drag::Transfer::Move,
        };
        let drag = cx.drag_listener(1001 + index as u64 * 3, move |this, event, cx| {
            this.pending_layer_click = None;
            let mut payload = payload.clone();
            if event.modifiers.contains(Modifiers::ALT) {
                payload.operation = super::layer_drag::Transfer::Copy;
            }
            this.tab_scrolling.dragging = payload.operation == super::layer_drag::Transfer::Move;
            cx.invalidate();
            quickgui::Drag::new(payload)
        });
        let center_drop = if layer.is_group() {
            super::layer_drag::DropPlacement::Into(id)
        } else {
            super::layer_drag::DropPlacement::Above(id)
        };
        let drop = cx.drop_listener(
            1001 + index as u64 * 3,
            move |this, payload: &super::layer_drag::LayerDrag, _, cx| {
                this.drop_layer(payload, center_drop, cx)
            },
        );
        let expand =
            cx.listener(1002 + index as u64 * 3, move |this, cx| {
                if let Err(error) = this.finish_pending_edits() {
                    this.result(Err(error), cx);
                    return;
                }
                if !this.session_mut().collapsed.remove(&id) {
                    this.session_mut().collapsed.insert(id);
                    if this.session().document.active.is_some_and(|active| {
                        this.session().document.descendants(id).contains(&active)
                    }) {
                        this.session_mut().select_layer(id, false);
                    }
                }
                this.changed(cx);
            });
        let visible = cx.pointer_listener(format!("layer-eye-{id}"), move |this, event, cx| {
            let result = this.visibility_pointer(id, event);
            cx.stop_propagation();
            this.result(result, cx);
        });
        let select = cx.mouse_down_listener(1001 + index as u64 * 3, move |this, event, cx| {
            let target_changed = this.session().document.active != Some(id)
                || !this.session().document.selected.contains(&id)
                || this.tools.mask_target;
            if target_changed && let Err(error) = this.finish_pending_edits() {
                this.result(Err(error), cx);
                return;
            }
            let extend = event.modifiers.contains(Modifiers::SHIFT);
            this.pending_layer_click = (!extend).then_some(id);
            if extend || !this.session().document.selected.contains(&id) {
                this.session_mut().select_layer(id, extend);
            } else {
                this.session_mut().select_layer(id, true);
            }
            this.tools.mask_target = false;
            if event.click_count == 2
                && this
                    .session()
                    .document
                    .active_layer()
                    .is_some_and(|layer| layer.text.is_some())
            {
                let result = this.edit_active_text();
                this.result(result, cx);
            } else if event.click_count == 2 {
                let action = if this.session().document.active_layer().is_some_and(|l| {
                    matches!(l.content, compositor::document::LayerContent::Adjustment(_))
                }) {
                    Action::EditAdjustment
                } else {
                    Action::Rename
                };
                this.action(action, cx);
            }
            this.changed(cx);
        });
        let click = cx.listener(1001 + index as u64 * 3, move |this, cx| {
            if this.pending_layer_click.take() == Some(id) {
                if this.session().document.selected.len() > 1
                    && let Err(error) = this.finish_pending_edits()
                {
                    this.result(Err(error), cx);
                    return;
                }
                this.session_mut().select_layer(id, false);
                this.changed(cx);
            }
        });
        let row_id = format!("layer-row-{id}");
        div()
            .id(row_id.clone())
            .report_bounds(
                self.layer_list
                    .cursors
                    .rows
                    .get(&id)
                    .map(|bounds| bounds.row.clone())
                    .unwrap_or_default(),
            )
            .on_context_menu(cx.context_menu_listener(row_id, move |this, event, cx| {
                this.open_layer_context(id, event.position, cx);
            }))
            .flex_row()
            .relative()
            .gap(0.)
            .items_center()
            .h(52.)
            .opacity(if effective_visible { 1. } else { 0.35 })
            .flex_shrink_0()
            .px(8.)
            .bg(if doc.selected.contains(&id) {
                Color::rgb8(69, 69, 69)
            } else {
                Color::TRANSPARENT
            })
            .child(
                (if layer.visible {
                    Icon::Eye
                } else {
                    Icon::EyeOff
                })
                .button(if layer.visible {
                    "Hide layer"
                } else {
                    "Show layer"
                })
                .w(20.)
                .h(32.)
                .text_color(Color::rgb8(164, 164, 164))
                .accessibility_label(format!(
                    "{} {}",
                    if layer.visible { "Hide" } else { "Show" },
                    layer.name
                ))
                .disabled(!self.can_edit_layers())
                .on_pointer(visible)
                // Pointer tracking already toggles on press. Suppress its
                // release click, but retain activation for assistive input.
                .on_mouse_up(
                    quickgui::MouseButton::Left,
                    cx.mouse_up_listener(format!("layer-eye-{id}"), |_, _, cx| {
                        cx.prevent_default()
                    }),
                )
                .on_click(cx.listener(format!("layer-eye-{id}"), move |this, cx| {
                    let result = this
                        .begin_visibility_swipe(id)
                        .and_then(|()| this.finish_visibility_swipe());
                    this.result(result, cx);
                }))
                .on_key_down(cx.key_down_listener(
                    format!("layer-eye-{id}"),
                    move |this, event, cx| {
                        if matches!(event.key, Key::Enter | Key::Space) {
                            let result = this
                                .begin_visibility_swipe(id)
                                .and_then(|()| this.finish_visibility_swipe());
                            this.result(result, cx);
                            cx.prevent_default();
                            cx.stop_propagation();
                        }
                    },
                )),
            )
            .child(
                div()
                    .w((depth.min(8) * 24 + usize::from(layer.clip_source.is_some()) * 24) as f32)
                    .flex_shrink_0(),
            )
            .child(if layer.is_group() {
                (if self.session().collapsed.contains(&id) {
                    Icon::ChevronRight
                } else {
                    Icon::ChevronDown
                })
                .button("Expand or collapse folder")
                .disabled(!self.can_edit_layers())
                .w(16.)
                .h(24.)
                .mr(-2.)
                .on_click(expand)
            } else {
                div().w(14.)
            })
            .child(
                div()
                    .flex_1()
                    .min_w(0.)
                    .h_full()
                    .relative()
                    .flex_row()
                    .items_center()
                    .gap(5.)
                    .on_mouse_down(quickgui::MouseButton::Left, select)
                    .on_click(click)
                    .when(self.can_edit_layers(), |row| row.on_drag(drag))
                    .on_drop(drop)
                    .drag_over(|s| s.bg(Color::rgb8(70, 95, 115)))
                    .child(self.layer_thumbnails(cx, layer))
                    .child(self.layer_description(cx, layer))
                    .child(self.layer_drop_zone(cx, id, false))
                    .child(self.layer_drop_zone(cx, id, true)),
            )
            .child(
                div()
                    .absolute()
                    .left(0.)
                    .bottom(0.)
                    .w_full()
                    .h(1. / cx.scale_factor())
                    .bg(Color::rgba8(255, 255, 255, 15)),
            )
    }
}
