//! Gray-channel conditional blending, evaluated against the current backdrop.
//!
//! Each range has independent black and white split handles. Byte endpoints
//! correspond to PSD composite-gray blending ranges without quantization.
use crate::{Result, invalid};
use serde::{Deserialize, Serialize};

/// Independent rising (black) and falling (white) opacity ramps.
///
/// Each pair is ordered. The two ramps may overlap or cross, in which case
/// their weights multiply. A collapsed pair is an inclusive hard cutoff.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SavedRange", into = "SavedRange")]
pub struct Range {
    black: [u8; 2],
    white: [u8; 2],
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedRange {
    black: [u8; 2],
    white: [u8; 2],
}

impl TryFrom<SavedRange> for Range {
    type Error = crate::Error;

    fn try_from(value: SavedRange) -> Result<Self> {
        Self::new(value.black, value.white)
    }
}

impl From<Range> for SavedRange {
    fn from(value: Range) -> Self {
        Self {
            black: value.black,
            white: value.white,
        }
    }
}

impl Default for Range {
    fn default() -> Self {
        Self {
            black: [0, 0],
            white: [255, 255],
        }
    }
}

impl Range {
    pub fn new(black: [u8; 2], white: [u8; 2]) -> Result<Self> {
        if black[0] > black[1] || white[0] > white[1] {
            return Err(invalid(
                "Blend If split handles must increase from left to right within each black or white pair. The layer is unchanged.",
            ));
        }
        Ok(Self { black, white })
    }

    /// PSD order: black start, black end, white start, white end.
    pub fn from_endpoints(endpoints: [u8; 4]) -> Result<Self> {
        Self::new([endpoints[0], endpoints[1]], [endpoints[2], endpoints[3]])
    }

    pub fn endpoints(self) -> [u8; 4] {
        [self.black[0], self.black[1], self.white[0], self.white[1]]
    }

    pub fn is_identity(self) -> bool {
        self == Self::default()
    }

    fn weight_key(self, gray: u32) -> f64 {
        let [black_start, black_end, white_start, white_end] =
            self.endpoints().map(|v| u32::from(v) * 1000);
        let rising = if black_start == black_end {
            if gray >= black_start { 1. } else { 0. }
        } else {
            (f64::from(gray.saturating_sub(black_start)) / f64::from(black_end - black_start))
                .min(1.)
        };
        let falling = if white_start == white_end {
            if gray <= white_end { 1. } else { 0. }
        } else {
            (f64::from(white_end.saturating_sub(gray)) / f64::from(white_end - white_start)).min(1.)
        };
        rising * falling
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub source: Range,
    pub underlying: Range,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            source: Range::default(),
            underlying: Range::default(),
        }
    }
}

impl Settings {
    /// Disabled settings retain their handles but do not change composition.
    pub fn is_identity(self) -> bool {
        !self.enabled || (self.source.is_identity() && self.underlying.is_identity())
    }

    /// Coverage multiplier for finite straight encoded-sRGB pixels in 0..=1.
    ///
    /// RGB is rounded to the nearest byte before computing integer-weighted
    /// gray (299R + 587G + 114B). This keeps hard cutoffs stable at raster
    /// precision on both CPU and GPU. Gray values are independent of alpha. A fully
    /// transparent backdrop has no tone to exclude; partial coverage interpolates
    /// from no exclusion to its tonal weight. Hidden backdrop RGB therefore has
    /// no effect. Existing source alpha, masks, and layer opacity are multiplied
    /// by the returned weight exactly once, immediately before blending.
    ///
    /// This defines the Linux renderer's gray-channel behavior. It does not
    /// assert pixel-identical Photoshop color-management or blending behavior.
    pub fn weight(self, source: [f64; 4], underlying: [f64; 4]) -> f64 {
        if self.is_identity() {
            return 1.;
        }
        let source_weight = self.source.weight_key(gray(source));
        let underlying_weight = if underlying[3] == 0. {
            1.
        } else {
            1. - underlying[3] * (1. - self.underlying.weight_key(gray(underlying)))
        };
        source_weight * underlying_weight
    }
}

fn gray(pixel: [f64; 4]) -> u32 {
    let bytes = pixel.map(|v| (v.clamp(0., 1.) * 255. + 0.5).floor() as u32);
    299 * bytes[0] + 587 * bytes[1] + 114 * bytes[2]
}

mod document;
pub(crate) use document::validate_document;

#[cfg(test)]
mod tests;
