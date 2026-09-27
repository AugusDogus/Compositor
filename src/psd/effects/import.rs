//! Accept only the solid, Normal-blend styles represented by our effect model.
use super::{Mapping, ps};
use crate::effects::{
    ColorOverlayEffect, InnerGlowEffect, LayerEffects, OuterGlowEffect, ShadowEffect, StrokeEffect,
};

fn one<T>(values: Option<&Vec<T>>) -> Mapping<Option<&T>> {
    match values.map(Vec::as_slice).unwrap_or_default() {
        [] => Ok(None),
        [value] => Ok(Some(value)),
        _ => Err("multiple instances of one style are unsupported"),
    }
}
fn normal(mode: Option<ps::BlendMode>) -> Mapping<()> {
    if mode != Some(ps::BlendMode::Normal) {
        return Err("effect blend modes other than Normal are unsupported");
    }
    Ok(())
}
fn size(value: Option<ps::UnitsValue>, scale: f64) -> Mapping<f64> {
    match value {
        Some(v) if v.units == ps::Units::Pixels && v.value.is_finite() => Ok(v.value * scale),
        _ => Err("effect dimensions must be explicit pixel values"),
    }
}
fn rgb(value: Option<ps::Color>) -> Mapping<[f64; 3]> {
    match value {
        Some(ps::Color::Rgb(c)) => Ok([c.r / 255., c.g / 255., c.b / 255.]),
        Some(ps::Color::Frgb(c)) => Ok([c.fr, c.fg, c.fb]),
        Some(ps::Color::Grayscale(c)) => Ok([c.k / 255.; 3]),
        _ => Err("only RGB or grayscale effect colors are supported"),
    }
}
fn contour(value: Option<&ps::EffectContour>) -> Mapping<()> {
    if value.is_some_and(|c| {
        !(c.curve.is_empty()
            || c.curve.len() == 2
                && c.curve[0].x == 0.
                && c.curve[0].y == 0.
                && c.curve[1].x == 255.
                && c.curve[1].y == 255.)
    }) {
        return Err("nonlinear effect contours are unsupported");
    }
    Ok(())
}
fn zero_choke(value: Option<ps::UnitsValue>) -> Mapping<()> {
    if value.is_some_and(|v| v.value != 0.) {
        return Err("effect spread/choke is unsupported");
    }
    Ok(())
}
fn shadow(s: &ps::LayerEffectShadow, scale: f64, master: bool) -> Mapping<ShadowEffect> {
    normal(s.blend_mode)?;
    contour(s.contour.as_ref())?;
    zero_choke(s.choke)?;
    if s.use_global_light != Some(false) || s.layer_conceals == Some(true) {
        return Err("global-light or knockout shadow behavior is unsupported");
    }
    let [red, green, blue] = rgb(s.color)?;
    Ok(ShadowEffect {
        enabled: Some(master && s.enabled.unwrap_or(false)),
        angle: s.angle.ok_or("shadow angle is missing")?,
        distance: size(s.distance, scale)?,
        blur: size(s.size, scale)?,
        red,
        green,
        blue,
        opacity: s.opacity.unwrap_or(1.),
    })
}
fn glow_options(
    noise: Option<f64>,
    jitter: Option<f64>,
    range: Option<f64>,
    source: Option<ps::GlowSource>,
) -> Mapping<()> {
    if noise.unwrap_or(0.) != 0.
        || jitter.unwrap_or(0.) != 0.
        || range.unwrap_or(0.5) != 0.5
        || source == Some(ps::GlowSource::Center)
    {
        return Err("noisy, centered or custom-range glows are unsupported");
    }
    Ok(())
}

pub(in crate::psd) fn decode(e: &ps::LayerEffectsInfo) -> Mapping<LayerEffects> {
    if e.bevel.is_some()
        || e.satin.is_some()
        || e.gradient_overlay.is_some()
        || e.pattern_overlay.is_some()
    {
        return Err("bevel, satin, gradient and pattern styles are unsupported");
    }
    let scale = e.scale.unwrap_or(1.);
    if !scale.is_finite() || scale <= 0. {
        return Err("effect scale is invalid");
    }
    let master = e.disabled != Some(true);
    let stroke = one(e.stroke.as_ref())?
        .map(|s| -> Mapping<_> {
            normal(s.blend_mode)?;
            if s.fill_type != Some(ps::StrokeFillType::Color)
                || s.gradient.is_some()
                || s.pattern.is_some()
                || s.position == Some(ps::StrokePosition::Center)
                || s.overprint == Some(true)
            {
                return Err("only solid inside or outside strokes are supported");
            }
            let [red, green, blue] = rgb(s.color)?;
            Ok(StrokeEffect {
                enabled: Some(master && s.enabled.unwrap_or(false)),
                size: size(s.size, scale)?,
                inside: s.position == Some(ps::StrokePosition::Inside),
                red,
                green,
                blue,
                opacity: s.opacity.unwrap_or(1.),
            })
        })
        .transpose()?;
    let color_overlay = one(e.solid_fill.as_ref())?
        .map(|s| -> Mapping<_> {
            normal(s.blend_mode)?;
            let [red, green, blue] = rgb(s.color)?;
            Ok(ColorOverlayEffect {
                enabled: Some(master && s.enabled.unwrap_or(false)),
                red,
                green,
                blue,
                opacity: s.opacity.unwrap_or(1.),
            })
        })
        .transpose()?;
    let outer_glow = e
        .outer_glow
        .as_ref()
        .map(|s| -> Mapping<_> {
            normal(s.blend_mode)?;
            contour(s.contour.as_ref())?;
            zero_choke(s.choke)?;
            glow_options(s.noise, s.jitter, s.range, s.source)?;
            let [red, green, blue] = rgb(s.color)?;
            Ok(OuterGlowEffect {
                enabled: Some(master && s.enabled.unwrap_or(false)),
                size: size(s.size, scale)?,
                red,
                green,
                blue,
                opacity: s.opacity.unwrap_or(1.),
            })
        })
        .transpose()?;
    let inner_glow = e
        .inner_glow
        .as_ref()
        .map(|s| -> Mapping<_> {
            normal(s.blend_mode)?;
            contour(s.contour.as_ref())?;
            zero_choke(s.choke)?;
            glow_options(s.noise, s.jitter, s.range, s.source)?;
            if s.technique == Some(ps::GlowTechnique::Precise) {
                return Err("precise glow technique is unsupported");
            }
            let [red, green, blue] = rgb(s.color)?;
            Ok(InnerGlowEffect {
                enabled: Some(master && s.enabled.unwrap_or(false)),
                size: size(s.size, scale)?,
                red,
                green,
                blue,
                opacity: s.opacity.unwrap_or(1.),
            })
        })
        .transpose()?;
    let effects = LayerEffects {
        pattern_overlay: None,
        gradient_overlay: None,
        bevel: None,
        stroke,
        color_overlay,
        outer_glow,
        inner_glow,
        shadow: one(e.drop_shadow.as_ref())?
            .map(|s| shadow(s, scale, master))
            .transpose()?,
        inner_shadow: one(e.inner_shadow.as_ref())?
            .map(|s| shadow(s, scale, master))
            .transpose()?,
    };
    if !effects.validate() {
        return Err("effect values exceed supported ranges");
    }
    Ok(effects)
}
