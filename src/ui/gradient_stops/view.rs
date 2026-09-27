use super::*;

impl Editor {
    pub(in crate::ui) fn gradient_stops_controls(
        &self,
        cx: &mut ViewContext<'_, Self>,
        draft: &Draft,
    ) -> Element {
        let stops = self.tools.gradient.stops.as_slice();
        let ramp = self.tools.gradient.stops.clone();
        let mut track = div()
            .id("gradient-stop-track")
            .h(36.)
            .relative()
            .bg(Color::rgb8(90, 90, 90))
            .child(ramp_view(ramp, false, false).absolute().size_full())
            .on_pointer(
                cx.pointer_listener("gradient-stop-track", |this, event, cx| {
                    if event.button != MouseButton::Left
                        || !matches!(event.phase, PointerPhase::Down | PointerPhase::Move)
                        || event.size.width <= 0.
                    {
                        return;
                    }
                    let position = (event.local_position.x / event.size.width).clamp(0., 1.) as f64;
                    if !position.is_finite() {
                        return;
                    }
                    if event.phase == PointerPhase::Down {
                        let selected = this
                            .tools
                            .gradient
                            .stops
                            .as_slice()
                            .iter()
                            .enumerate()
                            .min_by(|(_, a), (_, b)| {
                                (a.position - position)
                                    .abs()
                                    .total_cmp(&(b.position - position).abs())
                            })
                            .map(|(i, _)| i);
                        if let Some(Form::GradientStops(draft)) = &mut this.modal
                            && let Some(selected) = selected
                        {
                            draft.selected = selected;
                        }
                        this.sync_gradient_stop_fields();
                    } else {
                        let result =
                            this.gradient_stop_input(true, &format!("{:.2}", position * 100.));
                        this.result(result, cx);
                    }
                    cx.invalidate();
                }),
            );
        // Canvas rendering keeps the ramp and markers correct at every sheet width.
        let positions: Vec<_> = stops.iter().map(|stop| stop.position).collect();
        let selected = draft.selected;
        track = track.child(
            quickgui::canvas(move |bounds, painter| {
                for (i, position) in positions.iter().enumerate() {
                    let x = (*position as f32 * (bounds.width - 4.)).clamp(0., bounds.width - 4.);
                    painter.fill_rect(
                        quickgui::Rect::new(x, 0., 4., bounds.height),
                        if i == selected {
                            Color::WHITE
                        } else {
                            Color::BLACK
                        },
                    );
                }
            })
            .absolute()
            .size_full(),
        );
        let mut choices = div()
            .flex_row()
            .gap(4.)
            .flex_wrap()
            .max_h(100.)
            .overflow_y_scroll();
        for (index, stop) in stops.iter().enumerate() {
            let [r, g, b, a] = stop.color;
            choices = choices.child(
                Self::control(format!("{}", index + 1))
                    .bg(if index == draft.selected {
                        Color::rgb8(0, 122, 255)
                    } else {
                        Color::rgb8(55, 55, 55)
                    })
                    .id(format!("gradient-stop-{index}"))
                    .child(div().size(12., 12.).bg(Color::rgba8(r, g, b, a)))
                    .on_click(
                        cx.listener(format!("gradient-stop-{index}"), move |this, cx| {
                            if let Some(Form::GradientStops(draft)) = &mut this.modal {
                                draft.selected = index;
                            }
                            this.sync_gradient_stop_fields();
                            cx.invalidate();
                        }),
                    ),
            );
        }
        let mut content = div().flex_col().gap(12.).child(track).child(choices).child(
            div()
                .flex_row()
                .gap(8.)
                .child(
                    Self::control("Add stop")
                        .disabled(stops.len() >= MAX_STOPS)
                        .on_click(cx.listener("gradient-stop-add", |this, cx| {
                            let result = this.add_gradient_stop();
                            this.result(result, cx);
                        })),
                )
                .child(
                    Self::control("Remove stop")
                        .disabled(stops.len() <= 2)
                        .on_click(cx.listener("gradient-stop-remove", |this, cx| {
                            let result = this.remove_gradient_stop();
                            this.result(result, cx);
                        })),
                ),
        );
        for (position, label, value, id) in [
            (
                true,
                "Position (%)",
                &draft.position,
                "gradient-stop-position",
            ),
            (
                false,
                "Opacity (%)",
                &draft.opacity,
                "gradient-stop-opacity",
            ),
        ] {
            content = content.child(
                div()
                    .flex_row()
                    .items_center()
                    .gap(12.)
                    .child(text(label).flex_1())
                    .child(
                        Self::text_field(value.clone())
                            .id(id)
                            .w(100.)
                            .h(28.)
                            .on_input(cx.input_listener(id, move |this, value, cx| {
                                let result = this.gradient_stop_input(position, value);
                                this.result(result, cx);
                            })),
                    ),
            );
        }
        let [r, g, b, a] = stops[draft.selected].color;
        content = content.child(
            Self::control("Choose color…")
                .child(div().size(18., 18.).bg(Color::rgba8(r, g, b, a)))
                .on_click(cx.listener("gradient-stop-color", |this, cx| {
                    this.open_gradient_stop_picker();
                    cx.invalidate();
                })),
        );
        if self.tools.mask_target {
            content = content.child(
                text("Colors convert to grayscale when painting a mask.")
                    .wrap()
                    .text_size(12.),
            );
        }
        if !draft.error.is_empty() {
            content = content.child(
                text(draft.error.clone())
                    .wrap()
                    .text_size(12.)
                    .text_color(Color::rgb8(255, 159, 10)),
            );
        }
        content.child(
            div()
                .flex_row()
                .gap(8.)
                .child(Self::control("Cancel").on_click(cx.listener(
                    "gradient-stops-cancel",
                    |this, cx| {
                        let result = this.finish_gradient_stops(false);
                        this.result(result, cx);
                    },
                )))
                .child(div().flex_1())
                .child(
                    Self::control("Done")
                        .disabled(!draft.error.is_empty())
                        .on_click(cx.listener("gradient-stops-apply", |this, cx| {
                            let result = this.finish_gradient_stops(true);
                            this.result(result, cx);
                        })),
                ),
        )
    }
}
