//! Camera Raw's saturation-aware vibrance with protection for skin hues.
use crate::{Result, invalid};
use image::RgbaImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Parameters")]
pub struct Vibrance {
    vibrance: f32,
    saturation: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Parameters {
    vibrance: f32,
    saturation: f32,
}

impl TryFrom<Parameters> for Vibrance {
    type Error = crate::Error;

    fn try_from(value: Parameters) -> Result<Self> {
        Self::new(value.vibrance, value.saturation)
    }
}

impl Vibrance {
    /// Both sliders use Camera Raw's signed percentage range, -100 through 100.
    pub fn new(vibrance: f32, saturation: f32) -> Result<Self> {
        for (name, value) in [("Vibrance", vibrance), ("Saturation", saturation)] {
            if !(-100. ..=100.).contains(&value) {
                return Err(invalid(format!(
                    "{name} must be between -100% and 100%. The original pixels are preserved."
                )));
            }
        }
        Ok(Self {
            vibrance,
            saturation,
        })
    }

    pub fn vibrance(self) -> f32 {
        self.vibrance
    }

    pub fn saturation(self) -> f32 {
        self.saturation
    }

    pub(crate) fn identity(self) -> bool {
        self.vibrance == 0. && self.saturation == 0.
    }

    /// Encoded, straight RGB. Matches `vibrance_and_saturation` in AdjustPixels.c;
    /// f32 arithmetic also permits matching GPU evaluation without quantization.
    pub(crate) fn apply_rgb(self, rgb: [f32; 3]) -> [f32; 3] {
        if self.identity() {
            return rgb;
        }
        let [r, g, b] = rgb;
        let max = r.max(g).max(b);
        let chroma = max - r.min(g).min(b);
        let saturation = if max <= 1e-8 { 0. } else { chroma / max };
        let hue = if chroma <= 1e-8 {
            0.
        } else if r >= g && r >= b {
            (60. * ((g - b) / chroma)).rem_euclid(360.)
        } else if g >= r && g >= b {
            60. * ((b - r) / chroma + 2.)
        } else {
            60. * ((r - g) / chroma + 4.)
        };
        let skin = if (10. ..=50.).contains(&hue) {
            let weight = if hue <= 30. {
                (hue - 10.) / 20.
            } else {
                (50. - hue) / 20.
            };
            weight * ((saturation - 0.15) / 0.35).clamp(0., 1.)
        } else {
            0.
        };
        let mut amount = self.vibrance / 100. * (1. - saturation);
        if self.vibrance > 0. {
            amount *= 1. - 0.7 * skin;
        }
        let boosted = scale_chroma(rgb, 1. + amount);
        scale_chroma(boosted, 1. + self.saturation / 100.)
    }

    pub(crate) fn pixel(self, source: [u8; 4]) -> [u8; 4] {
        if source[3] == 0 || self.identity() {
            return source;
        }
        let input = std::array::from_fn(|channel| f32::from(source[channel]) / 255.);
        let mut output = source;
        for (channel, value) in self.apply_rgb(input).into_iter().enumerate() {
            output[channel] = (value * 255.).round() as u8;
        }
        output
    }
}

fn scale_chroma(rgb: [f32; 3], factor: f32) -> [f32; 3] {
    let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    rgb.map(|value| (luma + (value - luma) * factor).clamp(0., 1.))
}

pub(crate) fn reference(image: &RgbaImage, settings: Vibrance) -> RgbaImage {
    let mut output = image.clone();
    let bytes: &mut [u8] = output.as_mut();
    bytes
        .par_chunks_exact_mut(4)
        .for_each(|p| p.copy_from_slice(&settings.pixel([p[0], p[1], p[2], p[3]])));
    output
}

#[cfg(test)]
mod tests;
