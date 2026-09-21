use crate::{Result, invalid};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    #[serde(rename = "Hue/Saturation")]
    HueSaturation,
    Levels,
    Curves,
    Exposure,
    #[serde(rename = "Gradient Map")]
    GradientMap,
    Grain,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Adjustment {
    pub kind: Kind,
    #[serde(default)]
    pub hue: f64,
    #[serde(default)]
    pub saturation: f64,
    #[serde(default)]
    pub lightness: f64,
    #[serde(default)]
    pub colorize: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hsv_settings: Option<HueSaturation>,
    #[serde(default)]
    pub levels: Levels,
    #[serde(default)]
    pub curves: Curves,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure_settings: Option<Exposure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gradient_map_settings: Option<GradientMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain_settings: Option<Grain>,
}

impl Adjustment {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            hue: 0.,
            saturation: 0.,
            lightness: 0.,
            colorize: false,
            hsv_settings: None,
            levels: Levels::default(),
            curves: Curves::default(),
            exposure_settings: None,
            gradient_map_settings: None,
            grain_settings: None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        let exposure = self.exposure_settings.unwrap_or_default();
        let grain = self.grain_settings.unwrap_or_default();
        if !self.hue.is_finite()
            || self.hue.abs() > 360.
            || !self.saturation.is_finite()
            || self.saturation.abs() > 100.
            || !self.lightness.is_finite()
            || self.lightness.abs() > 100.
            || !self.levels.ranges.iter().all(LevelRange::valid)
            || !self.curves.valid()
            || !(-20. ..=20.).contains(&exposure.exposure)
            || !(-0.5..=0.5).contains(&exposure.offset)
            || !(0.01..=9.99).contains(&exposure.gamma)
            || !(0. ..=100.).contains(&grain.amount)
            || !(0.5..=20.).contains(&grain.size)
            || !(0. ..=100.).contains(&grain.roughness)
        {
            return Err(invalid(
                "Adjustment settings are outside their supported ranges.",
            ));
        }
        if let Some(map) = self.gradient_map_settings
            && (!map.shadows.valid() || !map.highlights.valid())
        {
            return Err(invalid("Gradient map colors must be between 0 and 1."));
        }
        if let Some(hsv) = &self.hsv_settings
            && (hsv.adjustments.iter().any(|(_, a)| {
                !a.hue.is_finite()
                    || a.hue.abs() > 360.
                    || !a.saturation.is_finite()
                    || a.saturation.abs() > 100.
                    || !a.lightness.is_finite()
                    || a.lightness.abs() > 100.
            }) || hsv.bands.iter().any(|(_, b)| {
                [b.falloff_start, b.range_start, b.range_end, b.falloff_end]
                    .iter()
                    .any(|v| !v.is_finite())
            }))
        {
            return Err(invalid("Hue/Saturation ranges contain invalid values."));
        }
        Ok(())
    }
    pub fn apply(&self, rgba: [f64; 4], point: [f64; 2]) -> [f64; 4] {
        let rgb = [rgba[0], rgba[1], rgba[2]];
        let out = match self.kind {
            Kind::HueSaturation => {
                let fallback = HueSaturation {
                    adjustments: vec![(
                        ColorRange::Master,
                        RangeAdjustment {
                            hue: self.hue,
                            saturation: self.saturation,
                            lightness: self.lightness,
                        },
                    )],
                    colorize: self.colorize,
                    ..HueSaturation::default()
                };
                self.hsv_settings.as_ref().unwrap_or(&fallback).apply(rgb)
            }
            Kind::Levels => std::array::from_fn(|i| {
                self.levels.ranges[0].apply(self.levels.ranges[i + 1].apply(rgb[i]))
            }),
            Kind::Curves => std::array::from_fn(|i| {
                self.curves
                    .value(self.curves.value(rgb[i] * 255., i + 1), 0)
                    / 255.
            }),
            Kind::Exposure => {
                let settings = self.exposure_settings.unwrap_or_default();
                rgb.map(|v| {
                    let linear = if v <= 0.04045 {
                        v / 12.92
                    } else {
                        ((v + 0.055) / 1.055).powf(2.4)
                    };
                    let value = (linear * 2_f64.powf(settings.exposure) + settings.offset)
                        .max(0.)
                        .powf(1. / settings.gamma);
                    if value <= 0.0031308 {
                        value * 12.92
                    } else {
                        1.055 * value.powf(1. / 2.4) - 0.055
                    }
                })
            }
            Kind::GradientMap => {
                let s = self.gradient_map_settings.unwrap_or_default();
                let mut t = luminance(rgb);
                if s.reversed {
                    t = 1. - t;
                }
                let a = s.shadows.rgb();
                let b = s.highlights.rgb();
                std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
            }
            Kind::Grain => {
                let s = self.grain_settings.unwrap_or_default();
                let x = point[0] / s.size;
                let y = point[1] / s.size;
                let ix = x.floor() as i64;
                let iy = y.floor() as i64;
                let tx = x - x.floor();
                let ty = y - y.floor();
                let tx = tx * tx * (3. - 2. * tx);
                let ty = ty * ty * (3. - 2. * ty);
                let a = lattice(ix, iy, s.seed);
                let b = lattice(ix + 1, iy, s.seed);
                let c = lattice(ix, iy + 1, s.seed);
                let d = lattice(ix + 1, iy + 1, s.seed);
                let smooth = ((a + (b - a) * tx) * (1. - ty) + (c + (d - c) * tx) * ty) * 1.6;
                let fine = lattice(
                    point[0].floor() as i64,
                    point[1].floor() as i64,
                    mix32(s.seed ^ 0xA511E9B3),
                );
                let noise = smooth + (fine - smooth) * s.roughness / 100.;
                let level = luminance(rgb);
                let delta = noise * s.amount / 100. * 0.35 * (0.4 + 2.4 * level * (1. - level));
                rgb.map(|v| v + delta)
            }
        };
        [
            out[0].clamp(0., 1.),
            out[1].clamp(0., 1.),
            out[2].clamp(0., 1.),
            rgba[3],
        ]
    }
}

