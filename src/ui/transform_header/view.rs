//! TransformInspector.swift: fixed title/actions around horizontally scrolling fields.
use super::*;

impl Editor {
    pub(in crate::ui) fn transform_header(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let targets_mask = self.tools.mask_target
            && self
                .current_document()
                .and_then(Document::active_layer)
                .and_then(|l| l.mask.as_ref())
                .is_some_and(|m| !m.linked);
        div()
            .h(42.)
            .flex_shrink_0()
            .flex_row()
            .items_center()
            .gap(12.)
            .px(18.)
            .bg(Color::rgb8(38, 38, 38))
            .child(
                text(if targets_mask {
                    "Transform Mask"
                } else {
                    "Transform"
                })
                .text_size(13.)
                .line_height(16.)
                .font_semibold()
                .flex_shrink_0(),
            )
            .child(
                div()
                    .id("transform-fields-scroll")
                    .flex_1()
                    .min_w(0.)
                    .overflow_x_scroll()
                    .child(self.transform_scroll_controls(cx)),
            )
            .child(self.transform_sampling_picker(cx))
            .child(
                Self::tool_header_control("Cancel")
                    .flex_shrink_0()
                    .disabled(self.transform_edit.is_none() && self.pending_pixels.is_none())
                    .disabled_style(|style| style.opacity(0.4))
                    .on_click(cx.listener("transform-cancel", |this, cx| {
                        let result = this.finish_toolbar_transform(false);
                        cx.focus(quickgui::FocusHandle::new("workspace"));
                        this.result(result, cx);
                    })),
            )
            .child(
                Self::tool_header_control("Apply")
                    .flex_shrink_0()
                    .disabled(self.transform_edit.is_none() && self.pending_pixels.is_none())
                    .disabled_style(|style| style.opacity(0.4))
                    .on_click(cx.listener("transform-apply", |this, cx| {
                        let result = this.finish_toolbar_transform(true);
                        cx.focus(quickgui::FocusHandle::new("workspace"));
                        this.result(result, cx);
                    })),
            )
    }
    fn transform_scroll_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        div()
            .flex_row()
            .items_center()
            .gap(12.)
            .flex_shrink_0()
            .child(
                Self::check_control("Auto Select", self.tools.transform_auto_select)
                    .tooltip("Select layers by clicking the canvas. When off, hold Ctrl to select a layer.")
                    .flex_shrink_0()
                    .on_click(cx.listener("transform-auto-select", |this, cx| {
                        this.tools.transform_auto_select = !this.tools.transform_auto_select;
                        this.changed(cx);
                    })),
            )
            .child(
                Self::check_control("Show Controls", self.tools.show_transform_controls)
                    .tooltip("Show the transform box and handles (Ctrl+H). When hidden, drag anywhere to move the layer.")
                    .flex_shrink_0()
                    .on_click(cx.listener("transform-show-controls", |this, cx| {
                        this.tools.show_transform_controls = !this.tools.show_transform_controls;
                        this.changed(cx);
                    })),
            )
            .child(self.transform_numeric_controls(cx))
    }

    fn transform_numeric_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let bounds = self.header_transform_bounds();
        let disabled = bounds.is_none() || !self.can_edit_transform_numbers();
        let transform = bounds.unwrap_or(Transform::new(1, 1));
        let current = self.transform_edit.as_ref().map_or_else(
            || values(transform, self.header_transform_pixel_size(transform)),
            |e| e.values.clone(),
        );
        let mut row = div()
            .flex_row()
            .items_center()
            .gap(12.)
            .px(18.)
            .flex_shrink_0();
        for (index, label) in ["X", "Y", "W", "H", "Scale", "°"].into_iter().enumerate() {
            let id = format!("transform-value-{index}");
            let mut field = Self::text_field(current[index].clone())
                .flex_1()
                .min_w(0.)
                .h(24.)
                .text_input_padding(5.)
                .rounded(5.)
                .text_size(12.)
                .line_height(15.)
                .bg(Color::rgb8(29, 29, 29))
                .border(1., Color::rgb8(67, 67, 67))
                .accessibility_label(format!("Transform {label}"))
                .disabled(disabled)
                .on_input(cx.input_listener(id.clone(), move |this, input, cx| {
                    let result = this.header_transform_input(index, input);
                    this.result(result, cx);
                }))
                .on_key_down(cx.key_down_listener(id, move |this, event, cx| {
                    if matches!(event.key, Key::Enter | Key::Escape) {
                        let result = this.finish_toolbar_transform(event.key == Key::Enter);
                        this.result(result, cx);
                        cx.focus(quickgui::FocusHandle::new("workspace"));
                        cx.prevent_default();
                        cx.stop_propagation();
                    } else if matches!(event.key, Key::ArrowUp | Key::ArrowDown) {
                        let bounds = this
                            .header_transform_bounds()
                            .unwrap_or(Transform::new(1, 1));
                        let current = this.transform_edit.as_ref().map_or_else(
                            || values(bounds, this.header_transform_pixel_size(bounds)),
                            |e| e.values.clone(),
                        );
                        if let Ok(value) = current[index].parse::<f64>() {
                            let step = if event.modifiers.contains(Modifiers::SHIFT) {
                                10.
                            } else {
                                1.
                            };
                            let next = value
                                + if event.key == Key::ArrowUp {
                                    step
                                } else {
                                    -step
                                };
                            let result = this.header_transform_input(index, &format_value(next));
                            this.result(result, cx);
                        }
                        cx.prevent_default();
                        cx.stop_propagation();
                    } else {
                        cx.stop_propagation();
                    }
                }));
            if index == 4 {
                field = field.tooltip("Scale width and height together, about the center");
            }
            row = row.child(
                div()
                    .flex_row()
                    .items_center()
                    .gap(4.)
                    .w(if index == 4 {
                        110.
                    } else if index == 5 {
                        75.
                    } else {
                        85.
                    })
                    .flex_shrink_0()
                    .child(
                        text(label)
                            .text_size(10.)
                            .line_height(13.)
                            .text_color(Color::rgb8(165, 165, 165)),
                    )
                    .child(field)
                    .child(if index == 4 {
                        text("%")
                            .text_size(10.)
                            .line_height(13.)
                            .text_color(Color::rgb8(165, 165, 165))
                    } else {
                        div()
                    }),
            );
            if index == 3 {
                row = row.child(
                    (if self.tools.transform_ratio {
                        Icon::Link
                    } else {
                        Icon::Unlink
                    })
                    .button("Lock aspect ratio")
                    .disabled(disabled)
                    .w(24.)
                    .on_click(cx.listener("transform-ratio", |this, cx| {
                        this.tools.transform_ratio = !this.tools.transform_ratio;
                        cx.invalidate();
                    })),
                );
            }
        }
        for (horizontal, label, id) in [
            (true, "Flip H", "transform-flip-h"),
            (false, "Flip V", "transform-flip-v"),
        ] {
            row = row.child(
                Self::tool_header_control(label)
                    .flex_shrink_0()
                    .disabled(disabled)
                    .on_click(cx.listener(id, move |this, cx| {
                        let result = this.change_header_transform(|value| {
                            if horizontal {
                                value.flip_x = !value.flip_x;
                            } else {
                                value.flip_y = !value.flip_y;
                            }
                        });
                        this.result(result, cx);
                    })),
            );
        }
        row.disabled(disabled)
            .opacity(if disabled { 0.4 } else { 1. })
    }
}
