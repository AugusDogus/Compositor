//! Each encoded-RGB output is a weighted sum of input channels plus a constant.
use crate::{Result, invalid};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelMixer {
    /// Red, green, and blue output rows: input RGB percentages, then constant.
    pub rows: [[f64; 4]; 3],
    /// Use the red output row for all three channels, retaining the other rows.
    pub monochrome: bool,
}
impl Default for ChannelMixer {
    fn default() -> Self {
        Self {
            rows: [[100., 0., 0., 0.], [0., 100., 0., 0.], [0., 0., 100., 0.]],
            monochrome: false,
        }
    }
}
impl ChannelMixer {
    pub fn validate(self) -> Result<()> {
        if self
            .rows
            .iter()
            .flatten()
            .any(|v| !(-200. ..=200.).contains(v))
        {
            return Err(invalid(
                "Channel Mixer coefficients and constants must be between -200% and 200%. The original pixels are preserved.",
            ));
        }
        Ok(())
    }
    pub(crate) fn identity(self) -> bool {
        self == Self::default()
    }
    pub(crate) fn coefficients(self) -> [[f32; 4]; 3] {
        std::array::from_fn(|channel| {
            self.rows[if self.monochrome { 0 } else { channel }].map(|value| (value / 100.) as f32)
        })
    }
    pub(crate) fn pixel(self, source: [u8; 4]) -> [u8; 4] {
        if source[3] == 0 || self.identity() {
            return source;
        }
        let input = [
            f32::from(source[0]) / 255.,
            f32::from(source[1]) / 255.,
            f32::from(source[2]) / 255.,
        ];
        let mut output = source;
        for (channel, row) in self.coefficients().into_iter().enumerate() {
            let value = input[0] * row[0] + input[1] * row[1] + input[2] * row[2] + row[3];
            output[channel] = (value.clamp(0., 1.) * 255.).round() as u8;
        }
        output
    }
}