pub fn luminance(rgb: [f64; 3]) -> f64 {
    rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722
}
pub fn mix32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^ (x >> 16)
}
fn lattice(x: i64, y: i64, seed: u32) -> f64 {
    let h = mix32(
        (x as u32).wrapping_mul(0x9E3779B1) ^ mix32((y as u32).wrapping_mul(0x85EBCA77) ^ seed),
    );
    f64::from(h & 65535) / 65535. + f64::from(h >> 16) / 65535. - 1.
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Channel {
    #[default]
    RGB,
    Red,
    Green,
    Blue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Levels {
    pub channel: Channel,
    pub ranges: [LevelRange; 4],
}
impl Default for Levels {
    fn default() -> Self {
        Self {
            channel: Channel::RGB,
            ranges: [LevelRange::default(); 4],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelRange {
    pub black: f64,
    pub gamma: f64,
    pub white: f64,
    pub output_black: f64,
    pub output_white: f64,
}
impl Default for LevelRange {
    fn default() -> Self {
        Self {
            black: 0.,
            gamma: 1.,
            white: 255.,
            output_black: 0.,
            output_white: 255.,
        }
    }
}
impl LevelRange {
    fn valid(&self) -> bool {
        (0. ..=254.).contains(&self.black)
            && (self.black + 1. ..=255.).contains(&self.white)
            && (0.1..=9.99).contains(&self.gamma)
            && (0. ..=255.).contains(&self.output_black)
            && (0. ..=255.).contains(&self.output_white)
    }
    fn apply(&self, value: f64) -> f64 {
        let input = ((value * 255. - self.black) / (self.white - self.black)).clamp(0., 1.);
        (self.output_black + input.powf(1. / self.gamma) * (self.output_white - self.output_black))
            / 255.
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curves {
    pub channel: Channel,
    pub channels: [Vec<CurvePoint>; 4],
}
impl Default for Curves {
    fn default() -> Self {
        Self {
            channel: Channel::RGB,
            channels: std::array::from_fn(|_| {
                vec![CurvePoint { x: 0., y: 0. }, CurvePoint { x: 255., y: 255. }]
            }),
        }
    }
}
impl Curves {
    pub fn sample(&self, input: f64, channel: Channel) -> f64 {
        self.value(
            input,
            match channel {
                Channel::RGB => 0,
                Channel::Red => 1,
                Channel::Green => 2,
                Channel::Blue => 3,
            },
        )
    }
    fn valid(&self) -> bool {
        self.channels.iter().all(|p| {
            (2..=32).contains(&p.len())
                && p.first().is_some_and(|p| p.x == 0.)
                && p.last().is_some_and(|p| p.x == 255.)
                && p.iter()
                    .all(|p| (0. ..=255.).contains(&p.x) && (0. ..=255.).contains(&p.y))
                && p.windows(2).all(|p| p[0].x < p[1].x)
        })
    }
    fn value(&self, x: f64, channel: usize) -> f64 {
        let p = &self.channels[channel];
        let i = p
            .partition_point(|p| p.x <= x)
            .saturating_sub(1)
            .min(p.len() - 2);
        let slope = |j: usize| (p[j + 1].y - p[j].y) / (p[j + 1].x - p[j].x);
        let tangent = |j: usize| {
            if j == 0 {
                slope(0)
            } else if j == p.len() - 1 {
                slope(j - 1)
            } else if slope(j - 1) * slope(j) <= 0. {
                0.
            } else {
                2. / (1. / slope(j - 1) + 1. / slope(j))
            }
        };
        let h = p[i + 1].x - p[i].x;
        let t = ((x - p[i].x) / h).clamp(0., 1.);
        ((2. * t.powi(3) - 3. * t.powi(2) + 1.) * p[i].y
            + (t.powi(3) - 2. * t.powi(2) + t) * h * tangent(i)
            + (-2. * t.powi(3) + 3. * t.powi(2)) * p[i + 1].y
            + (t.powi(3) - t.powi(2)) * h * tangent(i + 1))
        .clamp(0., 255.)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Exposure {
    pub exposure: f64,
    pub offset: f64,
    pub gamma: f64,
}
impl Default for Exposure {
    fn default() -> Self {
        Self {
            exposure: 0.,
            offset: 0.,
            gamma: 1.,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Color {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
}
impl Color {
    pub fn rgb(self) -> [f64; 3] {
        [self.red, self.green, self.blue]
    }
    fn valid(self) -> bool {
        self.rgb().iter().all(|v| (0. ..=1.).contains(v))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientMap {
    pub shadows: Color,
    pub highlights: Color,
    pub reversed: bool,
}
impl Default for GradientMap {
    fn default() -> Self {
        Self {
            shadows: Color {
                red: 0.,
                green: 0.,
                blue: 0.,
            },
            highlights: Color {
                red: 1.,
                green: 1.,
                blue: 1.,
            },
            reversed: false,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grain {
    pub amount: f64,
    pub size: f64,
    pub roughness: f64,
    pub seed: u32,
}
impl Default for Grain {
    fn default() -> Self {
        Self {
            amount: 25.,
            size: 1.5,
            roughness: 50.,
            seed: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum ColorRange {
    #[default]
    Master,
    Reds,
    Yellows,
    Greens,
    Cyans,
    Blues,
    Magentas,
}
impl ColorRange {
    pub fn default_band(self) -> HueBand {
        let center: f64 = match self {
            Self::Master | Self::Reds => 0.,
            Self::Yellows => 60.,
            Self::Greens => 120.,
            Self::Cyans => 180.,
            Self::Blues => 240.,
            Self::Magentas => 300.,
        };
        if self == Self::Master {
            return HueBand {
                falloff_start: 0.,
                range_start: 0.,
                range_end: 360.,
                falloff_end: 360.,
            };
        }
        HueBand {
            falloff_start: (center - 45.).rem_euclid(360.),
            range_start: (center - 15.).rem_euclid(360.),
            range_end: (center + 15.).rem_euclid(360.),
            falloff_end: (center + 45.).rem_euclid(360.),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RangeAdjustment {
    pub hue: f64,
    pub saturation: f64,
    pub lightness: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HueBand {
    pub falloff_start: f64,
    pub range_start: f64,
    pub range_end: f64,
    pub falloff_end: f64,
}
impl HueBand {
    pub fn weight(self, hue: f64) -> f64 {
        let span = (self.falloff_end - self.falloff_start).rem_euclid(360.);
        if span == 0. {
            return 1.;
        }
        let p = (hue - self.falloff_start).rem_euclid(360.);
        let start = (self.range_start - self.falloff_start).rem_euclid(360.);
        let end = (self.range_end - self.falloff_start).rem_euclid(360.);
        if p > span {
            0.
        } else if p < start {
            p / start
        } else if p <= end {
            1.
        } else if span > end {
            (span - p) / (span - end)
        } else {
            1.
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HueSaturation {
    pub range: ColorRange,
    pub colorize: bool,
    #[serde(default)]
    pub invert_range: bool,
    #[serde(with = "swift_dictionary")]
    pub adjustments: Vec<(ColorRange, RangeAdjustment)>,
    #[serde(with = "swift_dictionary")]
    pub bands: Vec<(ColorRange, HueBand)>,
}
impl HueSaturation {
    pub fn band(&self, range: ColorRange) -> HueBand {
        self.bands
            .iter()
            .find(|(r, _)| *r == range)
            .map_or(range.default_band(), |(_, band)| *band)
    }

    pub fn weight(&self, range: ColorRange, hue: f64) -> f64 {
        if range == ColorRange::Master {
            return 1.;
        }
        let weight = self.band(range).weight(hue);
        if self.invert_range && range == self.range {
            1. - weight
        } else {
            weight
        }
    }

    pub fn shifted_hue(&self, hue: f64) -> f64 {
        let shift: f64 = self
            .adjustments
            .iter()
            .map(|(range, adjustment)| adjustment.hue * self.weight(*range, hue))
            .sum();
        (hue + shift).rem_euclid(360.)
    }

    fn apply(&self, rgb: [f64; 3]) -> [f64; 3] {
        let [mut hue, mut sat, mut light] = rgb_to_hsl(rgb);
        let lightness;
        if self.colorize {
            let adjustment = self
                .adjustments
                .iter()
                .find(|(range, _)| *range == self.range)
                .map_or(RangeAdjustment::default(), |(_, a)| *a);
            hue = adjustment.hue;
            sat = (adjustment.saturation / 100.).clamp(0., 1.);
            lightness = adjustment.lightness / 100.;
        } else {
            // Swift samples one response per degree, sums all ranges, then adjusts HSL once.
            let degree = hue.round();
            let mut response = RangeAdjustment::default();
            for (range, adjustment) in &self.adjustments {
                let weight = self.weight(*range, degree);
                response.hue += adjustment.hue * weight;
                response.saturation += adjustment.saturation * weight;
                response.lightness += adjustment.lightness * weight;
            }
            hue += response.hue;
            sat = (sat * (1. + response.saturation / 100.)).clamp(0., 1.);
            lightness = response.lightness / 100.;
        }
        let amount = lightness.clamp(-1., 1.);
        light = if amount >= 0. {
            light + (1. - light) * amount
        } else {
            light * (1. + amount)
        };
        hsl_to_rgb([hue.rem_euclid(360.), sat, light.clamp(0., 1.)])
    }
}

pub fn rgb_to_hsl(c: [f64; 3]) -> [f64; 3] {
    let max = c.into_iter().fold(0., f64::max);
    let min = c.into_iter().fold(1., f64::min);
    let delta = max - min;
    let light = (max + min) / 2.;
    if delta == 0. {
        return [0., 0., light];
    }
    let hue = if max == c[0] {
        ((c[1] - c[2]) / delta).rem_euclid(6.)
    } else if max == c[1] {
        (c[2] - c[0]) / delta + 2.
    } else {
        (c[0] - c[1]) / delta + 4.
    };
    [hue * 60., delta / (1. - (2. * light - 1.).abs()), light]
}
pub fn hsl_to_rgb([h, s, l]: [f64; 3]) -> [f64; 3] {
    let c = (1. - (2. * l - 1.).abs()) * s;
    let x = c * (1. - ((h / 60.).rem_euclid(2.) - 1.).abs());
    let rgb = match (h / 60.).floor() as u32 {
        0 => [c, x, 0.],
        1 => [x, c, 0.],
        2 => [0., c, x],
        3 => [0., x, c],
        4 => [x, 0., c],
        _ => [c, 0., x],
    };
    rgb.map(|v| v + l - c / 2.)
}

// Swift encodes dictionaries with enum keys as alternating key/value arrays.
mod swift_dictionary {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error, ser::SerializeSeq};
    pub fn serialize<S: Serializer, K: Serialize, V: Serialize>(
        pairs: &[(K, V)],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(pairs.len() * 2))?;
        for (key, value) in pairs {
            seq.serialize_element(key)?;
            seq.serialize_element(value)?;
        }
        seq.end()
    }
    pub fn deserialize<
        'de,
        D: Deserializer<'de>,
        K: serde::de::DeserializeOwned,
        V: serde::de::DeserializeOwned,
    >(
        deserializer: D,
    ) -> Result<Vec<(K, V)>, D::Error> {
        let values = Vec::<serde_json::Value>::deserialize(deserializer)?;
        if values.len() % 2 != 0 {
            return Err(D::Error::custom(
                "Expected alternating dictionary keys and values",
            ));
        }
        values
            .chunks_exact(2)
            .map(|pair| {
                Ok((
                    serde_json::from_value(pair[0].clone()).map_err(D::Error::custom)?,
                    serde_json::from_value(pair[1].clone()).map_err(D::Error::custom)?,
                ))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hue_saturation_keeps_neutrals_and_sums_overlapping_ranges_before_applying() {
        let mut settings = HueSaturation {
            adjustments: vec![(
                ColorRange::Master,
                RangeAdjustment {
                    saturation: 50.,
                    ..RangeAdjustment::default()
                },
            )],
            ..HueSaturation::default()
        };
        assert_eq!(settings.apply([0.5; 3]), [0.5; 3]);
        let adjusted = rgb_to_hsl(settings.apply(hsl_to_rgb([0., 0.2, 0.5])));
        assert!((adjusted[1] - 0.3).abs() < 1e-9);
        settings.adjustments = vec![
            (
                ColorRange::Master,
                RangeAdjustment {
                    saturation: 50.,
                    lightness: 50.,
                    ..RangeAdjustment::default()
                },
            ),
            (
                ColorRange::Reds,
                RangeAdjustment {
                    saturation: -50.,
                    lightness: -50.,
                    ..RangeAdjustment::default()
                },
            ),
        ];
        let original = hsl_to_rgb([0., 0.4, 0.5]);
        let result = settings.apply(original);
        for (a, b) in result.into_iter().zip(original) {
            assert!((a - b).abs() < 1e-9);
        }
        settings.adjustments.reverse();
        assert_eq!(settings.apply(original), result);
    }

    #[test]
    fn hue_inversion_only_changes_selected_range_and_colorize_ignores_other_ranges() {
        let mut settings = HueSaturation {
            range: ColorRange::Reds,
            invert_range: true,
            adjustments: vec![
                (
                    ColorRange::Reds,
                    RangeAdjustment {
                        hue: 60.,
                        ..RangeAdjustment::default()
                    },
                ),
                (
                    ColorRange::Blues,
                    RangeAdjustment {
                        hue: 20.,
                        ..RangeAdjustment::default()
                    },
                ),
            ],
            ..HueSaturation::default()
        };
        assert!((rgb_to_hsl(settings.apply(hsl_to_rgb([0., 0.5, 0.5])))[0]).abs() < 1e-9);
        assert!((rgb_to_hsl(settings.apply(hsl_to_rgb([240., 0.5, 0.5])))[0] - 320.).abs() < 1e-9);
        settings.colorize = true;
        settings.adjustments[0].1 = RangeAdjustment {
            hue: 120.,
            saturation: 25.,
            lightness: 20.,
        };
        let result = rgb_to_hsl(settings.apply([0.5; 3]));
        for (a, b) in result.into_iter().zip([120., 0.25, 0.6]) {
            assert!((a - b).abs() < 1e-9);
        }
    }
    #[test]
    fn neutral_adjustments_preserve_rgb_and_alpha() {
        for kind in [
            Kind::HueSaturation,
            Kind::Levels,
            Kind::Curves,
            Kind::Exposure,
        ] {
            let result = Adjustment::new(kind).apply([0.2, 0.6, 0.8, 0.4], [0., 0.]);
            for (a, b) in result.into_iter().zip([0.2, 0.6, 0.8, 0.4]) {
                assert!((a - b).abs() < 1e-9, "{kind:?}: {a} != {b}");
            }
        }
    }
    #[test]
    fn exposure_operates_in_linear_light() {
        let mut adjustment = Adjustment::new(Kind::Exposure);
        adjustment.exposure_settings = Some(Exposure {
            exposure: 1.,
            ..Exposure::default()
        });
        let output = adjustment.apply([0.5, 0.5, 0.5, 0.3], [0., 0.]);
        assert!((output[0] - 0.68584).abs() < 0.0001);
        assert_eq!(output[3], 0.3);
    }
}
