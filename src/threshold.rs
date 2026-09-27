//! Binary encoded-RGB luminance adjustment, with no intermediate gray quantization.
//!
//! Uses ITU-R 601-2 luma, also documented by Pillow's Image.convert:
//! https://pillow.readthedocs.io/en/stable/reference/Image.html#PIL.Image.Image.convert
use image::RgbaImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Threshold {
    /// Encoded luminance at or above this integer level becomes white.
    /// The byte type also rejects fractional and out-of-range serialized values.
    pub level: u8,
}

impl Default for Threshold {
    fn default() -> Self {
        Self { level: 128 }
    }
}

impl Threshold {
    pub(crate) fn apply_rgb(self, [r, g, b]: [f32; 3]) -> [f32; 3] {
        // Integer coefficients retain exact byte-color boundaries without
        // quantizing continuous adjustment-layer backdrops.
        let luma = (r * 255.) * 299. + (g * 255.) * 587. + (b * 255.) * 114.;
        [if luma >= f32::from(self.level) * 1000. {
            1.
        } else {
            0.
        }; 3]
    }

    pub(crate) fn pixel(self, source: [u8; 4]) -> [u8; 4] {
        if source[3] == 0 {
            return source;
        }
        let luma =
            u32::from(source[0]) * 299 + u32::from(source[1]) * 587 + u32::from(source[2]) * 114;
        let value = if luma >= u32::from(self.level) * 1000 {
            255
        } else {
            0
        };
        [value, value, value, source[3]]
    }
}

pub(crate) fn reference(image: &RgbaImage, settings: Threshold) -> RgbaImage {
    let mut output = image.clone();
    let bytes: &mut [u8] = output.as_mut();
    bytes.par_chunks_exact_mut(4).for_each(|pixel| {
        pixel.copy_from_slice(&settings.pixel([pixel[0], pixel[1], pixel[2], pixel[3]]));
    });
    output
}

#[cfg(test)]
mod tests;
