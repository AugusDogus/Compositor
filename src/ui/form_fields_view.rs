use super::scalar_controls::Scalar;
use super::*;

impl Editor {
    pub(super) fn form_input(
        &self,
        cx: &mut ViewContext<'_, Self>,
        index: usize,
        value: &str,
    ) -> Element {
        let id = 50_000 + index as u64;
        self.form_input_with_id(cx, index, value, id)
    }

    fn form_input_with_id(
        &self,
        cx: &mut ViewContext<'_, Self>,
        index: usize,
        value: &str,
        id: u64,
    ) -> Element {
        Self::text_field(value.to_owned())
            .h(26.)
            .flex_shrink_0()
            .text_input_padding(7.)
            .rounded(5.)
            .text_size(13.).line_height(16.)
            .bg(Color::rgb8(29, 29, 29))
            .border(1., Color::rgb8(72, 72, 72))
            .on_input(cx.input_listener(id, move |this, value, cx| {
                this.update_form_field(index, value);
                this.changed(cx);
            }))
            .on_key_down(cx.key_down_listener(id, move |this, event, cx| {
                if (event.modifiers == Modifiers::CONTROL
                    || event.modifiers == (Modifiers::CONTROL | Modifiers::SHIFT))
                    && matches!(&event.key, Key::Character(c) if c.eq_ignore_ascii_case("z") || c.eq_ignore_ascii_case("y"))
                {
                    // Keep native text history ahead of the enclosing panel's document commands.
                    cx.stop_propagation();
                } else if matches!(event.key, Key::ArrowUp | Key::ArrowDown)
                    && this.step_numeric_field(index, event.key == Key::ArrowUp, event.modifiers)
                {
                    cx.stop_propagation();
                    cx.prevent_default();
                    this.changed(cx);
                } else {
                    cx.propagate();
                }
            }))
    }
    /// Display the source sheet's precision without rounding the stored dimensions.
    /// Keep the complete draft while editing so partial values and the caret survive.
    pub(super) fn form_number_input(
        &self,
        cx: &mut ViewContext<'_, Self>,
        index: usize,
        value: &str,
        decimals: usize,
    ) -> Element {
        self.number_input_with_id(cx, index, value, decimals, 50_000 + index as u64)
            .text_right()
    }

    pub(super) fn size_number_input(
        &self,
        cx: &mut ViewContext<'_, Self>,
        index: usize,
        value: &str,
        decimals: usize,
    ) -> Element {
        self.number_input_with_id(cx, index, value, decimals, self.size_field_id(index))
    }

