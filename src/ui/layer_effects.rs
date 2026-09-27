//! Nondestructive layer effects with one undo entry per confirmed editing session.
mod bevel;
pub(super) mod blend_if;
mod buttons;
mod controls;
mod gradient;
pub(super) mod pattern;
#[cfg(test)]
mod tests;
use super::*;
use compositor::effects::{ColorOverlayEffect, LayerEffects, ShadowEffect, StrokeEffect};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum EffectKind {
    Stroke,
    Shadow,
    Overlay,
    Inner,
    Glow,
    InnerGlow,
    Pattern,
    Gradient,
    Bevel,
    BlendIf,
}
impl EffectKind {
    const ALL: [Self; 10] = [
        Self::Stroke,
        Self::Shadow,
        Self::Overlay,
        Self::Inner,
        Self::Glow,
        Self::InnerGlow,
        Self::Pattern,
        Self::Gradient,
        Self::Bevel,
        Self::BlendIf,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::BlendIf => "Blend If",
            Self::Bevel => "Bevel/Emboss",
            Self::Gradient => "Gradient Overlay",
            Self::Pattern => "Pattern Overlay",
            Self::Stroke => "Stroke",
            Self::Shadow => "Drop Shadow",
            Self::Overlay => "Color Overlay",
            Self::Inner => "Inner Shadow",
            Self::Glow => "Outer Glow",
            Self::InnerGlow => "Inner Glow",
        }
    }
    fn enabled(self, edit: &EffectsEditor) -> Option<bool> {
        let e = &edit.effects;
        match self {
            Self::BlendIf => edit.blend_if.map(|s| s.enabled),
            Self::Bevel => e.bevel.as_ref().map(|s| s.enabled),
            Self::Gradient => e.gradient_overlay.as_ref().map(|s| s.enabled),
            Self::Pattern => e.pattern_overlay.as_ref().map(|s| s.settings.enabled),
            Self::Stroke => e.stroke.as_ref().map(|s| s.enabled != Some(false)),
            Self::Shadow => e.shadow.as_ref().map(|s| s.enabled != Some(false)),
            Self::Overlay => e.color_overlay.as_ref().map(|s| s.enabled != Some(false)),
            Self::Inner => e.inner_shadow.as_ref().map(|s| s.enabled != Some(false)),
            Self::Glow => e.outer_glow.as_ref().map(|s| s.enabled != Some(false)),
            Self::InnerGlow => e.inner_glow.as_ref().map(|s| s.enabled != Some(false)),
        }
    }
    fn color(self, e: &LayerEffects) -> [f64; 3] {
        match self {
            Self::Pattern | Self::Gradient | Self::Bevel | Self::BlendIf => None,
            Self::Stroke => e.stroke.as_ref().map(|s| [s.red, s.green, s.blue]),
            Self::Shadow => e.shadow.as_ref().map(|s| [s.red, s.green, s.blue]),
            Self::Overlay => e.color_overlay.as_ref().map(|s| [s.red, s.green, s.blue]),
            Self::Inner => e.inner_shadow.as_ref().map(|s| [s.red, s.green, s.blue]),
            Self::Glow => e.outer_glow.as_ref().map(|s| [s.red, s.green, s.blue]),
            Self::InnerGlow => e.inner_glow.as_ref().map(|s| [s.red, s.green, s.blue]),
        }
        .unwrap_or([0.; 3])
    }
}
#[derive(Clone)]
pub(super) struct EffectsEditor {
    id: Uuid,
    kind: EffectKind,
    effects: LayerEffects,
    blend_if: Option<compositor::blend_if::Settings>,
    color: String,
    error: String,
    copy_to: bool,
    patterns: pattern::Choices,
    gradient: gradient::Draft,
}
#[derive(Clone, Copy)]
pub(super) enum Parameter {
    Opacity,
    Size,
    Angle,
    Distance,
    Blur,
    Scale,
    BevelSize,
    Depth,
    Altitude,
    Highlight,
    Shading,
}
impl Parameter {
    fn label(self) -> &'static str {
        match self {
            Self::BevelSize => "Size (px)",
            Self::Depth => "Depth (%)",
            Self::Altitude => "Altitude (°)",
            Self::Highlight => "Highlight (%)",
            Self::Shading => "Shadow (%)",
            Self::Scale => "Scale (%)",
            Self::Opacity => "Opacity (%)",
            Self::Size => "Size (px)",
            Self::Angle => "Angle (°)",
            Self::Distance => "Distance (px)",
            Self::Blur => "Blur (px)",
        }
    }
    fn range(self) -> (f64, f64) {
        match self {
            Self::BevelSize => (0., 250.),
            Self::Depth => (1., 1000.),
            Self::Altitude => (0., 90.),
            Self::Highlight | Self::Shading => (0., 100.),
            Self::Scale => (5., 2000.),
            Self::Opacity => (0., 100.),
            Self::Size | Self::Blur => (0., 500.),
            Self::Angle => (-360., 360.),
            Self::Distance => (0., 5000.),
        }
    }
}
impl EffectsEditor {
    fn color_text(&mut self) {
        let c = self
            .kind
            .color(&self.effects)
            .map(|v| (v * 255.).round() as u8);
        self.color = format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
    }
    pub(super) fn set_color(&mut self, rgb: [u8; 4]) {
        let [r, g, b, _] = rgb.map(|v| f64::from(v) / 255.);
        match self.kind {
            EffectKind::Pattern | EffectKind::Bevel | EffectKind::BlendIf => {}
            EffectKind::Gradient => self.set_overlay_color(rgb),
            EffectKind::Stroke => {
                if let Some(s) = &mut self.effects.stroke {
                    (s.red, s.green, s.blue) = (r, g, b);
                }
            }
            EffectKind::Shadow => {
                if let Some(s) = &mut self.effects.shadow {
                    (s.red, s.green, s.blue) = (r, g, b);
                }
            }
            EffectKind::Overlay => {
                if let Some(s) = &mut self.effects.color_overlay {
                    (s.red, s.green, s.blue) = (r, g, b);
                }
            }
            EffectKind::InnerGlow => {
                if let Some(s) = &mut self.effects.inner_glow {
                    (s.red, s.green, s.blue) = (r, g, b);
                }
            }
            EffectKind::Glow => {
                if let Some(s) = &mut self.effects.outer_glow {
                    (s.red, s.green, s.blue) = (r, g, b);
                }
            }
            EffectKind::Inner => {
                if let Some(s) = &mut self.effects.inner_shadow {
                    (s.red, s.green, s.blue) = (r, g, b);
                }
            }
        }
        self.color_text();
    }
    pub(super) fn rgb(&self) -> [u8; 4] {
        if self.kind == EffectKind::Gradient {
            return self.overlay_color();
        }
        let [r, g, b] = self
            .kind
            .color(&self.effects)
            .map(|v| (v * 255.).round() as u8);
        [r, g, b, 255]
    }
    pub(super) fn number(&self, p: Parameter) -> f64 {
        match self.kind {
            EffectKind::BlendIf => 0.,
            EffectKind::Bevel => self.bevel_number(p),
            EffectKind::Gradient => {
                self.effects
                    .gradient_overlay
                    .as_ref()
                    .map_or(0., |s| match p {
                        Parameter::Angle => s.angle,
                        Parameter::Opacity => s.opacity * 100.,
                        _ => 0.,
                    })
            }
            EffectKind::Pattern => self
                .effects
                .pattern_overlay
                .as_ref()
                .map_or(0., |s| match p {
                    Parameter::Scale => s.settings.scale * 100.,
                    Parameter::Opacity => s.settings.opacity * 100.,
                    _ => 0.,
                }),
            EffectKind::Stroke => self.effects.stroke.as_ref().map_or(0., |s| match p {
                Parameter::Opacity => s.opacity * 100.,
                Parameter::Size => s.size,
                _ => 0.,
            }),
            EffectKind::InnerGlow => self.effects.inner_glow.as_ref().map_or(0., |s| match p {
                Parameter::Opacity => s.opacity * 100.,
                Parameter::Size => s.size,
                _ => 0.,
            }),

            EffectKind::Glow => self.effects.outer_glow.as_ref().map_or(0., |s| match p {
                Parameter::Opacity => s.opacity * 100.,
                Parameter::Size => s.size,
                _ => 0.,
            }),
            EffectKind::Overlay => self
                .effects
                .color_overlay
                .as_ref()
                .map_or(0., |s| s.opacity * 100.),
            kind => {
                let s = if kind == EffectKind::Shadow {
                    &self.effects.shadow
                } else {
                    &self.effects.inner_shadow
                };
                s.as_ref().map_or(0., |s| match p {
                    Parameter::Opacity => s.opacity * 100.,
                    Parameter::Angle => s.angle,
                    Parameter::Distance => s.distance,
                    Parameter::Blur => s.blur,
                    _ => 0.,
                })
            }
        }
    }
    pub(super) fn set_number(&mut self, p: Parameter, value: f64) {
        match self.kind {
            EffectKind::BlendIf => {}
            EffectKind::Bevel => self.set_bevel_number(p, value),
            EffectKind::Gradient => {
                if let Some(s) = &mut self.effects.gradient_overlay {
                    match p {
                        Parameter::Angle => s.angle = value,
                        Parameter::Opacity => s.opacity = value / 100.,
                        _ => {}
                    }
                }
            }
            EffectKind::Pattern => {
                if let Some(s) = &mut self.effects.pattern_overlay {
                    match p {
                        Parameter::Scale => s.settings.scale = value / 100.,
                        Parameter::Opacity => s.settings.opacity = value / 100.,
                        _ => {}
                    }
                }
            }
            EffectKind::Stroke => {
                if let Some(s) = &mut self.effects.stroke {
                    match p {
                        Parameter::Opacity => s.opacity = value / 100.,
                        Parameter::Size => s.size = value,
                        _ => {}
                    }
                }
            }
            EffectKind::InnerGlow => {
                if let Some(s) = &mut self.effects.inner_glow {
                    match p {
                        Parameter::Opacity => s.opacity = value / 100.,
                        Parameter::Size => s.size = value,
                        _ => {}
                    }
                }
            }
            EffectKind::Glow => {
                if let Some(s) = &mut self.effects.outer_glow {
                    match p {
                        Parameter::Opacity => s.opacity = value / 100.,
                        Parameter::Size => s.size = value,
                        _ => {}
                    }
                }
            }
            EffectKind::Overlay => {
                if let Some(s) = &mut self.effects.color_overlay {
                    s.opacity = value / 100.;
                }
            }
            kind => {
                let s = if kind == EffectKind::Shadow {
                    &mut self.effects.shadow
                } else {
                    &mut self.effects.inner_shadow
                };
                if let Some(s) = s {
                    match p {
                        Parameter::Opacity => s.opacity = value / 100.,
                        Parameter::Angle => s.angle = value,
                        Parameter::Distance => s.distance = value,
                        Parameter::Blur => s.blur = value,
                        _ => {}
                    }
                }
            }
        }
    }
}
impl Editor {
    pub(super) fn effects_button(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        self.control("fx")
            .id("layer-effects")
            .w(33.)
            .h(41.)
            .p(0.)
            .flex_shrink_0()
            .justify_center()
            .rounded(6.)
            .bg(Color::TRANSPARENT)
            .text_color(self.colors.neutral(164))
            .hover(|s| s.bg(self.colors.neutral(66)))
            .disabled_style(|s| s.opacity(0.4))
            .accessibility_label("Edit layer effects")
            .tooltip("Edit layer effects")
            .disabled(
                !self.can_edit_layers()
                    || self
                        .current_document()
                        .and_then(Document::active_layer)
                        .and_then(|l| l.raster())
                        .is_none(),
            )
            .on_click(cx.listener("layer-effects", |this, cx| {
                let r = this.open_effects();
                this.result(r, cx);
            }))
    }
    pub(super) fn open_effects(&mut self) -> Result<()> {
        if !self.can_edit_layers() {
            return Ok(());
        }
        let Some(layer) = self
            .current_document()
            .and_then(Document::active_layer)
            .filter(|l| l.raster().is_some())
        else {
            return Ok(());
        };
        let mut edit = EffectsEditor {
            id: layer.id,
            kind: EffectKind::Stroke,
            effects: layer.effects.clone().unwrap_or_default(),
            blend_if: layer.blend_if,
            color: String::new(),
            error: String::new(),
            copy_to: false,
            patterns: pattern::Choices::default(),
            gradient: gradient::Draft::default(),
        };
        edit.kind = EffectKind::ALL
            .into_iter()
            .find(|k| k.enabled(&edit).is_some())
            .unwrap_or(EffectKind::Stroke);
        edit.color_text();
        edit.sync_overlay_stop();
        self.session_mut().begin("Layer Effects")?;
        self.modal = Some(Form::Effects(Box::new(edit)));
        Ok(())
    }
    pub(super) fn change_effect(&mut self, change: impl FnOnce(&mut EffectsEditor)) {
        if self.pending {
            return;
        }
        let Some(Form::Effects(edit)) = &mut self.modal else {
            return;
        };
        change(edit);
        if !matches!(
            edit.kind,
            EffectKind::Pattern | EffectKind::Gradient | EffectKind::Bevel | EffectKind::BlendIf
        ) && edit.kind.enabled(edit).is_some()
            && compositor::palette::parse_hex(&edit.color).is_err()
        {
            edit.error = "Enter an RGB color such as #336699.".into();
            return;
        }
        if edit.effects.gradient_overlay.is_some() && !edit.gradient.error.is_empty() {
            edit.error.clone_from(&edit.gradient.error);
            return;
        }
        let (id, effects, blend_if) = (edit.id, edit.effects.clone(), edit.blend_if);
        let mut document = self.session().document.clone();
        let Some(layer) = document.layers.iter_mut().find(|l| l.id == id) else {
            return;
        };
        layer.effects = (!effects.is_empty()).then_some(effects);
        layer.blend_if = blend_if;
        match document.validate() {
            Ok(()) => {
                self.session_mut().document = document;
                if let Some(Form::Effects(e)) = &mut self.modal {
                    e.error.clear();
                }
            }
            Err(error) => {
                if let Some(Form::Effects(e)) = &mut self.modal {
                    e.error = error.to_string();
                }
            }
        }
    }
    fn copy_effect_to(&mut self, target: Uuid) -> Result<()> {
        let Some(Form::Effects(edit)) = &self.modal else {
            return Ok(());
        };
        let kind = edit.kind;
        let source = edit.effects.clone();
        let blend_if = edit.blend_if;
        let mut document = self.session().document.clone();
        let layer = document
            .layers
            .iter_mut()
            .find(|l| l.id == target && l.raster().is_some())
            .ok_or_else(|| {
                compositor::invalid("The destination layer no longer has editable pixels.")
            })?;
        let mut effects = layer.effects.clone().unwrap_or_default();
        match kind {
            EffectKind::BlendIf => layer.blend_if = blend_if,
            EffectKind::Bevel => effects.bevel = source.bevel,
            EffectKind::Gradient => effects.gradient_overlay = source.gradient_overlay,
            EffectKind::Pattern => effects.pattern_overlay = source.pattern_overlay,
            EffectKind::Stroke => effects.stroke = source.stroke,
            EffectKind::Shadow => effects.shadow = source.shadow,
            EffectKind::Overlay => effects.color_overlay = source.color_overlay,
            EffectKind::Inner => effects.inner_shadow = source.inner_shadow,
            EffectKind::Glow => effects.outer_glow = source.outer_glow,
            EffectKind::InnerGlow => effects.inner_glow = source.inner_glow,
        }
        layer.effects = (!effects.is_empty()).then_some(effects);
        document.validate()?;
        self.session_mut().document = document;
        if let Some(Form::Effects(edit)) = &mut self.modal {
            edit.copy_to = false;
        }
        Ok(())
    }
    pub(super) fn finish_effects(&mut self, commit: bool) -> Result<()> {
        if !matches!(self.modal, Some(Form::Effects(_))) {
            return Ok(());
        }
        if commit {
            if let Some(Form::Effects(e)) = &self.modal
                && !e.error.is_empty()
            {
                return Err(compositor::invalid(e.error.clone()));
            }
            compositor::project::validate_storage_metadata(&self.session().document)?;
            self.session_mut().commit()?;
        } else {
            self.session_mut().cancel();
        }
        self.modal = None;
        Ok(())
    }
}
