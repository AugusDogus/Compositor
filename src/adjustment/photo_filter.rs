//! Encoded-RGB color filtration with optional weighted-luminance preservation.
use crate::{Result, invalid};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PhotoFilter {
    /// Filter color in encoded RGB, from zero to one.
    pub color: [f64; 3],
    /// Strength as a percentage, from zero to 100.
    pub density: f64,
    pub preserve_luminosity: bool,
}
impl Default for PhotoFilter {
    fn default() -> Self {
        Self {
            color: [236. / 255., 138. / 255., 0.],
            density: 25.,
            preserve_luminosity: true,
        }
    }
}
impl PhotoFilter {
    pub fn validate(self) -> Result<()> {
        if !(0. ..=100.).contains(&self.density)
            || self.color.iter().any(|value| !(0. ..=1.).contains(value))
        {
            return Err(invalid(
                "Photo Filter density must be between 0 and 100, and each color channel between 0 and 1. The original pixels are preserved.",
            ));
        }
        Ok(())
    }

    pub(crate) fn identity(self) -> bool {
        self.density == 0. || self.color == [1.; 3]
    }

    /// Match the GPU's f32 arithmetic. Alpha and invisible RGB are untouched.
    pub(crate) fn pixel(self, source: [u8; 4]) -> [u8; 4] {
        if source[3] == 0 || self.identity() {
            return source;
        }
        let input = source.map(|value| f32::from(value) / 255.);
        let density = (self.density / 100.) as f32;
        let mut rgb = std::array::from_fn::<_, 3, _>(|i| {
            input[i] * (1. - density) + input[i] * self.color[i] as f32 * density
        });
        if self.preserve_luminosity {
            let before = input[0] * 0.299 + input[1] * 0.587 + input[2] * 0.114;
            let after = rgb[0] * 0.299 + rgb[1] * 0.587 + rgb[2] * 0.114;
            if after > 1e-6 {
                rgb = rgb.map(|value| value * (before / after));
            }
        }
        let mut output = source;
        for i in 0..3 {
            output[i] = (rgb[i].clamp(0., 1.) * 255.).round() as u8;
        }
        output
    }
}