    fn number_input_with_id(
        &self,
        cx: &mut ViewContext<'_, Self>,
        index: usize,
        value: &str,
        decimals: usize,
        id: u64,
    ) -> Element {
        let focused = cx.is_focused(quickgui::FocusHandle::new(id));
        let displayed = if focused {
            value.to_owned()
        } else if let Ok(number) = value.parse::<f64>() {
            let formatted = format!("{number:.decimals$}");
            if formatted.contains('.') {
                formatted
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_owned()
            } else {
                formatted
            }
        } else {
            value.to_owned()
        };
        self.form_input_with_id(cx, index, &displayed, id)
    }
    pub(super) fn update_form_field(&mut self, index: usize, value: &str) {
        self.update_dimension(index, value);
        self.update_transform_scale(index);
        self.refresh_adjustment();
        self.refresh_filter();
        self.refresh_jpeg();
    }
    fn field_column(
        &self,
        cx: &mut ViewContext<'_, Self>,
        index: usize,
        label: &str,
        value: &str,
    ) -> Element {
        let decimals = if index == 1 { 2 } else { 0 };
        let displayed = if cx.is_focused(quickgui::FocusHandle::new(50_000 + index as u64)) {
            value.to_owned()
        } else {
            value.parse::<f64>().map_or_else(
                |_| value.to_owned(),
                |number| format!("{number:.decimals$}"),
            )
        };
        div()
            .flex_col()
            .gap(5.)
            .w(80.)
            .child(
                text(label.to_owned())
                    .text_size(10.)
                    .line_height(13.)
                    .text_color(Color::rgb8(181, 181, 181)),
            )
            .child(self.form_input(cx, index, &displayed).text_right().w_full())
    }
    fn field_choices(
        &self,
        cx: &mut ViewContext<'_, Self>,
        index: usize,
        value: &str,
        options: &[(&'static str, &'static str)],
    ) -> Element {
        let mut row = div()
            .flex_row()
            .gap(2.)
            .p(2.)
            .rounded(6.)
            .bg(Color::rgb8(30, 30, 30));
        for &(label, choice) in options {
            row = row.child(
                Self::segment(label, value == choice)
                    .h(25.)
                    .px(8.)
                    .rounded(4.)
                    .text_size(13.)
                    .line_height(16.)
                    .on_click(cx.listener(
                        format!("form-choice-{index}-{choice}"),
                        move |this, cx| {
                            this.update_form_field(index, choice);
                            this.changed(cx);
                        },
                    )),
            );
        }
        row
    }
    pub(super) fn form_fields_view(
        &self,
        cx: &mut ViewContext<'_, Self>,
        action: Action,
        fields: &[(&'static str, String)],
    ) -> Element {
        let kind = self.adjustment_edit.as_ref().map(|e| e.settings.kind);
        if kind == Some(Kind::Levels) {
            return self.levels_fields_view(cx, fields);
        }
        let mut rows = div().flex_col().gap(16.).flex_shrink_0();
        let mut order: Vec<_> = (0..fields.len()).collect();
        if matches!(
            action,
            Action::Filter(compositor::filters::Filter::Motion { .. })
        ) {
            order.swap(0, 1);
        }
        for index in order {
            let (label, value) = &fields[index];
            if matches!(kind, Some(Kind::Curves | Kind::GradientMap))
                || (kind == Some(Kind::HueSaturation) && index >= 3)
            {
                continue;
            }
            if matches!(action, Action::RemoveBackground)
                && self.background_mode == super::background_controls::Mode::Basic
            {
                continue;
            }
            if matches!(action, Action::CameraRaw)
                && let Some(control) = self.camera_parameter_control(cx, index, value)
            {
                rows = rows.child(control);
                continue;
            }
            if let Some(control) = self.parameter_control(cx, action, index, value) {
                rows = rows.child(control);
                continue;
            }
            if kind == Some(Kind::Grain) && index == 3 {
                continue;
            }
            rows = rows.child(self.form_field_view(cx, action, kind, index, label, value));
        }
        rows
    }

    // Build each field family separately so inactive controls do not reserve
    // large Element temporaries on the frame constructing a parameter slider.
    fn levels_fields_view(
        &self,
        cx: &mut ViewContext<'_, Self>,
        fields: &[(&'static str, String)],
    ) -> Element {
        let mut rows = div().flex_col().gap(16.).flex_shrink_0();
        for (start, end) in [(0, 3), (3, 5)] {
            if start == 3 {
                rows = rows.child(
                    div()
                        .flex_col()
                        .gap(0.)
                        .child(div().id("levels-output-ramp").h(14.).bg_linear_gradient(
                            quickgui::GradientDirection::ToRight,
                            [Color::BLACK, Color::WHITE],
                        ))
                        .child(self.levels_handles(cx, true)),
                );
            }
            let mut row = div().flex_row().justify_between();
            for (index, (label, value)) in fields.iter().enumerate().take(end).skip(start) {
                row = row.child(self.field_column(cx, index, label, value));
            }
            rows = rows.child(row);
        }
        rows
    }

    fn form_field_view(
        &self,
        cx: &mut ViewContext<'_, Self>,
        action: Action,
        kind: Option<Kind>,
        index: usize,
        label: &'static str,
        value: &str,
    ) -> Element {
        if matches!(
            action,
            Action::Filter(compositor::filters::Filter::Noise { .. })
        ) && index == 1
        {
            return div()
                .flex_row()
                .items_center()
                .gap(10.)
                .child(text("Distribution").text_size(13.).line_height(16.))
                .child(self.field_choices(
                    cx,
                    index,
                    value,
                    &[("Uniform", "0"), ("Gaussian", "1")],
                ));
        }
        if label.ends_with("(0 or 1)") {
            let label = label.trim_end_matches(" (0 or 1)");
            return self.form_toggle(cx, index, label, value == "1");
        }
        let options: &[(&str, &str)] = if label.starts_with("Sampling (") {
            &[
                ("High Quality", "high"),
                ("Smooth", "smooth"),
                ("Nearest", "nearest"),
            ]
        } else if label.starts_with("Kind (") {
            &[("Rectangle", "rectangle"), ("Ellipse", "ellipse")]
        } else if label.starts_with("Sample width (") {
            &[("1 × 1", "1"), ("3 × 3", "3"), ("5 × 5", "5")]
        } else {
            &[]
        };
        let short_label = label.split(" (").next().unwrap_or(label);
        if !options.is_empty() {
            return div()
                .flex_col()
                .gap(5.)
                .child(text(short_label).text_size(13.).line_height(16.))
                .child(self.field_choices(cx, index, value, options));
        }
        let range = match (kind, index) {
            (Some(Kind::HueSaturation), 0) => Some(
                if self
                    .adjustment_edit
                    .as_ref()
                    .and_then(|e| e.settings.hsv_settings.as_ref())
                    .is_some_and(|s| s.colorize)
                {
                    (0., 360.)
                } else {
                    (-180., 180.)
                },
            ),
            (Some(Kind::HueSaturation), 1) => Some(
                if self
                    .adjustment_edit
                    .as_ref()
                    .and_then(|e| e.settings.hsv_settings.as_ref())
                    .is_some_and(|s| s.colorize)
                {
                    (0., 100.)
                } else {
                    (-100., 100.)
                },
            ),
            (Some(Kind::HueSaturation), 2) => Some((-100., 100.)),
            _ => None,
        };
        if let Some(range) = range {
            let slider_id = [
                "adjustment-value-0",
                "adjustment-value-1",
                "adjustment-value-2",
            ][index];
            div()
                .flex_row()
                .gap(10.)
                .items_center()
                .child(text(short_label).text_size(13.).line_height(16.).w(76.))
                .child(self.scalar_slider(
                    cx,
                    slider_id,
                    short_label,
                    Scalar::Field(index),
                    range,
                    250.,
                ))
                .child(
                    div()
                        .flex_row()
                        .items_center()
                        .flex_shrink_0()
                        .gap(2.)
                        .child(self.form_number_input(cx, index, value, 0).w(48.))
                        .child(
                            text(if index == 0 { "°" } else { "" })
                                .w(14.)
                                .text_size(13.)
                                .line_height(16.),
                        ),
                )
        } else {
            div()
                .flex_row()
                .gap(10.)
                .items_center()
                .child(text(short_label).text_size(13.).line_height(16.).w(125.))
                .child(self.form_input(cx, index, value).flex_1().min_w(0.))
        }
    }

    pub(super) fn form_toggle(
        &self,
        cx: &mut ViewContext<'_, Self>,
        index: usize,
        label: &'static str,
        checked: bool,
    ) -> Element {
        Self::check_control(label, checked)
            .text_size(13.)
            .line_height(16.)
            .on_click(cx.listener(50_000 + index as u64, move |this, cx| {
                let checked = matches!(&this.modal, Some(Form::Edit { fields, .. })
                    if fields.get(index).is_some_and(|f| f.1 == "1"));
                this.update_form_field(index, if checked { "0" } else { "1" });
                this.changed(cx);
            }))
    }
}
