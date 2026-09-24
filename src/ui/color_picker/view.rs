//! Layout follows ColorPickerSheet.swift: field, hue strip, preview/buttons and RGB entry.
use super::*;

// The source's 538-point content includes both 20-point insets; add the panel border.
const WIDTH: f32 = 540.;
const HEIGHT: f32 = 326.;

impl Editor {
    pub(in crate::ui) fn color_picker_view(
        &mut self,
        cx: &mut ViewContext<'_, Self>,
        picker: &Picker,
    ) -> Element {
        let available = cx.size();
        let position = *self.color_picker_position.get_or_insert([
            56. + (available.width - 56. - self.panel_layout.width - WIDTH) / 2.,
            116. + (available.height - 146. - HEIGHT) / 2.,
        ]);
        let title = match picker.purpose {
            Purpose::LayerEffect { .. } => "Layer effect color",
            Purpose::LayerText { .. } => "Text color",
            Purpose::ForegroundText { .. } => "Color Picker (Foreground Color)",
            Purpose::CanvasExtension { .. } => "Canvas extension color",
            Purpose::JpegBackground { .. } => "JPEG background color",
            Purpose::Palette if matches!(picker.target, Target::Background) => {
                "Color Picker (Background Color)"
            }
            Purpose::Palette => "Color Picker (Foreground Color)",
            Purpose::GradientMap { .. } if matches!(picker.target, Target::Background) => {
                "Color Picker (Gradient Map Highlights)"
            }
            Purpose::GradientMap { .. } => "Color Picker (Gradient Map Shadows)",
        };
        let header = div()
            .id("color-picker-title")
            .h(28.)
            .flex_shrink_0()
            .px(8.)
            .flex_row()
            .items_center()
            .bg(Color::rgb8(52, 52, 52))
            .rounded_t(7.)
            .child(
                Icon::X
                    .button("Cancel color selection")
                    .size(20., 20.)
                    .on_click(cx.listener("color-picker-close", |this, cx| {
                        let result = this.finish_color(false);
                        this.result(result, cx);
                    })),
            )
            .child(div().flex_1())
            .child(text(title).text_size(12.).font_semibold())
            .child(div().flex_1())
            .child(div().w(20.))
            .on_pointer(
                cx.pointer_listener("color-picker-title", |this, event, cx| {
                    if event.phase == PointerPhase::Move && event.button == MouseButton::Left {
                        if let Some(position) = &mut this.color_picker_position {
                            position[0] = (position[0] + event.delta.x).max(0.);
                            position[1] = (position[1] + event.delta.y).max(0.);
                        }
                        cx.invalidate();
                    }
                }),
            );
        let body = div()
            .flex_row()
            .gap(14.)
            .p(20.)
            .flex_shrink_0()
            .child(self.picker_field(cx, picker))
            .child(self.picker_hue(cx, picker))
            .child(self.picker_sidebar(cx, picker));
        let panel = div()
            .track_focus(quickgui::FocusHandle::new("color-picker"))
            .auto_focus()
            .focus_trap()
            .restore_previous_focus()
            .absolute()
            .block_pointer()
            .left(position[0].clamp(0., (available.width - WIDTH).max(0.)))
            .top(position[1].clamp(0., (available.height - HEIGHT).max(0.)))
            .w(WIDTH)
            .max_h((available.height - 20.).max(100.))
            .overflow_y_scroll()
            .flex_col()
            .bg(Color::rgb8(45, 45, 45))
            .border(1., Color::rgb8(90, 90, 90))
            .rounded(8.)
            .shadow(crate::ui::surfaces::panel_shadow())
            .child(header)
            .child(body);
        self.panel_activation.present(panel)
    }

    fn picker_field(&self, cx: &mut ViewContext<'_, Self>, picker: &Picker) -> Element {
        let hsb = picker.current().hsb;
        div()
            .id("color-field")
            .size(256., 256.)
            .flex_shrink_0()
            .relative()
            .overflow_hidden()
            .accessibility_label("Saturation and brightness")
            .child(spectrum::field(
                &self.color_picker_shader,
                hsb.hue,
                false,
                256.,
            ))
            .child(spectrum::marker(hsb).absolute().size_full())
            .child(
                div()
                    .absolute()
                    .size_full()
                    .border(1., Color::rgba8(0, 0, 0, 153)),
            )
            .cursor(quickgui::CursorStyle::Crosshair)
            .on_pointer(cx.pointer_listener("color-field", |this, event, cx| {
                this.picker_pointer(event, false);
                let result = this.preview_picker();
                this.result(result, cx);
            }))
    }

