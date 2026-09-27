//! Luminance-only unsharp masking with gradual suppression of small differences.
//! This is not lens or motion deconvolution.
use crate::{Result, document::validate_size};
use image::RgbaImage;
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub amount: f64,
    pub radius: f64,
    pub noise: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            amount: 100.,
            radius: 1.,
            noise: 10.,
        }
    }
}
impl Settings {
    pub fn validate(self) -> Result<()> {
        super::finishing::range(self.amount, 0., 500.)?;
        super::finishing::range(self.radius, 0.1, 250.)?;
        super::finishing::range(self.noise, 0., 100.)
    }
}
pub(super) fn apply(image: &RgbaImage, settings: Settings) -> Result<RgbaImage> {
    settings.validate()?;
    validate_size(image.width(), image.height())?;
    if settings.amount == 0. {
        return Ok(image.clone());
    }
    match crate::render::gpu::luminosity_sharpen::apply(image, settings)? {
        Some(result) => Ok(result),
        None => Ok(cpu(image, settings)),
    }
}
pub(crate) fn cpu(image: &RgbaImage, settings: Settings) -> RgbaImage {
    let blur = |values: &[f32]| {
        crate::effects::cpu::gaussian(
            values,
            image.width() as usize,
            image.height() as usize,
            settings.radius as f32,
        )
    };
    let plane: Vec<_> = image.pixels().map(|p| f32::from(p[3]) / 255.).collect();
    let blurred_alpha = blur(&plane);
    drop(plane);
    let plane: Vec<_> = image
        .pixels()
        .map(|p| luma(&p.0) * f32::from(p[3]) / 255.)
        .collect();
    let blurred_luma = blur(&plane);
    drop(plane);
    let mut result = image.clone();
    result
        .par_chunks_exact_mut(4)
        .zip(blurred_luma.par_iter().zip(&blurred_alpha))
        .for_each(|(pixel, (base, alpha))| {
            if pixel[3] == 0 || *alpha <= 0. {
                return;
            }
            let difference = luma(pixel) - base / alpha;
            let gate = settings.noise as f32 * 0.12;
            let keep = if gate > 0. {
                (difference.abs() / gate).min(1.)
            } else {
                1.
            };
            let add = difference * settings.amount as f32 / 100. * keep;
            for value in &mut pixel[..3] {
                *value = (f32::from(*value) + add).clamp(0., 255.).round() as u8;
            }
        });
    result
}
fn luma(pixel: &[u8]) -> f32 {
    f32::from(pixel[0]) * 0.299 + f32::from(pixel[1]) * 0.587 + f32::from(pixel[2]) * 0.114
}
#[cfg(test)]
mod tests;
