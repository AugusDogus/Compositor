//! Camera Raw grading for rendered image layers, using upstream's portable kernels.
mod color;
mod geometry;
mod native;
mod preview;
pub use preview::{Clipping, Preview};
pub mod sampling;
pub mod scalars;
#[cfg(test)]
mod tests;
use crate::{
    Result,
    document::{Document, LayerContent},
    invalid,
};
pub use color::{ColorGrading, ColorMixer, PointColor, Wheel};
pub use geometry::Guide;
use image::RgbaImage;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Light,
    Color,
    Effects,
    Curve,
    Mixer,
    Grading,
    Detail,
    Optics,
    Geometry,
    Calibration,
}
impl Group {
    pub const ALL: [Self; 10] = [
        Self::Light,
        Self::Color,
        Self::Effects,
        Self::Curve,
        Self::Mixer,
        Self::Grading,
        Self::Detail,
        Self::Optics,
        Self::Geometry,
        Self::Calibration,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Color => "Color",
            Self::Effects => "Effects",
            Self::Curve => "Curve",
            Self::Mixer => "Color Mixer",
            Self::Grading => "Color Grading",
            Self::Detail => "Detail",
            Self::Optics => "Optics",
            Self::Geometry => "Geometry",
            Self::Calibration => "Calibration",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GlowStyle {
    #[default]
    Diffusion,
    Bloom,
    Halation,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VignetteStyle {
    #[default]
    HighlightPriority,
    ColorPriority,
    PaintOverlay,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Projection {
    #[default]
    Perspective,
    Rectilinear,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Process {
    One,
    Two,
    Three,
    Four,
    Five,
    #[default]
    Six,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub light: scalars::Light,
    pub color: scalars::Color,
    pub effects: scalars::Effects,
    pub curve: scalars::Curve,
    pub curves: crate::adjustment::Curves,
    pub mixer: ColorMixer,
    pub grading: ColorGrading,
    pub detail: scalars::Detail,
    pub optics: scalars::Optics,
    pub geometry: scalars::Geometry,
    pub calibration: scalars::Calibration,
    pub glow_style: GlowStyle,
    pub vignette_style: VignetteStyle,
    pub remove_chromatic: bool,
    pub lens_profile: bool,
    pub projection: Projection,
    pub guided: bool,
    pub guides: Vec<Guide>,
    pub constrain_crop: bool,
    pub process: Process,
    pub enabled: [bool; 10],
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            light: Default::default(),
            color: Default::default(),
            effects: Default::default(),
            curve: Default::default(),
            curves: Default::default(),
            mixer: Default::default(),
            grading: Default::default(),
            detail: Default::default(),
            optics: Default::default(),
            geometry: Default::default(),
            calibration: Default::default(),
            glow_style: Default::default(),
            vignette_style: Default::default(),
            remove_chromatic: false,
            lens_profile: false,
            projection: Default::default(),
            guided: false,
            guides: Vec::new(),
            constrain_crop: false,
            process: Default::default(),
            enabled: [true; 10],
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        self.light.validate()?;
        self.color.validate()?;
        self.effects.validate()?;
        self.curve.validate()?;
        self.detail.validate()?;
        self.optics.validate()?;
        self.geometry.validate()?;
        self.calibration.validate()?;
        if self.curve.dark_split < self.curve.shadow_split + 2.
            || self.curve.light_split < self.curve.dark_split + 2.
        {
            return Err(invalid(
                "Curve dividers must increase with at least two points between them.",
            ));
        }
        if self.optics.purple_hue_low > self.optics.purple_hue_high
            || self.optics.green_hue_low > self.optics.green_hue_high
        {
            return Err(invalid(
                "Each defringe hue range must start below its upper bound.",
            ));
        }
        self.mixer.validate()?;
        self.grading.validate()?;
        let adjustment = crate::adjustment::Adjustment {
            curves: self.curves.clone(),
            ..crate::adjustment::Adjustment::new(crate::adjustment::Kind::Curves)
        };
        adjustment.validate()?;
        if self.guides.len() > 4 || self.guides.iter().any(|g| !g.valid()) {
            return Err(invalid(
                "Use at most four geometry guides with endpoints inside the image.",
            ));
        }
        Ok(())
    }
    pub fn enabled(&self, group: Group) -> bool {
        self.enabled[group as usize]
    }
    /// Whether the group contributes pixels. Support controls stay idle until
    /// their effect amount is enabled, matching upstream's identity checks.
    fn adjusts(&self, group: Group) -> bool {
        match group {
            Group::Light => self.light != Default::default(),
            Group::Color => self.color != Default::default(),
            Group::Effects => {
                let e = &self.effects;
                [
                    e.texture,
                    e.clarity,
                    e.dehaze,
                    e.glow,
                    e.vignette_amount,
                    e.grain_amount,
                ]
                .iter()
                .any(|v| *v != 0.)
            }
            Group::Curve => {
                let c = &self.curve;
                [
                    c.shadows,
                    c.darks,
                    c.lights,
                    c.highlights,
                    c.refine_saturation,
                ]
                .iter()
                .any(|v| *v != 0.)
                    || self.curves != Default::default()
            }
            Group::Mixer => {
                let m = &self.mixer;
                m.hue
                    .iter()
                    .chain(&m.saturation)
                    .chain(&m.luminance)
                    .any(|v| *v != 0.)
                    || m.points.iter().any(|p| {
                        p.hue_shift != 0. || p.saturation_shift != 0. || p.luminance_shift != 0.
                    })
            }
            Group::Grading => self
                .grading
                .wheels
                .iter()
                .any(|w| w.saturation != 0. || w.luminance != 0.),
            Group::Detail => {
                let d = &self.detail;
                d.sharpen_amount != 0. || d.noise_luminance != 0. || d.noise_color != 0.
            }
            Group::Optics => {
                let o = &self.optics;
                self.remove_chromatic
                    || self.lens_profile
                    || o.distortion != 0.
                    || o.purple_amount != 0.
                    || o.green_amount != 0.
                    || o.vignette_amount != 0.
            }
            Group::Geometry => {
                self.geometry != Default::default()
                    || self.guided && self.guides.iter().any(Guide::usable)
            }
            Group::Calibration => self.calibration != Default::default(),
        }
    }
    pub fn rendered(&self) -> Self {
        let mut s = self.clone();
        for group in Group::ALL {
            if !self.enabled(group) {
                s.reset(group);
            }
        }
        s.enabled = [true; 10];
        s
    }
    pub fn reset(&mut self, group: Group) {
        let defaults = Self::default();
        match group {
            Group::Light => self.light = defaults.light,
            Group::Color => self.color = defaults.color,
            Group::Effects => {
                self.effects = defaults.effects;
                self.glow_style = defaults.glow_style;
                self.vignette_style = defaults.vignette_style;
            }
            Group::Curve => {
                self.curve = defaults.curve;
                self.curves = defaults.curves;
            }
            Group::Mixer => self.mixer = defaults.mixer,
            Group::Grading => self.grading = defaults.grading,
            Group::Detail => self.detail = defaults.detail,
            Group::Optics => {
                self.optics = defaults.optics;
                self.remove_chromatic = false;
                self.lens_profile = false;
            }
            Group::Geometry => {
                self.geometry = defaults.geometry;
                self.projection = defaults.projection;
                self.guided = false;
                self.guides.clear();
                self.constrain_crop = false;
            }
            Group::Calibration => {
                self.calibration = defaults.calibration;
                self.process = defaults.process;
            }
        }
    }
}
pub fn apply(document: &mut Document, settings: &Settings) -> Result<()> {
    apply_scaled(document, settings, None, Preview::default())
}

/// Bound interactive processing while keeping the original layer placement.
/// Full-resolution pixels are used only by the Apply worker.
pub fn preview(document: &mut Document, settings: &Settings, options: Preview) -> Result<()> {
    apply_scaled(document, settings, Some(1280), options)
}

fn apply_scaled(
    document: &mut Document,
    settings: &Settings,
    preview_side: Option<u32>,
    options: Preview,
) -> Result<()> {
    settings.validate()?;
    let selection = document.selection.clone();
    let layer = document
        .active_layer_mut()
        .ok_or_else(|| invalid("Select an image layer for Camera Raw."))?;
    layer.require_rasterized()?;
    let source = layer
        .raster()
        .ok_or_else(|| invalid("Select an image layer for Camera Raw."))?;
    let source = match preview_side {
        Some(limit) if source.width().max(source.height()) > limit => {
            let scale = f64::from(limit) / f64::from(source.width().max(source.height()));
            let resized = image::imageops::resize(
                &crate::native_pixels::premultiply(source),
                (f64::from(source.width()) * scale).round().max(1.) as u32,
                (f64::from(source.height()) * scale).round().max(1.) as u32,
                image::imageops::FilterType::Triangle,
            );
            Arc::new(crate::native_pixels::unpremultiply(resized))
        }
        _ => source.clone(),
    };
    let scale = preview_side.map_or(1., |limit| {
        (f64::from(limit) / f64::from(layer.raster().map_or(limit, |p| p.width().max(p.height()))))
            .min(1.)
    });
    let mut result = render_scaled(&source, settings, scale, options)?;
    if let Some(selection) = selection {
        let (w, h) = result.dimensions();
        for (x, y, pixel) in result.enumerate_pixels_mut() {
            let amount = selection.coverage(layer.transform.point([
                (f64::from(x) + 0.5) / f64::from(w),
                (f64::from(y) + 0.5) / f64::from(h),
            ]));
            let before = source[(x, y)];
            let a = f64::from(before[3]) * (1. - amount);
            let b = f64::from(pixel[3]) * amount;
            let alpha = a + b;
            for c in 0..3 {
                pixel[c] = if alpha > 0. {
                    ((f64::from(before[c]) * a + f64::from(pixel[c]) * b) / alpha).round() as u8
                } else {
                    0
                };
            }
            pixel[3] = alpha.round() as u8;
        }
    }
    layer.content = LayerContent::Raster(Some(Arc::new(result)));
    layer.shape = None;
    layer.text = None;
    Ok(())
}
pub fn render(source: &RgbaImage, settings: &Settings) -> Result<RgbaImage> {
    render_scaled(source, settings, 1., Preview::default())
}

fn render_scaled(
    source: &RgbaImage,
    settings: &Settings,
    scale: f64,
    options: Preview,
) -> Result<RgbaImage> {
    settings.validate()?;
    crate::document::validate_size(source.width(), source.height())?;
    let s = settings.rendered();
    if !Group::ALL.into_iter().any(|g| s.adjusts(g)) && options == Preview::default() {
        return Ok(source.clone());
    }
    let warped =
        if options.clipping.is_none() && !options.sharpen_mask && options.point_color.is_none() {
            geometry::render(source, &s)?
        } else {
            source.clone()
        };
    native::render(&warped, &s, scale, options)
}

/// Gray-world white balance in linear light, using the same gains as the grade.
pub fn auto_balance(source: &RgbaImage) -> Result<[f64; 2]> {
    let mut sum = [0.; 3];
    let mut weight = 0.;
    for pixel in source.pixels() {
        if pixel[3] == 0 {
            continue;
        }
        let alpha = 1.;
        weight += alpha;
        for c in 0..3 {
            let v = f64::from(pixel[c]) / 255.;
            sum[c] += alpha
                * if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                };
        }
    }
    if weight <= 0. {
        return Err(invalid(
            "White balance needs visible pixels. The current settings are preserved.",
        ));
    }
    sampling::white_balance_linear(sum.map(|x| x / weight)).ok_or_else(|| {
        invalid(
            "This image has too little color information for automatic white balance. Adjust Temperature and Tint manually.",
        )
    })
}