    fn picker_hue(&self, cx: &mut ViewContext<'_, Self>, picker: &Picker) -> Element {
        div()
            .id("color-hue")
            .w(34.)
            .h(256.)
            .flex_shrink_0()
            .relative()
            .accessibility_label("Hue")
            .accessibility_value(format!("{:.0} degrees", picker.current().hsb.hue))
            .child(
                spectrum::field(&self.color_picker_shader, 0., true, 20.)
                    .absolute()
                    .left(7.),
            )
            .child(
                div()
                    .absolute()
                    .left(7.)
                    .w(20.)
                    .h(256.)
                    .border(1., Color::rgba8(0, 0, 0, 153)),
            )
            .child(
                Icon::HueMarkers
                    .element(34.)
                    .h(10.)
                    .absolute()
                    .left(0.)
                    .top(((1. - picker.current().hsb.hue / 360.) * 256.) as f32 - 5.),
            )
            .on_pointer(cx.pointer_listener("color-hue", |this, event, cx| {
                this.picker_pointer(event, true);
                let result = this.preview_picker();
                this.result(result, cx);
            }))
    }

    fn picker_sidebar(&self, cx: &mut ViewContext<'_, Self>, picker: &Picker) -> Element {
        let buttons = div()
            .w(90.)
            .flex_col()
            .gap(8.)
            .child(
                Self::control("OK")
                    .w_full()
                    .h(30.)
                    .justify_center()
                    .bg(Color::rgb8(0, 122, 255))
                    .hover(|s| s.bg(Color::rgb8(24, 137, 255)))
                    .on_click(cx.listener("form-apply", |this, cx| {
                        let result = this.finish_color(true);
                        this.operation_result(alerts::Operation::Paint, result, cx);
                    })),
            )
            .child(
                Self::control("Cancel")
                    .w_full()
                    .h(30.)
                    .justify_center()
                    .on_click(cx.listener("form-cancel", |this, cx| {
                        let result = this.finish_color(false);
                        this.result(result, cx);
                    })),
            );
        let preview = div()
            .w(64.)
            .h(64.)
            .rounded(5.)
            .bg(color(picker.current().hsb.rgb()))
            .border(1., Color::rgba8(0, 0, 0, 153))
            .accessibility_label("New color");
        let fields = self.picker_channels(cx, picker);
        div()
            .w(180.)
            .h(256.)
            .flex_shrink_0()
            .flex_col()
            .child(div().flex_row().gap(16.).child(preview).child(buttons))
            .child(div().flex_1().min_h(12.))
            .child(fields)
            .child(
                text("Click the canvas to sample")
                    .text_size(10.)
                    .line_height(13.)
                    .text_color(Color::rgb8(170, 170, 170))
                    .mt(8.)
                    .wrap(),
            )
    }

    fn picker_channels(&self, cx: &mut ViewContext<'_, Self>, picker: &Picker) -> Element {
        let mut fields = div().flex_col().gap(6.);
        for (index, label) in ["R", "G", "B", "#"].into_iter().enumerate() {
            let value = if index < 3 {
                picker.current().rgb[index].clone()
            } else {
                picker
                    .current()
                    .hex_draft
                    .clone()
                    .unwrap_or_else(|| picker.current().hex())
            };
            let input = Self::text_field(value)
                .h(26.)
                .w(if index < 3 { 52. } else { 84. })
                .text_input_padding(6.)
                .rounded(5.)
                .text_size(13.)
                .line_height(16.)
                .bg(Color::rgb8(29, 29, 29))
                .border(1., Color::rgb8(72, 72, 72))
                .accessibility_label(["Red", "Green", "Blue", "Hex color"][index])
                .on_input(
                    cx.input_listener(51_000 + index as u64, move |this, value, cx| {
                        this.picker_input((index < 3).then_some(index), value);
                        if index < 3 {
                            let result = this.preview_picker();
                            this.result(result, cx);
                        } else {
                            cx.invalidate();
                        }
                    }),
                )
                .on_key_down(cx.key_down_listener(
                    51_000 + index as u64,
                    move |this, event, cx| {
                        if index == 3 && event.key == Key::Enter {
                            this.commit_picker_hex();
                            let result = this.preview_picker();
                            this.result(result, cx);
                            cx.prevent_default();
                            cx.stop_propagation();
                        } else if index < 3 && matches!(event.key, Key::ArrowUp | Key::ArrowDown) {
                            this.commit_picker_hex();
                            if let Some(picker) = this.picker_mut() {
                                let mut rgb = picker.current().hsb.rgb();
                                let step = if event.modifiers.contains(Modifiers::SHIFT) {
                                    10
                                } else {
                                    1
                                };
                                rgb[index] = if event.key == Key::ArrowUp {
                                    rgb[index].saturating_add(step)
                                } else {
                                    rgb[index].saturating_sub(step)
                                };
                                picker.current_mut().hsb.set_rgb(rgb);
                                picker.current_mut().sync();
                            }
                            let result = this.preview_picker();
                            this.result(result, cx);
                            cx.prevent_default();
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    },
                ));
            let input = if index == 3 {
                input.font_family(quickgui::FontFamily::Monospace)
            } else {
                input
            };
            fields = fields.child(
                div()
                    .flex_row()
                    .items_center()
                    .gap(8.)
                    .child(text(label).w(14.).text_size(13.).line_height(16.))
                    .child(input),
            );
        }
        fields
    }
}

fn color([r, g, b, _]: [u8; 4]) -> Color {
    Color::rgb8(r, g, b)
}
