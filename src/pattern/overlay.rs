use super::Pattern;
use crate::{Result, invalid};
use serde::{Deserialize, Serialize};

/// A Normal overlay attached to the source pixel grid. The layer transform
/// moves, rotates, and scales the tiled appearance with its source pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub scale: f64,
    pub opacity: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            scale: 1.,
            opacity: 1.,
        }
    }
}

impl Settings {
    pub fn validate(self) -> Result<()> {
        if !self.scale.is_finite()
            || !(0.05..=20.).contains(&self.scale)
            || !self.opacity.is_finite()
            || !(0. ..=1.).contains(&self.opacity)
        {
            return Err(invalid(
                "Pattern Overlay requires scale between 5% and 2000% and opacity between 0% and 100%. The layer is unchanged.",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Overlay {
    pub pattern: Pattern,
    pub settings: Settings,
}

impl Overlay {
    pub fn new(pattern: Pattern) -> Self {
        Self {
            pattern,
            settings: Settings::default(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.settings.validate()
    }

    /// Repeat with premultiplied bilinear interpolation. The caller validates
    /// settings once and supplies finite source-pixel coordinates.
    pub(crate) fn sample(&self, point: [f64; 2]) -> [f64; 4] {
        let tile = self.pattern.pixels();
        let p = point.map(|v| v / self.settings.scale - 0.5);
        let start = p.map(f64::floor);
        let fraction = [p[0] - start[0], p[1] - start[1]];
        let mut sum = [0.; 4];
        for (dx, dy, weight) in [
            (0., 0., (1. - fraction[0]) * (1. - fraction[1])),
            (1., 0., fraction[0] * (1. - fraction[1])),
            (0., 1., (1. - fraction[0]) * fraction[1]),
            (1., 1., fraction[0] * fraction[1]),
        ] {
            let x = (start[0] + dx).rem_euclid(f64::from(tile.width())) as u32;
            let y = (start[1] + dy).rem_euclid(f64::from(tile.height())) as u32;
            let rgba = tile[(x, y)].0.map(|v| f64::from(v) / 255.);
            for axis in 0..3 {
                sum[axis] += rgba[axis] * rgba[3] * weight;
            }
            sum[3] += rgba[3] * weight;
        }
        if sum[3] > 0. {
            let alpha = sum[3];
            for value in &mut sum[..3] {
                *value /= alpha;
            }
        }
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    #[test]
    fn sampling_repeats_at_negative_coordinates_and_preserves_alpha_edges() {
        let pixels = RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 0, 255, 0])
            }
        });
        let overlay = Overlay::new(Pattern::from_pixels("Tile", pixels).unwrap());
        assert_eq!(overlay.sample([0.5, 0.5]), [1., 0., 0., 1.]);
        assert_eq!(overlay.sample([-1.5, 0.5]), [1., 0., 0., 1.]);
        assert_eq!(overlay.sample([1., 0.5]), [1., 0., 0., 0.5]);
        assert_eq!(overlay.sample([2.5, 0.5]), [1., 0., 0., 1.]);
    }
    #[test]
    fn scale_changes_repeat_period_and_invalid_settings_are_rejected() {
        let pixels = RgbaImage::from_fn(2, 1, |x, _| Rgba([x as u8 * 255, 0, 0, 255]));
        let mut overlay = Overlay::new(Pattern::from_pixels("Tile", pixels).unwrap());
        overlay.settings.scale = 2.;
        assert_eq!(overlay.sample([1., 1.]), [0., 0., 0., 1.]);
        assert_eq!(overlay.sample([3., 1.]), [1., 0., 0., 1.]);
        for value in [0., f64::NAN, f64::INFINITY, 20.1] {
            overlay.settings.scale = value;
            assert!(overlay.validate().is_err());
        }
    }
}
