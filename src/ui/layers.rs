use super::*;

impl Editor {
    fn empty_layers(&self) -> Element {
        div()
            .id("layer-list")
            .flex_1()
            .w_full()
            .flex_col()
            .items_center()
            .justify_center()
            .p(16.)
            .gap(10.)
            .text_color(Color::rgb8(160, 160, 160))
            .child(Icon::Layers.element(25.))
            .child(
                text("No layers yet")
                    .id("empty-layer-title")
                    .text_size(12.)
                    .line_height(15.)
                    .font_medium(),
            )
            .child(
                text(if self.has_document() {
                    "Import an image or add a blank layer."
                } else {
                    "Create a canvas or import an image."
                })
                .id("empty-layer-description")
                .text_size(10.)
                .line_height(13.)
                .text_center()
                .wrap(),
            )
    }

    pub(super) fn layers_panel(&mut self, cx: &mut ViewContext<'_, Self>) -> Element {
        if self
            .current_document()
            .is_none_or(|doc| doc.layers.is_empty())
        {
            return self.layers_shell(cx, self.empty_layers());
        }
        let ordered = self.prepare_layer_list();
        let mut rows = div()
            .w_full()
            .h(self.layer_list.content_height())
            .flex_col()
            .gap(2.);
        for (index, depth) in ordered {
            rows = rows.child(self.layer_row(cx, index, depth));
        }
        let root_drop = cx.drop_listener(
            "layers-root-drop",
            |this, payload: &super::layer_drag::LayerDrag, _, cx| {
                this.drop_layer(payload, super::layer_drag::DropPlacement::Root, cx)
            },
        );
        rows = rows.child(
            div()
                .h(32.)
                .flex_shrink_0()
                .w_full()
                .on_drop(root_drop)
                .drag_over(|s| s.bg(Color::rgb8(55, 65, 78))),
        );
        let list = self.layer_list_view(cx, rows);
        self.layers_shell(cx, list)
    }

    fn layers_shell(&self, cx: &mut ViewContext<'_, Self>, rows: Element) -> Element {
        let doc = self.current_document();
        let active = doc.and_then(Document::active_layer);
        let blend = active.map_or("Normal", |l| l.blend.label());
        div()
            .id("layers-panel")
            .border_left(1., Color::rgb8(62, 62, 62))
            .w(self.panel_layout.width)
            .flex_shrink_0()
            .relative()
            .h_full()
            .flex_col()
            .bg(Color::rgb8(36, 36, 36))
            .child(
                div()
                    .flex_row()
                    .items_center()
                    .p(18.)
                    .child(
                        text("Layers")
                            .text_size(12.)
                            .line_height(15.)
                            .font_semibold(),
                    )
                    .child(div().flex_1())
                    .child(
                        text(doc.map_or(0, |doc| doc.layers.len()).to_string())
                            .id("layer-count")
                            .text_size(10.)
                            .line_height(13.)
                            .font_features(
                                quickgui::FontFeatures::new()
                                    .enable(quickgui::FontFeatureTag::TABULAR_NUMBERS),
                            )
                            .text_color(Color::rgb8(135, 135, 135)),
                    ),
            )
            .child(Self::divider())
            .child(
                div()
                    .flex_col()
                    .p(12.)
                    .gap(8.)
                    .opacity(if self.can_edit_opacity() { 1. } else { 0.4 })
                    .child(
                        div()
                            .flex_row()
                            .items_center()
                            .gap(8.)
                            .child(text("Blend").text_size(10.).line_height(13.).w(28.))
                            .child(
                                self.action_button(cx, 500, blend, Action::Blend)
                                    .accessibility_label("Blend mode")
                                    .flex_1()
                                    .disabled(!self.can_edit_appearance())
                                    .child(div().flex_1())
                                    .child(Icon::PopupChevron.element(14.)),
                            ),
                    )
                    .child(
                        div()
                            .flex_row()
                            .items_center()
                            .gap(6.)
                            .child(text("Opacity").text_size(10.).line_height(13.).w(38.))
                            .child(
                                self.layer_opacity_slider(cx)
                                    .disabled(!self.can_edit_opacity()),
                            )
                            .child(
                                div()
                                    .flex_row()
                                    .items_center()
                                    .flex_shrink_0()
                                    .gap(2.)
                                    .child(self.layer_opacity_input(cx))
                                    .child(text("%").text_size(10.).line_height(13.)),
                            ),
                    ),
            )
            .child(Self::divider())
            .child(rows)
            .child(Self::divider())
            .child(
                div()
                    .flex_row()
                    .items_center()
                    .px(8.)
                    .py(4.)
                    .child(
                        self.icon_action(
                            cx,
                            510,
                            Icon::SquarePlus,
                            "New blank layer",
                            Action::AddLayer,
                        )
                        .w(33.)
                        .h(41.)
                        .text_color(Color::rgb8(164, 164, 164))
                        .tooltip("New blank layer (Ctrl+Shift+N)")
                        .disabled(!self.can_edit_layers()),
                    )
                    .child(
                        self.icon_action(
                            cx,
                            511,
                            Icon::FolderPlus,
                            "Group selected layers",
                            Action::Group,
                        )
                        .w(33.)
                        .h(41.)
                        .text_color(Color::rgb8(164, 164, 164))
                        .accessibility_label("New folder")
                        .tooltip("Group selected layers (Ctrl+G)")
                        .disabled(!self.can_edit_layers()),
                    )
                    .child(
                        self.layer_mask_button(cx)
                            .w(33.)
                            .h(41.)
                            .text_color(Color::rgb8(164, 164, 164)),
                    )
                    .child(self.effects_button(cx))
                    .child(
                        self.layer_menu_button(cx)
                            .w(33.)
                            .h(41.)
                            .text_color(Color::rgb8(164, 164, 164)),
                    )
                    .child(div().flex_1())
                    .child(
                        self.icon_action(
                            cx,
                            513,
                            Icon::Trash2,
                            if self.tools.mask_target
                                && doc.is_some_and(|doc| doc.selected.len() == 1)
                            {
                                "Delete layer mask"
                            } else if doc.is_some_and(|doc| doc.selected.len() > 1) {
                                "Delete selected layers"
                            } else {
                                "Delete selected layer"
                            },
                            Action::DeleteLayer,
                        )
                        .w(33.)
                        .h(41.)
                        .text_color(Color::rgb8(164, 164, 164))
                        .disabled(!self.can_edit_layers() || active.is_none()),
                    ),
            )
            .child(self.panel_resize_edge(cx))
    }
    pub(super) fn divider() -> Element {
        div().h(1.).flex_shrink_0().bg(Color::rgb8(53, 53, 53))
    }
}
