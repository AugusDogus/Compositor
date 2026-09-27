use super::*;
use compositor::bevel::Style;

impl Editor {
    pub(super) fn bevel_controls(
        &self,
        cx: &mut ViewContext<'_, Self>,
        edit: &EffectsEditor,
    ) -> Element {
        let Some(bevel) = &edit.effects.bevel else {
            return div();
        };
        let mut choices = div().flex_row().gap(6.);
        for (style, label) in [
            (Style::Inner, "Inner Bevel"),
            (Style::Outer, "Outer Bevel"),
            (Style::Emboss, "Emboss"),
        ] {
            choices = choices.child(
                self.segment(label, bevel.style == style)
                    .id(format!("bevel-style-{label}"))
                    .on_click(cx.listener(format!("bevel-style-{label}"), move |e, cx| {
                        e.change_effect(|edit| {
                            if let Some(s) = &mut edit.effects.bevel {
                                s.style = style;
                            }
                        });
                        e.changed(cx);
                    })),
            );
        }
        choices
    }
}
impl EffectsEditor {
    pub(super) fn bevel_number(&self, p: Parameter) -> f64 {
        self.effects.bevel.as_ref().map_or(0., |s| match p {
            Parameter::BevelSize => s.size,
            Parameter::Angle => s.angle,
            Parameter::Depth => s.depth,
            Parameter::Altitude => s.altitude,
            Parameter::Highlight => s.highlight_opacity * 100.,
            Parameter::Shading => s.shadow_opacity * 100.,
            _ => 0.,
        })
    }
    pub(super) fn set_bevel_number(&mut self, p: Parameter, value: f64) {
        let Some(s) = &mut self.effects.bevel else {
            return;
        };
        match p {
            Parameter::BevelSize => s.size = value,
            Parameter::Angle => s.angle = value,
            Parameter::Depth => s.depth = value,
            Parameter::Altitude => s.altitude = value,
            Parameter::Highlight => s.highlight_opacity = value / 100.,
            Parameter::Shading => s.shadow_opacity = value / 100.,
            _ => {}
        }
    }
}
#[cfg(test)]
mod tests;
