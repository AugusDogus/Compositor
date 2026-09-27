//! Conservative Photoshop styles: bake combinations whose geometry or blending
//! cannot be represented by a single set of pixel-valued lfx2 descriptors.
mod import;
use crate::{
    blend::Blend,
    document::{Document, Layer},
    effects::{LayerEffects, ShadowEffect},
};
use ag_psd::psd as ps;
pub(super) use import::decode;

type Mapping<T> = std::result::Result<T, &'static str>;

fn color(red: f64, green: f64, blue: f64) -> ps::Color {
    ps::Color::Rgb(ps::Rgb {
        r: red * 255.,
        g: green * 255.,
        b: blue * 255.,
    })
}
fn pixels(value: f64) -> ps::UnitsValue {
    ps::UnitsValue {
        units: ps::Units::Pixels,
        value,
    }
}
fn linear() -> ps::EffectContour {
    ps::EffectContour {
        name: "Linear".into(),
        curve: vec![ps::PointF { x: 0., y: 0. }, ps::PointF { x: 255., y: 255. }],
    }
}

pub(super) fn encode(doc: &Document, layer: &Layer) -> Mapping<ps::LayerEffectsInfo> {
    let effects = layer.effects.as_ref().ok_or("no layer effects")?;
    if layer.transform.warp.is_some() {
        return Err("perspective cannot be represented by Photoshop effect sizes");
    }
    if effects.bevel.is_some() {
        return Err(
            "Bevel/Emboss is rendered into PSD pixels; save a .comp project to keep it editable",
        );
    }
    if effects.gradient_overlay.is_some() {
        return Err(
            "Gradient Overlay is rendered into PSD pixels; save a .comp project to keep its independent color and opacity stops editable",
        );
    }
    if effects.pattern_overlay.is_some() {
        return Err(
            "Pattern Overlay is rendered into PSD pixels; save a .comp project to keep it editable",
        );
    }
    if layer.mask.is_some() {
        return Err("layer-mask ordering differs from Photoshop");
    }
    if layer.blend != Blend::Normal
        || layer.clip_source.is_some()
        || doc
            .layers
            .iter()
            .any(|other| other.clip_source == Some(layer.id))
    {
        return Err("effect blending in this layer or clipping stack differs from Photoshop");
    }
    let source = layer.raster().ok_or("the layer has no raster source")?;
    let scale = layer.transform.size[0] / f64::from(source.width());
    let y_scale = layer.transform.size[1] / f64::from(source.height());
    if (scale - y_scale).abs() > scale.max(y_scale) * 1e-9 {
        return Err("nonuniform scaling cannot be represented by Photoshop effect sizes");
    }
    let mut effects = effects.clone();
    if let Some(stroke) = &mut effects.stroke {
        stroke.size *= scale;
    }
    if let Some(glow) = &mut effects.outer_glow {
        glow.size *= scale;
    }
    if let Some(glow) = &mut effects.inner_glow {
        glow.size *= scale;
    }
    for shadow in [&mut effects.shadow, &mut effects.inner_shadow]
        .into_iter()
        .flatten()
    {
        shadow.distance *= scale;
        shadow.blur *= scale;
        let angle = shadow.angle.to_radians();
        let mut offset = [-angle.cos(), angle.sin()];
        if layer.transform.flip_x {
            offset[0] = -offset[0];
        }
        if layer.transform.flip_y {
            offset[1] = -offset[1];
        }
        let (sin, cos) = layer.transform.rotation.to_radians().sin_cos();
        let [x, y] = [
            offset[0] * cos - offset[1] * sin,
            offset[0] * sin + offset[1] * cos,
        ];
        shadow.angle = y.atan2(-x).to_degrees();
    }
    if !effects.validate() {
        return Err("transformed effect sizes exceed the supported editable ranges");
    }
    Ok(descriptors(&effects))
}

fn shadow(value: &ShadowEffect) -> ps::LayerEffectShadow {
    ps::LayerEffectShadow {
        enabled: Some(value.enabled != Some(false)),
        present: Some(true),
        show_in_dialog: Some(true),
        size: Some(pixels(value.blur)),
        distance: Some(pixels(value.distance)),
        angle: Some(value.angle),
        color: Some(color(value.red, value.green, value.blue)),
        opacity: Some(value.opacity),
        blend_mode: Some(ps::BlendMode::Normal),
        use_global_light: Some(false),
        choke: Some(pixels(0.)),
        contour: Some(linear()),
        layer_conceals: Some(false),
        ..Default::default()
    }
}
fn descriptors(e: &LayerEffects) -> ps::LayerEffectsInfo {
    ps::LayerEffectsInfo {
        disabled: Some(false),
        scale: Some(1.),
        stroke: e.stroke.as_ref().map(|s| {
            vec![ps::LayerEffectStroke {
                enabled: Some(s.enabled != Some(false)),
                present: Some(true),
                show_in_dialog: Some(true),
                size: Some(pixels(s.size)),
                position: Some(if s.inside {
                    ps::StrokePosition::Inside
                } else {
                    ps::StrokePosition::Outside
                }),
                fill_type: Some(ps::StrokeFillType::Color),
                blend_mode: Some(ps::BlendMode::Normal),
                color: Some(color(s.red, s.green, s.blue)),
                opacity: Some(s.opacity),
                ..Default::default()
            }]
        }),
        drop_shadow: e.shadow.as_ref().map(|s| vec![shadow(s)]),
        inner_shadow: e.inner_shadow.as_ref().map(|s| vec![shadow(s)]),
        solid_fill: e.color_overlay.as_ref().map(|s| {
            vec![ps::LayerEffectSolidFill {
                enabled: Some(s.enabled != Some(false)),
                present: Some(true),
                show_in_dialog: Some(true),
                blend_mode: Some(ps::BlendMode::Normal),
                color: Some(color(s.red, s.green, s.blue)),
                opacity: Some(s.opacity),
            }]
        }),
        outer_glow: e.outer_glow.as_ref().map(|s| ps::LayerEffectsOuterGlow {
            enabled: Some(s.enabled != Some(false)),
            present: Some(true),
            show_in_dialog: Some(true),
            size: Some(pixels(s.size)),
            blend_mode: Some(ps::BlendMode::Normal),
            color: Some(color(s.red, s.green, s.blue)),
            opacity: Some(s.opacity),
            source: Some(ps::GlowSource::Edge),
            range: Some(0.5),
            noise: Some(0.),
            jitter: Some(0.),
            choke: Some(pixels(0.)),
            contour: Some(linear()),
            ..Default::default()
        }),
        inner_glow: e.inner_glow.as_ref().map(|s| ps::LayerEffectInnerGlow {
            enabled: Some(s.enabled != Some(false)),
            present: Some(true),
            show_in_dialog: Some(true),
            size: Some(pixels(s.size)),
            blend_mode: Some(ps::BlendMode::Normal),
            color: Some(color(s.red, s.green, s.blue)),
            opacity: Some(s.opacity),
            source: Some(ps::GlowSource::Edge),
            technique: Some(ps::GlowTechnique::Softer),
            range: Some(0.5),
            noise: Some(0.),
            jitter: Some(0.),
            choke: Some(pixels(0.)),
            contour: Some(linear()),
            ..Default::default()
        }),
        ..Default::default()
    }
}
