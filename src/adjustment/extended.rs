//! Linux authoring adjustments, persisted separately from the native v10 manifest.
use super::{ChannelMixer, PhotoFilter};
use crate::Result;
use crate::selective_color::SelectiveColor;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "settings", deny_unknown_fields)]
pub enum ExtendedAdjustment {
    PhotoFilter(PhotoFilter),
    ChannelMixer(ChannelMixer),
    SelectiveColor(SelectiveColor),
    Threshold(crate::threshold::Threshold),
    Posterize(crate::posterize::Posterize),
}
impl ExtendedAdjustment {
    pub fn label(self) -> &'static str {
        match self {
            Self::PhotoFilter(_) => "Photo Filter",
            Self::ChannelMixer(_) => "Channel Mixer",
            Self::SelectiveColor(_) => "Selective Color",
            Self::Threshold(_) => "Threshold",
            Self::Posterize(_) => "Posterize",
        }
    }
    pub fn validate(self) -> Result<()> {
        match self {
            Self::PhotoFilter(settings) => settings.validate(),
            Self::ChannelMixer(settings) => settings.validate(),
            Self::SelectiveColor(settings) => settings.validate(),
            Self::Threshold(_) | Self::Posterize(_) => Ok(()),
        }
    }
    /// Evaluate continuous backdrop RGB. Quantization belongs to the final output.
    pub fn apply_rgba(self, rgba: [f64; 4]) -> [f64; 4] {
        if rgba[3] == 0. || matches!(self, Self::Posterize(settings) if settings.levels() == 256) {
            return rgba;
        }
        let input = [rgba[0] as f32, rgba[1] as f32, rgba[2] as f32];
        let rgb = match self {
            Self::PhotoFilter(settings) => settings.apply_rgb(input),
            Self::ChannelMixer(settings) => settings.apply_rgb(input),
            Self::SelectiveColor(settings) => settings.apply_rgb(input),
            Self::Threshold(settings) => settings.apply_rgb(input),
            Self::Posterize(settings) => settings.apply_rgb(input),
        };
        [
            f64::from(rgb[0]),
            f64::from(rgb[1]),
            f64::from(rgb[2]),
            rgba[3],
        ]
    }
}
