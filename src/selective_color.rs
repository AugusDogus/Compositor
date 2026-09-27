//! CMYK corrections within nine overlapping ranges of encoded RGB.
//!
//! The equations follow the mathematical analysis at
//! https://blog.pkh.me/p/22-understanding-selective-coloring-in-adobe-photoshop.html.
//! Every range uses the original RGB; their corrections are summed before clipping.
use crate::{Result, invalid};
use image::RgbaImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Relative,
    Absolute,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Range {
    #[default]
    Reds,
    Yellows,
    Greens,
    Cyans,
    Blues,
    Magentas,
    Whites,
    Neutrals,
    Blacks,
}

impl Range {
    pub const ALL: [Self; 9] = [
        Self::Reds,
        Self::Yellows,
        Self::Greens,
        Self::Cyans,
        Self::Blues,
        Self::Magentas,
        Self::Whites,
        Self::Neutrals,
        Self::Blacks,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Reds => "Reds",
            Self::Yellows => "Yellows",
            Self::Greens => "Greens",
            Self::Cyans => "Cyans",
            Self::Blues => "Blues",
            Self::Magentas => "Magentas",
            Self::Whites => "Whites",
            Self::Neutrals => "Neutrals",
            Self::Blacks => "Blacks",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectiveColor {
    /// CMYK percentages, each from -100 to 100, in `Range::ALL` order.
    pub adjustments: [[f32; 4]; 9],
    pub mode: Mode,
}

impl SelectiveColor {
    pub fn validate(self) -> Result<()> {
        if self
            .adjustments
            .iter()
            .flatten()
            .any(|value| !(-100. ..=100.).contains(value))
        {
            return Err(invalid(
                "Selective Color CMYK values must be between -100% and 100%. The original pixels are preserved.",
            ));
        }
        Ok(())
    }

    pub(crate) fn identity(self) -> bool {
        self.adjustments.iter().flatten().all(|value| *value == 0.)
    }

    pub(crate) fn coefficients(self) -> [[f32; 4]; 9] {
        self.adjustments.map(|row| row.map(|value| value / 100.))
    }

    pub(crate) fn apply_rgb(self, input: [f32; 3]) -> [f32; 3] {
        let weights = membership(input);
        let mut correction = [0.; 3];
        for (row, weight) in self.coefficients().into_iter().zip(weights) {
            if weight == 0. {
                continue;
            }
            for channel in 0..3 {
                let available_ink = 1. - input[channel];
                let mode_scale = match self.mode {
                    Mode::Relative => available_ink,
                    Mode::Absolute => 1.,
                };
                let change = (-(1. + row[channel]) * row[3] - row[channel]) * mode_scale;
                correction[channel] += change.clamp(-input[channel], available_ink) * weight;
            }
        }
        std::array::from_fn(|channel| (input[channel] + correction[channel]).clamp(0., 1.))
    }

    /// Preserve alpha and RGB hidden by zero alpha, using the GPU's f32 arithmetic.
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

/// Chromatic ranges fade with their distance from the other channels. Achromatic
/// ranges overlap continuously, excluding pure black/white from neutrals.
fn membership([r, g, b]: [f32; 3]) -> [f32; 9] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    [
        (r - g.max(b)).max(0.),
        (r.min(g) - b).max(0.),
        (g - r.max(b)).max(0.),
        (g.min(b) - r).max(0.),
        (b - r.max(g)).max(0.),
        (r.min(b) - g).max(0.),
        (2. * min - 1.).max(0.),
        (1. - (max - 0.5).abs() - (min - 0.5).abs()).max(0.),
        (1. - 2. * max).max(0.),
    ]
}

pub(crate) fn reference(image: &RgbaImage, settings: SelectiveColor) -> RgbaImage {
    let mut output = image.clone();
    let bytes: &mut [u8] = output.as_mut();
    bytes.par_chunks_exact_mut(4).for_each(|pixel| {
        pixel.copy_from_slice(&settings.pixel([pixel[0], pixel[1], pixel[2], pixel[3]]));
    });
    output
}

#[cfg(test)]
mod tests;
