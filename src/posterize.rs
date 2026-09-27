//! Equal-width encoded-RGB bins, including an exact 256-level identity.
use crate::{Result, invalid};
use image::RgbaImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Parameters")]
pub struct Posterize {
    levels: u16,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Parameters {
    levels: u16,
}
impl TryFrom<Parameters> for Posterize {
    type Error = crate::Error;
    fn try_from(value: Parameters) -> Result<Self> {
        Self::new(value.levels)
    }
}
impl Default for Posterize {
    fn default() -> Self {
        Self { levels: 4 }
    }
}
impl Posterize {
    pub fn new(levels: u16) -> Result<Self> {
        if !(2..=256).contains(&levels) {
            return Err(invalid(
                "Posterize needs a whole number of levels from 2 to 256.",
            ));
        }
        Ok(Self { levels })
    }
    pub fn levels(self) -> u16 {
        self.levels
    }
    pub(crate) fn apply_rgb(self, rgb: [f32; 3]) -> [f32; 3] {
        if self.levels == 256 {
            return rgb;
        }
        let levels = f32::from(self.levels);
        rgb.map(|c| (((c * 255.) * levels / 256.).floor() / (levels - 1.)).clamp(0., 1.))
    }
    pub(crate) fn pixel(self, source: [u8; 4]) -> [u8; 4] {
        if source[3] == 0 || self.levels == 256 {
            return source;
        }
        let mut output = source;
        let denominator = u32::from(self.levels - 1);
        for i in 0..3 {
            let bin = u32::from(source[i]) * u32::from(self.levels) / 256;
            output[i] = ((bin * 255 + denominator / 2) / denominator) as u8;
        }
        output
    }
}
pub(crate) fn reference(image: &RgbaImage, settings: Posterize) -> RgbaImage {
    let mut output = image.clone();
    let bytes: &mut [u8] = output.as_mut();
    bytes
        .par_chunks_exact_mut(4)
        .for_each(|p| p.copy_from_slice(&settings.pixel([p[0], p[1], p[2], p[3]])));
    output
}
#[cfg(test)]
mod tests;
