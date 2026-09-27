//! Linux authoring adjustments, persisted separately from the native v10 manifest.
use super::PhotoFilter;
use crate::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "settings", deny_unknown_fields)]
pub enum ExtendedAdjustment {
    PhotoFilter(PhotoFilter),
}
impl ExtendedAdjustment {
    pub fn label(self) -> &'static str {
        match self {
            Self::PhotoFilter(_) => "Photo Filter",
        }
    }
    pub fn validate(self) -> Result<()> {
        match self {
            Self::PhotoFilter(settings) => settings.validate(),
        }
    }
    /// Evaluate continuous backdrop RGB. Quantization belongs to the final output.
    pub fn apply_rgba(self, rgba: [f64; 4]) -> [f64; 4] {
        if rgba[3] == 0. {
            return rgba;
        }
        let input = [rgba[0] as f32, rgba[1] as f32, rgba[2] as f32];
        let rgb = match self {
            Self::PhotoFilter(settings) => settings.apply_rgb(input),
        };
        [
            f64::from(rgb[0]),
            f64::from(rgb[1]),
            f64::from(rgb[2]),
            rgba[3],
        ]
    }
}
