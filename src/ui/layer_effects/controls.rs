use super::*;
impl Editor {
    pub(in crate::ui) fn effects_controls(
        &self,
        cx: &mut ViewContext<'_, Self>,
        edit: &EffectsEditor,
    ) -> Element {
        let mut tabs = div().flex_row().gap(4.);
        for kind in EffectKind::ALL {
            tabs = tabs.child(Self::segment(kind.label(), edit.kind == kind).on_click(
                cx.listener(format!("effect-tab-{}", kind.label()), move |this, cx| {
                    if let Some(Form::Effects(e)) = &mut this.modal {
                        e.kind = kind;
                        e.color_text();
                    }
                    cx.invalidate();
                }),
            ));
        }
        let mut content = div().flex_col().gap(14.).child(tabs);
        let kind = edit.kind;
        let enabled = kind.enabled(&edit.effects);
        let mut actions = div().flex_row().gap(8.).child(
            Self::control(if enabled.is_none() {
                "Add effect"
            } else if enabled == Some(true) {
                "Hide effect"
            } else {
                "Show effect"
            })
            .id("effect-toggle")
            .on_click(cx.listener("effect-toggle", move |this, cx| {
                let color = this.tools.background.map(|c| c as f64 / 255.);
                this.change_effect(|e| {
                    let visible = Some(!kind.enabled(&e.effects).unwrap_or(false));
                    match kind {
                        EffectKind::Stroke => {
                            e.effects
                                .stroke
                                .get_or_insert_with(|| StrokeEffect {
                                    red: color[0],
                                    green: color[1],
                                    blue: color[2],
                                    ..Default::default()
                                })
                                .enabled = visible
                        }
                        EffectKind::Shadow => {
                            e.effects.shadow.get_or_insert_default().enabled = visible
                        }
                        EffectKind::Overlay => {
                            e.effects
                                .color_overlay
                                .get_or_insert_with(|| ColorOverlayEffect {
                                    red: color[0],
                                    green: color[1],
                                    blue: color[2],
                                    ..Default::default()
                                })
                                .enabled = visible
                        }
                        EffectKind::Inner => {
                            e.effects
                                .inner_shadow
                                .get_or_insert_with(ShadowEffect::inner_default)
                                .enabled = visible
                        }
                    }
                    e.color_text();
                });
                this.changed(cx);
            })),
        );
        if enabled.is_some() {
            actions = actions.child(Self::control("Remove").id("effect-remove").on_click(
                cx.listener("effect-remove", move |this, cx| {
                    this.change_effect(|e| match kind {
                        EffectKind::Stroke => e.effects.stroke = None,
                        EffectKind::Shadow => e.effects.shadow = None,
                        EffectKind::Overlay => e.effects.color_overlay = None,
                        EffectKind::Inner => e.effects.inner_shadow = None,
                    });
                    this.changed(cx);
                }),
            ));
        }
        content = content.child(actions);
        if enabled.is_some() {
            let rgb = kind.color(&edit.effects).map(|c| (c * 255.).round() as u8);
            content = content.child(
                div()
                    .flex_row()
                    .items_center()
                    .gap(12.)
                    .child(text("Color").w(100.))
                    .child(
                        button()
                            .id("effect-color-picker")
                            .on_click(cx.listener("effect-color-picker", |this, cx| {
                                this.open_effect_color_picker();
                                cx.invalidate();
                            }))
                            .accessibility_label("Choose effect color")
                            .size(30., 22.)
                            .rounded(3.)
                            .bg(Color::rgb8(rgb[0], rgb[1], rgb[2]))
                            .border(1., Color::WHITE),
                    )
                    .child(
                        Self::text_field(edit.color.clone())
                            .id("effect-color")
                            .w(100.)
                            .h(26.)
                            .accessibility_label("Effect color hex RGB")
                            .on_input(cx.input_listener("effect-color", |this, value, cx| {
                                let value = value.to_owned();
                                this.change_effect(|e| {
                                    if let Ok(rgb) = compositor::palette::parse_hex(&value) {
                                        e.set_color(rgb);
                                    }
                                    e.color = value;
                                });
                                this.changed(cx);
                            })),
                    ),
            );
            let parameters = match kind {
                EffectKind::Stroke => vec![Parameter::Size, Parameter::Opacity],
                EffectKind::Overlay => vec![Parameter::Opacity],
                _ => vec![
                    Parameter::Opacity,
                    Parameter::Angle,
                    Parameter::Distance,
                    Parameter::Blur,
                ],
            };
            for parameter in parameters {
                content = content.child(self.effect_number(cx, edit, parameter));
            }
            if kind == EffectKind::Stroke {
                let inside = edit.effects.stroke.as_ref().is_some_and(|s| s.inside);
                content = content.child(
                    Self::control(if inside {
                        "Position: Inside"
                    } else {
                        "Position: Outside"
                    })
                    .id("effect-stroke-position")
                    .on_click(cx.listener(
                        "effect-stroke-position",
                        |this, cx| {
                            this.change_effect(|e| {
                                if let Some(s) = &mut e.effects.stroke {
                                    s.inside = !s.inside;
                                }
                            });
                            this.changed(cx);
                        },
                    )),
                );
            }
        }
        if enabled.is_some() {
            content = content.child(
                Self::control("Copy this effect to…")
                    .id("effect-copy")
                    .on_click(cx.listener("effect-copy", |this, cx| {
                        if let Some(Form::Effects(edit)) = &mut this.modal {
                            edit.copy_to = !edit.copy_to;
                        }
                        cx.invalidate();
                    })),
            );
            if edit.copy_to {
                let mut destinations = div().flex_col().gap(3.).max_h(140.).overflow_y_scroll();
                for layer in self
                    .session()
                    .document
                    .layers
                    .iter()
                    .filter(|l| l.id != edit.id && l.raster().is_some())
                {
                    let id = layer.id;
                    destinations = destinations.child(Self::control(layer.name.clone()).on_click(
                        cx.listener(format!("effect-copy-{id}"), move |this, cx| {
                            let result = this.copy_effect_to(id);
                            this.result(result, cx);
                        }),
                    ));
                }
                content = content.child(destinations);
            }
        }
        if !edit.error.is_empty() {
            content = content.child(
                text(edit.error.clone())
                    .wrap()
                    .text_color(Color::rgb8(255, 159, 10)),
            );
        }
        content.child(
            div()
                .flex_row()
                .justify_end()
                .gap(8.)
                .child(
                    Self::control("Cancel")
                        .id("effects-cancel")
                        .on_click(cx.listener("effects-cancel", |this, cx| {
                            let r = this.finish_effects(false);
                            this.result(r, cx);
                        })),
                )
                .child(
                    Self::control("OK")
                        .id("effects-apply")
                        .disabled(!edit.error.is_empty())
                        .on_click(cx.listener("effects-apply", |this, cx| {
                            let r = this.finish_effects(true);
                            this.result(r, cx);
                        })),
                ),
        )
    }
    fn effect_number(
        &self,
        cx: &mut ViewContext<'_, Self>,
        edit: &EffectsEditor,
        parameter: Parameter,
    ) -> Element {
        let value = edit.number(parameter);
        let range = parameter.range();
        let label = parameter.label();
        div()
            .flex_row()
            .items_center()
            .gap(10.)
            .child(text(label).w(100.).text_size(12.))
            .child(self.scalar_slider(
                cx,
                match parameter {
                    Parameter::Opacity => "effect-opacity",
                    Parameter::Size => "effect-size",
                    Parameter::Angle => "effect-angle",
                    Parameter::Distance => "effect-distance",
                    Parameter::Blur => "effect-blur",
                },
                label,
                super::scalar_controls::Scalar::Effect(parameter),
                range,
                165.,
            ))
            .child(
                Self::text_field(format!("{value:.1}"))
                    .w(65.)
                    .h(26.)
                    .accessibility_label(label)
                    .on_input(cx.input_listener(
                        format!("effect-number-{label}"),
                        move |this, value, cx| {
                            if let Ok(number) = value.parse::<f64>()
                                && number.is_finite()
                            {
                                this.change_effect(|e| {
                                    e.set_number(parameter, number.clamp(range.0, range.1))
                                });
                                this.changed(cx);
                            }
                        },
                    )),
            )
    }
}
