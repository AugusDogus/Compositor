//! Radius-aware tonal correction using alpha-weighted local brightness.
use crate::{Result, invalid};
use image::{ImageBuffer, Luma, RgbaImage};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Parameters")]
pub struct Settings {
    shadows: f64,
    highlights: f64,
    radius: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Parameters {
    shadows: f64,
    highlights: f64,
    radius: f64,
}

impl TryFrom<Parameters> for Settings {
    type Error = crate::Error;

    fn try_from(value: Parameters) -> Result<Self> {
        Self::new(value.shadows, value.highlights, value.radius)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shadows: 35.,
            highlights: 0.,
            radius: 30.,
        }
    }
}

impl Settings {
    pub fn new(shadows: f64, highlights: f64, radius: f64) -> Result<Self> {
        for (name, value, minimum, maximum) in [
            ("Shadows", shadows, 0., 100.),
            ("Highlights", highlights, 0., 100.),
            ("Radius", radius, 1., 500.),
        ] {
            if !(minimum..=maximum).contains(&value) {
                return Err(invalid(format!(
                    "Shadows/Highlights {name} must be between {minimum} and {maximum}. The original pixels are preserved."
                )));
            }
        }
        Ok(Self {
            shadows,
            highlights,
            radius,
        })
    }

    pub fn shadows(self) -> f64 {
        self.shadows
    }

    pub fn highlights(self) -> f64 {
        self.highlights
    }

    pub fn radius(self) -> f64 {
        self.radius
    }

    pub fn identity(self) -> bool {
        self.shadows == 0. && self.highlights == 0.
    }
}

type FloatPlane = ImageBuffer<Luma<f32>, Vec<f32>>;

/// `pixel_scale` is preview pixels per source pixel, in (0, 1]. Radius is
/// scaled after validating the saved setting, so zoomed-out previews may use a
/// subpixel radius. `accelerated = false` keeps all computation on the CPU.
pub(crate) fn apply(
    image: &RgbaImage,
    settings: Settings,
    pixel_scale: f64,
    accelerated: bool,
) -> Result<RgbaImage> {
    crate::document::validate_size(image.width(), image.height())?;
    if !pixel_scale.is_finite() || pixel_scale <= 0. || pixel_scale > 1. {
        return Err(invalid(
            "Shadows/Highlights preview scale must be greater than zero and at most one.",
        ));
    }
    if settings.identity() {
        return Ok(image.clone());
    }
    // Source, retained plane, blur input and up to four GPU scratch planes.
    crate::document::validate_pixel_budget(
        u64::from(image.width()) * u64::from(image.height()) * 7,
    )?;
    let sigma = (settings.radius * pixel_scale / 2.) as f32;
    let blur = |plane: FloatPlane| -> Result<Vec<f32>> {
        // Off-center weights are zero at this scale. Avoid squaring subnormal
        // sigma in either blur kernel, while retaining the actual preview scale.
        if sigma <= 0.01 {
            return Ok(plane.into_raw());
        }
        if accelerated && let Some(result) = crate::render::gpu_float_blur(&plane, sigma)? {
            return Ok(result.into_raw());
        }
        Ok(crate::effects::cpu::gaussian(
            plane.as_raw(),
            image.width() as usize,
            image.height() as usize,
            sigma,
        ))
    };
    let coverage = blur(FloatPlane::from_fn(
        image.width(),
        image.height(),
        |x, y| Luma([f32::from(image[(x, y)][3]) / 255.]),
    ))?;
    let luminance = blur(FloatPlane::from_fn(
        image.width(),
        image.height(),
        |x, y| {
            let p = image[(x, y)];
            let luma =
                (0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2]))
                    / 255.;
            Luma([luma * f32::from(p[3]) / 255.])
        },
    ))?;
    let shadows = (settings.shadows / 100.) as f32;
    let highlights = (settings.highlights / 100.) as f32;
    let mut output = image.clone();
    output
        .par_chunks_exact_mut(4)
        .zip(luminance.par_iter().zip(&coverage))
        .for_each(|(pixel, (&luma, &alpha))| {
            if pixel[3] == 0 {
                return;
            }
            let local = if alpha > 0. {
                (luma / alpha).clamp(0., 1.)
            } else {
                0.5
            };
            let lift = 1. / (1. + shadows * (1. - local).powi(2) * 2.);
            let drop = 1. + highlights * local.powi(2) * 2.;
            for channel in &mut pixel[..3] {
                let value = f32::from(*channel) / 255.;
                *channel = (value.powf(lift * drop) * 255.).round() as u8;
            }
        });
    Ok(output)
}

#[cfg(test)]
mod tests;
