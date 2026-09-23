//! Color-family and tonal-range adjustments, matching upstream's sRGB formulas.
use super::hsl_to_rgb;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BlackWhite {
    pub reds: f64,
    pub yellows: f64,
    pub greens: f64,
    pub cyans: f64,
    pub blues: f64,
    pub magentas: f64,
    pub tint: bool,
    pub tint_hue: f64,
    pub tint_saturation: f64,
}
impl Default for BlackWhite {
    fn default() -> Self {
        Self {
            reds: 40.,
            yellows: 60.,
            greens: 40.,
            cyans: 60.,
            blues: 20.,
            magentas: 80.,
            tint: false,
            tint_hue: 40.,
            tint_saturation: 20.,
        }
    }
}
impl BlackWhite {
    pub fn weights(self) -> [f64; 6] {
        [
            self.reds,
            self.yellows,
            self.greens,
            self.cyans,
            self.blues,
            self.magentas,
        ]
    }
    pub(super) fn valid(self) -> bool {
        self.weights()
            .into_iter()
            .all(|v| (-200. ..=300.).contains(&v))
            && (0. ..=360.).contains(&self.tint_hue)
            && (0. ..=100.).contains(&self.tint_saturation)
    }
    pub(super) fn apply(self, rgb: [f64; 3]) -> [f64; 3] {
        let [r, g, b] = rgb;
        let high = r.max(g).max(b);
        let low = r.min(g).min(b);
        let mid = r + g + b - high - low;
        let (primary, secondary) = if high == r {
            (0, if g >= b { 1 } else { 5 })
        } else if high == g {
            (2, if r >= b { 1 } else { 3 })
        } else {
            (4, if g >= r { 3 } else { 5 })
        };
        let w = self.weights();
        let gray = (low + (mid - low) * w[secondary] / 100. + (high - mid) * w[primary] / 100.)
            .clamp(0., 1.);
        if self.tint {
            hsl_to_rgb([
                self.tint_hue.rem_euclid(360.),
                self.tint_saturation / 100.,
                gray,
            ])
        } else {
            [gray; 3]
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ColorBalance {
    pub shadow_cyan_red: f64,
    pub shadow_magenta_green: f64,
    pub shadow_yellow_blue: f64,
    pub mid_cyan_red: f64,
    pub mid_magenta_green: f64,
    pub mid_yellow_blue: f64,
    pub highlight_cyan_red: f64,
    pub highlight_magenta_green: f64,
    pub highlight_yellow_blue: f64,
    pub preserve_luminosity: bool,
}
impl Default for ColorBalance {
    fn default() -> Self {
        Self {
            shadow_cyan_red: 0.,
            shadow_magenta_green: 0.,
            shadow_yellow_blue: 0.,
            mid_cyan_red: 0.,
            mid_magenta_green: 0.,
            mid_yellow_blue: 0.,
            highlight_cyan_red: 0.,
            highlight_magenta_green: 0.,
            highlight_yellow_blue: 0.,
            preserve_luminosity: true,
        }
    }
}
impl ColorBalance {
    pub fn ranges(self) -> [[f64; 3]; 3] {
        [
            [
                self.shadow_cyan_red,
                self.shadow_magenta_green,
                self.shadow_yellow_blue,
            ],
            [
                self.mid_cyan_red,
                self.mid_magenta_green,
                self.mid_yellow_blue,
            ],
            [
                self.highlight_cyan_red,
                self.highlight_magenta_green,
                self.highlight_yellow_blue,
            ],
        ]
    }
    pub(super) fn valid(self) -> bool {
        self.ranges()
            .into_iter()
            .flatten()
            .all(|v| (-100. ..=100.).contains(&v))
    }
    pub(super) fn apply(self, rgb: [f64; 3]) -> [f64; 3] {
        let [shadows, midtones, highlights] = self.ranges();
        let mut out = std::array::from_fn(|i| {
            let v = rgb[i];
            let s = ((v - 0.333) / -0.25 + 0.5).clamp(0., 1.) * 0.7;
            let h = ((v + 0.333 - 1.) / 0.25 + 0.5).clamp(0., 1.) * 0.7;
            let m = ((v - 0.333) / 0.25 + 0.5).clamp(0., 1.)
                * ((v + 0.333 - 1.) / -0.25 + 0.5).clamp(0., 1.)
                * 0.7;
            (v + (shadows[i] * s + midtones[i] * m + highlights[i] * h) / 100.).clamp(0., 1.)
        });
        let lum = |c: [f64; 3]| c[0] * 0.299 + c[1] * 0.587 + c[2] * 0.114;
        if self.preserve_luminosity && lum(out) > 0.0001 {
            let ratio = lum(rgb) / lum(out);
            out = out.map(|v| (v * ratio).clamp(0., 1.));
        }
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::adjustment::rgb_to_hsl;
    #[test]
    fn color_adjustments_round_trip_validate_and_keep_alpha() {
        use crate::adjustment::{Adjustment, Kind};
        for kind in [Kind::BlackWhite, Kind::ColorBalance, Kind::Invert] {
            let a = Adjustment::new(kind);
            a.validate().unwrap();
            let json = serde_json::to_string(&a).unwrap();
            assert_eq!(serde_json::from_str::<Adjustment>(&json).unwrap(), a);
            assert_eq!(a.apply([0.2, 0.4, 0.6, 0.25], [0., 0.])[3], 0.25);
        }
        assert!(
            !BlackWhite {
                reds: f64::NAN,
                ..Default::default()
            }
            .valid()
        );
        assert!(
            !ColorBalance {
                mid_cyan_red: 101.,
                ..Default::default()
            }
            .valid()
        );
    }
    #[test]
    fn black_white_weights_and_tint_preserve_lightness() {
        let settings = BlackWhite::default();
        for (rgb, gray) in [
            ([1., 0., 0.], 0.4),
            ([1., 1., 0.], 0.6),
            ([0., 0., 1.], 0.2),
            ([0.5; 3], 0.5),
        ] {
            assert_eq!(settings.apply(rgb), [gray; 3]);
        }
        let tinted = BlackWhite {
            tint: true,
            ..settings
        }
        .apply([1., 0., 0.]);
        assert!((rgb_to_hsl(tinted)[2] - 0.4).abs() < 1e-10);
    }
    #[test]
    fn color_balance_is_identity_by_default_and_targets_tones() {
        let rgb = [0.2, 0.4, 0.7];
        assert_eq!(ColorBalance::default().apply(rgb), rgb);
        let settings = ColorBalance {
            shadow_cyan_red: 100.,
            preserve_luminosity: false,
            ..Default::default()
        };
        assert!(settings.apply([0.1; 3])[0] > 0.5);
        assert_eq!(settings.apply([0.9; 3]), [0.9; 3]);
    }
}
