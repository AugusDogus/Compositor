//! Editable effects use the macOS project schema and leave source pixels untouched.
pub(crate) mod cpu;
mod surface;
use serde::{Deserialize, Serialize};
pub(crate) use surface::prepare;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerEffects {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<StrokeEffect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<ShadowEffect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_overlay: Option<ColorOverlayEffect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner_shadow: Option<ShadowEffect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_glow: Option<OuterGlowEffect>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StrokeEffect {
    pub enabled: Option<bool>,
    pub size: f64,
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub opacity: f64,
    pub inside: bool,
}
impl Default for StrokeEffect {
    fn default() -> Self {
        Self {
            enabled: None,
            size: 4.,
            red: 0.,
            green: 0.,
            blue: 0.,
            opacity: 1.,
            inside: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShadowEffect {
    pub enabled: Option<bool>,
    pub angle: f64,
    pub distance: f64,
    pub blur: f64,
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub opacity: f64,
}
impl Default for ShadowEffect {
    fn default() -> Self {
        Self {
            enabled: None,
            angle: 90.,
            distance: 20.,
            blur: 20.,
            red: 0.,
            green: 0.,
            blue: 0.,
            opacity: 0.5,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorOverlayEffect {
    pub enabled: Option<bool>,
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub opacity: f64,
}
impl Default for ColorOverlayEffect {
    fn default() -> Self {
        Self {
            enabled: None,
            red: 0.,
            green: 0.,
            blue: 0.,
            opacity: 1.,
        }
    }
}
/// Soft coverage outside the layer, using half the size as Gaussian sigma.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OuterGlowEffect {
    pub enabled: Option<bool>,
    pub size: f64,
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub opacity: f64,
}
impl Default for OuterGlowEffect {
    fn default() -> Self {
        Self {
            enabled: None,
            size: 20.,
            red: 1.,
            green: 1.,
            blue: 1.,
            opacity: 0.75,
        }
    }
}
fn unit(value: f64) -> bool {
    value.is_finite() && (0. ..=1.).contains(&value)
}
fn color(red: f64, green: f64, blue: f64, opacity: f64) -> bool {
    [red, green, blue, opacity].into_iter().all(unit)
}
impl ShadowEffect {
    pub fn inner_default() -> Self {
        Self {
            distance: 10.,
            blur: 10.,
            ..Self::default()
        }
    }
    pub fn offset(&self) -> [f32; 2] {
        let a = self.angle.to_radians();
        [
            (-a.cos() * self.distance) as f32,
            (a.sin() * self.distance) as f32,
        ]
    }
    fn validate(&self) -> bool {
        self.angle.is_finite()
            && (-360. ..=360.).contains(&self.angle)
            && self.distance.is_finite()
            && (0. ..=5000.).contains(&self.distance)
            && self.blur.is_finite()
            && (0. ..=500.).contains(&self.blur)
            && color(self.red, self.green, self.blue, self.opacity)
    }
}
impl LayerEffects {
    pub fn validate(&self) -> bool {
        self.stroke.as_ref().is_none_or(|s| {
            s.size.is_finite()
                && (0. ..=500.).contains(&s.size)
                && color(s.red, s.green, s.blue, s.opacity)
        }) && self.outer_glow.as_ref().is_none_or(|s| {
            s.size.is_finite()
                && (0. ..=500.).contains(&s.size)
                && color(s.red, s.green, s.blue, s.opacity)
        }) && self.shadow.as_ref().is_none_or(ShadowEffect::validate)
            && self
                .inner_shadow
                .as_ref()
                .is_none_or(ShadowEffect::validate)
            && self
                .color_overlay
                .as_ref()
                .is_none_or(|s| color(s.red, s.green, s.blue, s.opacity))
    }
    pub fn validate_size(&self, width: u32, height: u32) -> bool {
        let margin = u64::from(self.margin()) * 2;
        (u64::from(width) + margin)
            .checked_mul(u64::from(height) + margin)
            .is_some_and(|area| area <= 100_000_000)
    }
    pub fn is_empty(&self) -> bool {
        self.stroke.is_none()
            && self.shadow.is_none()
            && self.color_overlay.is_none()
            && self.inner_shadow.is_none()
            && self.outer_glow.is_none()
    }
    pub fn visible(&self) -> Self {
        Self {
            outer_glow: self.outer_glow.clone().filter(|s| s.enabled != Some(false)),
            stroke: self.stroke.clone().filter(|s| s.enabled != Some(false)),
            shadow: self.shadow.clone().filter(|s| s.enabled != Some(false)),
            color_overlay: self
                .color_overlay
                .clone()
                .filter(|s| s.enabled != Some(false)),
            inner_shadow: self
                .inner_shadow
                .clone()
                .filter(|s| s.enabled != Some(false)),
        }
    }
    pub fn margin(&self) -> u32 {
        let effects = self.visible();
        let stroke = effects
            .stroke
            .as_ref()
            .filter(|s| !s.inside)
            .map_or(0., |s| s.size);
        let shadow = effects
            .shadow
            .as_ref()
            .map_or(0., |s| s.distance + s.blur * 3.);
        let glow = effects.outer_glow.as_ref().map_or(0., |s| s.size * 3.);
        stroke.max(shadow).max(glow).ceil() as u32 + 2
    }
}

#[cfg(test)]
mod tests;

/// Warp rendered effects only for a temporary canvas preview. Committing a
/// distortion still warps source pixels and keeps effect settings editable.
pub fn distorted_preview(
    current: &crate::document::Document,
    original: &crate::document::Document,
    bounds: crate::geometry::Transform,
    corners: [crate::geometry::Point; 4],
) -> crate::Result<crate::document::Document> {
    let targets = crate::transform::target_ids(original);
    let mut prepared = prepare(original, true)?;
    crate::distort::apply(&mut prepared, bounds, corners, false)?;
    let mut result = current.clone();
    for layer in &mut result.layers {
        if !targets.contains(&layer.id)
            || original
                .layer(layer.id)
                .is_none_or(|l| l.effects.as_ref().is_none_or(|e| e.visible().is_empty()))
        {
            continue;
        }
        if let Some(warped) = prepared.layer(layer.id) {
            layer.content = warped.content.clone();
            layer.transform = warped.transform;
            layer.mask = warped.mask.clone();
            layer.effects = None;
            layer.text = None;
            layer.raw = None;
            layer.shape = None;
        }
    }
    Ok(result)
}

/// Gaussian coverage blur with clamped edges, accelerated by the effect kernels.
pub fn gaussian_mask(mask: &image::GrayImage, sigma: f32) -> crate::Result<image::GrayImage> {
    if !sigma.is_finite()
        || !(0. ..=500.).contains(&sigma)
        || mask.width() == 0
        || mask.height() == 0
    {
        return Err(crate::invalid(
            "Coverage blur requires a nonempty mask and a finite radius from 0 to 500 pixels.",
        ));
    }
    if sigma == 0. {
        return Ok(mask.clone());
    }
    if let Some(result) = crate::render::gpu_coverage_blur(mask, sigma)? {
        return Ok(result);
    }
    let values: Vec<_> = mask.as_raw().iter().map(|v| f32::from(*v) / 255.).collect();
    let blurred = cpu::gaussian(
        &values,
        mask.width() as usize,
        mask.height() as usize,
        sigma,
    );
    image::GrayImage::from_raw(
        mask.width(),
        mask.height(),
        blurred
            .into_iter()
            .map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
            .collect(),
    )
    .ok_or_else(|| crate::invalid("Coverage blur returned the wrong pixel count."))
}
