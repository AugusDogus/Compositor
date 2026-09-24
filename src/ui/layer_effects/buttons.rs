use super::*;
impl Editor {
    pub(super) fn effect_actions(
        &self,
        cx: &mut ViewContext<'_, Self>,
        edit: &EffectsEditor,
    ) -> Element {
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
                        EffectKind::Glow => {
                            e.effects.outer_glow.get_or_insert_default().enabled = visible
                        }
                        EffectKind::InnerGlow => {
                            e.effects.inner_glow.get_or_insert_default().enabled = visible
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
                        EffectKind::Glow => e.effects.outer_glow = None,
                        EffectKind::InnerGlow => e.effects.inner_glow = None,
                    });
                    this.changed(cx);
                }),
            ));
        }
        actions
    }
    pub(super) fn effect_color_controls(
        &self,
        cx: &mut ViewContext<'_, Self>,
        edit: &EffectsEditor,
    ) -> Element {
        let kind = edit.kind;
        let rgb = kind.color(&edit.effects).map(|c| (c * 255.).round() as u8);
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
            )
    }
    pub(super) fn effect_footer(
        &self,
        cx: &mut ViewContext<'_, Self>,
        edit: &EffectsEditor,
    ) -> Element {
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
            )
    }
}
