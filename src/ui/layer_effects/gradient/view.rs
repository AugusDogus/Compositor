use super::*;

impl Editor {
    pub(in crate::ui::layer_effects) fn gradient_overlay_controls(
        &self,
        cx: &mut ViewContext<'_, Self>,
        edit: &EffectsEditor,
    ) -> Element {
        let Some(overlay) = &edit.effects.gradient_overlay else {
            return div();
        };
        let draft = &edit.gradient;
        let count = match draft.channel {
            Channel::Color => overlay.stops.as_slice().len(),
            Channel::Opacity => overlay.opacity_stops.as_slice().len(),
        };
        div()
            .flex_col()
            .gap(8.)
            .child(self.overlay_styles(cx, overlay))
            .child(Self::overlay_ramp((**overlay).clone()))
            .child(self.overlay_channels(cx, draft))
            .child(self.overlay_choices(cx, draft, count))
            .child(self.overlay_stop_actions(cx, count))
            .child(self.overlay_stop_fields(cx, edit))
    }
    fn overlay_styles(&self, cx: &mut ViewContext<'_, Self>, overlay: &Overlay) -> Element {
        let mut styles = div().flex_row().gap(8.);
        for (style, name) in [(Style::Linear, "Linear"), (Style::Radial, "Radial")] {
            styles = styles.child(
                self.segment(name, overlay.style == style)
                    .id(format!("overlay-style-{name}"))
                    .on_click(cx.listener(format!("overlay-style-{name}"), move |e, cx| {
                        e.change_effect(|draft| {
                            if let Some(s) = &mut draft.effects.gradient_overlay {
                                s.style = style;
                            }
                        });
                        e.changed(cx);
                    })),
            );
        }
        styles.child(
            self.control("Reverse")
                .selected(overlay.reverse)
                .id("overlay-reverse")
                .on_click(cx.listener("overlay-reverse", |e, cx| {
                    e.change_effect(|draft| {
                        if let Some(s) = &mut draft.effects.gradient_overlay {
                            s.reverse = !s.reverse;
                        }
                    });
                    e.changed(cx);
                })),
        )
    }
    fn overlay_ramp(ramp: Overlay) -> Element {
        quickgui::canvas(move |bounds, painter| {
            let width = bounds.width.ceil() as usize;
            for x in 0..width {
                let t = x as f64 / width.saturating_sub(1).max(1) as f64;
                let rgba = ramp.sample(if ramp.reverse { 1. - t } else { t });
                painter.fill_rect(
                    quickgui::Rect::new(x as f32, 0., 1., bounds.height),
                    Color::rgba8(
                        (rgba[0] * 255.).round() as u8,
                        (rgba[1] * 255.).round() as u8,
                        (rgba[2] * 255.).round() as u8,
                        (rgba[3] * 255.).round() as u8,
                    ),
                );
            }
        })
        .h(24.)
        .id("overlay-ramp")
    }
    fn overlay_channels(&self, cx: &mut ViewContext<'_, Self>, draft: &Draft) -> Element {
        let mut channels = div().flex_row().gap(8.);
        for (channel, label) in [
            (Channel::Color, "Color stops"),
            (Channel::Opacity, "Opacity stops"),
        ] {
            channels = channels.child(
                self.segment(label, draft.channel == channel)
                    .id(format!("overlay-channel-{label}"))
                    .on_click(
                        cx.listener(format!("overlay-channel-{label}"), move |e, cx| {
                            e.overlay_stop_select(channel, 0);
                            e.changed(cx);
                        }),
                    ),
            );
        }
        channels
    }
    fn overlay_choices(
        &self,
        cx: &mut ViewContext<'_, Self>,
        draft: &Draft,
        count: usize,
    ) -> Element {
        let mut choices = div()
            .flex_row()
            .flex_wrap()
            .gap(4.)
            .max_h(70.)
            .overflow_y_scroll();
        for index in 0..count {
            let channel = draft.channel;
            choices = choices.child(
                self.control(format!("{}", index + 1))
                    .selected(draft.selected == index)
                    .selected_style(|s| self.colors.accent_style(s, [0, 122, 255]))
                    .id(format!("overlay-stop-{index}"))
                    .on_click(cx.listener(format!("overlay-stop-{index}"), move |e, cx| {
                        e.overlay_stop_select(channel, index);
                        e.changed(cx);
                    })),
            );
        }
        choices
    }
    fn overlay_stop_actions(&self, cx: &mut ViewContext<'_, Self>, count: usize) -> Element {
        div()
            .flex_row()
            .gap(8.)
            .child(
                self.control("Add stop")
                    .id("overlay-stop-add")
                    .disabled(count >= 32)
                    .on_click(cx.listener("overlay-stop-add", |e, cx| {
                        e.overlay_stop_count(true);
                        e.changed(cx);
                    })),
            )
            .child(
                self.control("Remove stop")
                    .id("overlay-stop-remove")
                    .disabled(count <= 2)
                    .on_click(cx.listener("overlay-stop-remove", |e, cx| {
                        e.overlay_stop_count(false);
                        e.changed(cx);
                    })),
            )
    }
    fn overlay_stop_fields(&self, cx: &mut ViewContext<'_, Self>, edit: &EffectsEditor) -> Element {
        let draft = &edit.gradient;
        let mut content = div().flex_col().gap(8.);
        let label = if draft.channel == Channel::Color {
            "Color (hex)"
        } else {
            "Opacity (%)"
        };
        for (position, label, value, id) in [
            (
                true,
                "Position (%)",
                &draft.position,
                "overlay-stop-position",
            ),
            (false, label, &draft.value, "overlay-stop-value"),
        ] {
            content = content.child(
                div()
                    .flex_row()
                    .items_center()
                    .gap(8.)
                    .child(text(label).w(100.))
                    .child(
                        self.text_field(value.clone())
                            .id(id)
                            .w(120.)
                            .h(26.)
                            .accessibility_label(label)
                            .on_input(cx.input_listener(id, move |e, value, cx| {
                                e.overlay_stop_input(position, value);
                                e.changed(cx);
                            })),
                    ),
            );
        }
        if draft.channel == Channel::Color {
            let [r, g, b, _] = edit.overlay_color();
            content = content.child(
                self.control("Choose color…")
                    .id("overlay-color-picker")
                    .child(div().size(18., 18.).bg(Color::rgb8(r, g, b)))
                    .on_click(cx.listener("overlay-color-picker", |e, cx| {
                        e.open_effect_color_picker();
                        cx.invalidate();
                    })),
            );
        }
        content
    }
}
