use super::*;
impl Editor {
    pub(in crate::ui) fn effects_controls(
        &self,
        cx: &mut ViewContext<'_, Self>,
        edit: &EffectsEditor,
    ) -> Element {
        let mut tabs = div().flex_row().flex_wrap().gap(4.);
        for kind in EffectKind::ALL {
            tabs = tabs.child(
                Self::segment(kind.label(), edit.kind == kind)
                    .id(format!("effect-tab-{}", kind.label()))
                    .on_click(cx.listener(
                        format!("effect-tab-{}", kind.label()),
                        move |this, cx| {
                            if let Some(Form::Effects(e)) = &mut this.modal {
                                e.kind = kind;
                                e.color_text();
                            }
                            cx.invalidate();
                        },
                    )),
            );
        }
        let mut content = div().flex_col().gap(14.).child(tabs);
        let kind = edit.kind;
        let enabled = kind.enabled(&edit.effects);
        content = content.child(self.effect_actions(cx, edit));
        if enabled.is_some() {
            content = content.child(self.effect_color_controls(cx, edit));
            let parameters = match kind {
                EffectKind::Stroke | EffectKind::Glow | EffectKind::InnerGlow => {
                    vec![Parameter::Size, Parameter::Opacity]
                }
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
        content.child(self.effect_footer(cx, edit))
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
