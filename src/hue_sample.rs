use crate::{
    Result,
    adjustment::{ColorRange, HueBand, HueSaturation, RangeAdjustment, rgb_to_hsl},
    invalid,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HueSample {
    Center,
    Include,
    Exclude,
}

pub fn sampled_hue(rgba: [f64; 4]) -> Option<f64> {
    if rgba
        .iter()
        .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        || rgba[3] == 0.
    {
        return None;
    }
    let rgb = [rgba[0], rgba[1], rgba[2]];
    let max = rgb.into_iter().fold(0., f64::max);
    let min = rgb.into_iter().fold(1., f64::min);
    (max > 0. && (max - min) / max > 0.02).then(|| rgb_to_hsl(rgb)[0])
}

fn forward(from: f64, to: f64) -> f64 {
    (to - from).rem_euclid(360.)
}

fn band(settings: &HueSaturation, range: ColorRange) -> HueBand {
    settings
        .bands
        .iter()
        .find(|(r, _)| *r == range)
        .map_or(range.default_band(), |(_, band)| *band)
}

impl HueSample {
    pub fn apply(self, settings: &mut HueSaturation, hue: f64) -> Result<()> {
        if !hue.is_finite() {
            return Err(invalid(
                "The sampled hue is invalid. Choose another image pixel.",
            ));
        }
        if settings.colorize || settings.range == ColorRange::Master {
            return Ok(());
        }
        let hue = hue.rem_euclid(360.);
        let mut value = band(settings, settings.range);
        let leading = forward(value.falloff_start, value.range_start);
        let trailing = forward(value.range_end, value.falloff_end);
        match self {
            Self::Center => {
                let core = forward(value.range_start, value.range_end);
                value.range_start = hue - core / 2.;
                value.range_end = value.range_start + core;
                value.falloff_start = value.range_start - leading;
                value.falloff_end = value.range_end + trailing;
            }
            Self::Include if value.weight(hue) < 1. => {
                if forward(hue, value.range_start) <= forward(value.range_end, hue) {
                    value.range_start = hue;
                    value.falloff_start = hue - leading;
                } else {
                    value.range_end = hue;
                    value.falloff_end = hue + trailing;
                }
            }
            Self::Exclude if value.weight(hue) > 0. => {
                if forward(value.falloff_start, hue) <= forward(hue, value.falloff_end) {
                    value.falloff_start = hue + 1.;
                    value.range_start = hue + 1. + leading;
                } else {
                    value.falloff_end = hue - 1.;
                    value.range_end = hue - 1. - trailing;
                }
            }
            _ => return Ok(()),
        }
        value.falloff_start = value.falloff_start.rem_euclid(360.);
        value.range_start = value.range_start.rem_euclid(360.);
        value.range_end = value.range_end.rem_euclid(360.);
        value.falloff_end = value.falloff_end.rem_euclid(360.);
        if self != Self::Center && forward(value.falloff_start, value.falloff_end) > 350. {
            value.falloff_end = (value.falloff_start + 350.).rem_euclid(360.);
        }
        if let Some((_, b)) = settings
            .bands
            .iter_mut()
            .find(|(r, _)| *r == settings.range)
        {
            *b = value;
        } else {
            settings.bands.push((settings.range, value));
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct HueTarget {
    pub range: ColorRange,
    original: RangeAdjustment,
}

impl HueTarget {
    pub fn begin(settings: &mut HueSaturation, hue: f64) -> Option<Self> {
        if settings.colorize || !hue.is_finite() {
            return None;
        }
        let ranges = [
            ColorRange::Reds,
            ColorRange::Yellows,
            ColorRange::Greens,
            ColorRange::Cyans,
            ColorRange::Blues,
            ColorRange::Magentas,
        ];
        let mut range = ColorRange::Reds;
        let weight = |range| settings.weight(range, hue);
        for candidate in ranges.into_iter().skip(1) {
            if weight(candidate) > weight(range) {
                range = candidate;
            }
        }
        settings.range = range;
        let original = settings
            .adjustments
            .iter()
            .find(|(r, _)| *r == range)
            .map_or(RangeAdjustment::default(), |(_, a)| *a);
        Some(Self { range, original })
    }

    pub fn update(self, settings: &mut HueSaturation, view_delta: f64, hue: bool) -> Result<()> {
        if !view_delta.is_finite() {
            return Err(invalid(
                "The color adjustment drag is outside supported coordinates.",
            ));
        }
        let adjustment = if let Some((_, a)) = settings
            .adjustments
            .iter_mut()
            .find(|(r, _)| *r == self.range)
        {
            a
        } else {
            settings.adjustments.push((self.range, self.original));
            &mut settings
                .adjustments
                .last_mut()
                .ok_or_else(|| invalid("The targeted color range is missing."))?
                .1
        };
        if hue {
            adjustment.hue = (self.original.hue + view_delta / 2.).clamp(-180., 180.);
        } else {
            adjustment.saturation = (self.original.saturation + view_delta / 2.).clamp(-100., 100.);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eyedroppers_center_widen_and_narrow_wrapped_bands() {
        let mut settings = HueSaturation {
            range: ColorRange::Reds,
            ..HueSaturation::default()
        };
        HueSample::Center.apply(&mut settings, 350.).unwrap();
        assert_eq!(band(&settings, ColorRange::Reds).range_start, 335.);
        HueSample::Include.apply(&mut settings, 30.).unwrap();
        assert_eq!(band(&settings, ColorRange::Reds).weight(30.), 1.);
        HueSample::Exclude.apply(&mut settings, 30.).unwrap();
        assert_eq!(band(&settings, ColorRange::Reds).weight(30.), 0.);
        assert_eq!(sampled_hue([0.5, 0.5, 0.5, 1.]), None);
        assert_eq!(sampled_hue([1., 0., 0., 0.]), None);
        assert_eq!(sampled_hue([0., 1., 0., 1.]), Some(120.));
    }
    #[test]
    fn targeted_drag_uses_original_values_and_clamps_at_limits() {
        let mut settings = HueSaturation::default();
        let drag = HueTarget::begin(&mut settings, 120.).unwrap();
        assert_eq!(settings.range, ColorRange::Greens);
        drag.update(&mut settings, 30., false).unwrap();
        drag.update(&mut settings, 30., false).unwrap();
        assert_eq!(settings.adjustments[0].1.saturation, 15.);
        drag.update(&mut settings, -1000., true).unwrap();
        assert_eq!(settings.adjustments[0].1.hue, -180.);
        settings.colorize = true;
        assert!(HueTarget::begin(&mut settings, 120.).is_none());
    }
}
