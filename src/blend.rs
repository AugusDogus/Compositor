use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Blend {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    #[serde(rename = "Soft Light")]
    SoftLight,
    Darken,
    Lighten,
    Difference,
    #[serde(rename = "Color Dodge")]
    ColorDodge,
    #[serde(rename = "Color Burn")]
    ColorBurn,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl Blend {
    pub const ALL: [Self; 14] = [
        Self::Normal,
        Self::Multiply,
        Self::Screen,
        Self::Overlay,
        Self::SoftLight,
        Self::Darken,
        Self::Lighten,
        Self::Difference,
        Self::ColorDodge,
        Self::ColorBurn,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Overlay => "Overlay",
            Self::SoftLight => "Soft Light",
            Self::Darken => "Darken",
            Self::Lighten => "Lighten",
            Self::Difference => "Difference",
            Self::ColorDodge => "Color Dodge",
            Self::ColorBurn => "Color Burn",
            Self::Hue => "Hue",
            Self::Saturation => "Saturation",
            Self::Color => "Color",
            Self::Luminosity => "Luminosity",
        }
    }

    pub fn composite(self, bottom: [f64; 4], top: [f64; 4]) -> [f64; 4] {
        let alpha = top[3] + bottom[3] * (1. - top[3]);
        if alpha <= 0. {
            return [0.; 4];
        }
        let b = [bottom[0], bottom[1], bottom[2]];
        let s = [top[0], top[1], top[2]];
        let blend = match self {
            Self::Hue => set_lum(set_sat(s, sat(b)), lum(b)),
            Self::Saturation => set_lum(set_sat(b, sat(s)), lum(b)),
            Self::Color => set_lum(s, lum(b)),
            Self::Luminosity => set_lum(b, lum(s)),
            _ => std::array::from_fn(|i| match self {
                Self::Multiply => b[i] * s[i],
                Self::Screen => b[i] + s[i] - b[i] * s[i],
                Self::Overlay => {
                    if b[i] <= 0.5 {
                        2. * b[i] * s[i]
                    } else {
                        1. - 2. * (1. - b[i]) * (1. - s[i])
                    }
                }
                Self::SoftLight => {
                    // PDF separable blend formula, evaluated in the document's sRGB space.
                    if s[i] <= 0.5 {
                        b[i] - (1. - 2. * s[i]) * b[i] * (1. - b[i])
                    } else {
                        let d = if b[i] <= 0.25 {
                            ((16. * b[i] - 12.) * b[i] + 4.) * b[i]
                        } else {
                            b[i].sqrt()
                        };
                        b[i] + (2. * s[i] - 1.) * (d - b[i])
                    }
                }
                Self::Darken => b[i].min(s[i]),
                Self::Lighten => b[i].max(s[i]),
                Self::Difference => (b[i] - s[i]).abs(),
                Self::ColorDodge => {
                    if b[i] == 0. {
                        0.
                    } else if s[i] == 1. {
                        1.
                    } else {
                        (b[i] / (1. - s[i])).min(1.)
                    }
                }
                Self::ColorBurn => {
                    if b[i] == 1. {
                        1.
                    } else if s[i] == 0. {
                        0.
                    } else {
                        1. - ((1. - b[i]) / s[i]).min(1.)
                    }
                }
                _ => s[i],
            }),
        };
        let mut out = [0., 0., 0., alpha];
        for i in 0..3 {
            out[i] = (top[3] * ((1. - bottom[3]) * s[i] + bottom[3] * blend[i])
                + bottom[3] * (1. - top[3]) * b[i])
                / alpha;
        }
        out
    }
}

fn lum(c: [f64; 3]) -> f64 {
    c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11
}
fn sat(c: [f64; 3]) -> f64 {
    c.into_iter().fold(0., f64::max) - c.into_iter().fold(1., f64::min)
}
fn set_lum(mut c: [f64; 3], target: f64) -> [f64; 3] {
    let diff = target - lum(c);
    c.iter_mut().for_each(|v| *v += diff);
    let low = c.into_iter().fold(f64::INFINITY, f64::min);
    let high = c.into_iter().fold(f64::NEG_INFINITY, f64::max);
    if low < 0. {
        c.iter_mut()
            .for_each(|v| *v = target + (*v - target) * target / (target - low));
    }
    if high > 1. {
        c.iter_mut()
            .for_each(|v| *v = target + (*v - target) * (1. - target) / (high - target));
    }
    c
}
fn set_sat(mut c: [f64; 3], value: f64) -> [f64; 3] {
    let mut indices = [0, 1, 2];
    indices.sort_by(|a, b| c[*a].total_cmp(&c[*b]));
    let [low, middle, high] = indices;
    if c[high] > c[low] {
        c[middle] = (c[middle] - c[low]) * value / (c[high] - c[low]);
        c[high] = value;
    } else {
        c[middle] = 0.;
        c[high] = 0.;
    }
    c[low] = 0.;
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn soft_light_uses_pdf_curve_and_straight_alpha() {
        for (bottom, top, expected) in [
            (0.4, 0.0, 0.16),
            (0.4, 0.5, 0.4),
            (0.04, 1.0, 0.141824),
            (0.25, 1.0, 0.5),
            (0.81, 1.0, 0.9),
        ] {
            let actual =
                Blend::SoftLight.composite([bottom, bottom, bottom, 1.], [top, top, top, 1.]);
            assert!(
                (actual[0] - expected).abs() < 1e-10,
                "{bottom}, {top}: {actual:?}"
            );
            assert_eq!(actual[3], 1.);
        }
        let actual = Blend::SoftLight.composite([0.25, 0.25, 0.25, 0.5], [1., 1., 1., 0.5]);
        assert_eq!(actual, [7. / 12., 7. / 12., 7. / 12., 0.75]);
        assert_eq!(
            serde_json::to_string(&Blend::SoftLight).unwrap(),
            "\"Soft Light\""
        );
        assert_eq!(
            serde_json::from_str::<Blend>("\"Soft Light\"").unwrap(),
            Blend::SoftLight
        );
    }
    #[test]
    fn transparent_backdrop_keeps_source_color_for_every_mode() {
        for mode in Blend::ALL {
            assert_eq!(
                mode.composite([0.; 4], [0.2, 0.6, 0.8, 0.5]),
                [0.2, 0.6, 0.8, 0.5]
            );
        }
    }
    #[test]
    fn source_over_uses_straight_alpha() {
        assert_eq!(
            Blend::Normal.composite([1., 0., 0., 1.], [0., 0., 1., 0.5]),
            [0.5, 0., 0.5, 1.]
        );
        assert_eq!(
            Blend::Multiply.composite([0.5, 0.5, 0.5, 1.], [0.5, 0.5, 0.5, 1.]),
            [0.25, 0.25, 0.25, 1.]
        );
    }
}
