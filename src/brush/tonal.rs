//! Tonal painting changes existing RGB, retaining alpha and stroke coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    All,
    Shadows,
    Midtones,
    Highlights,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tonal {
    Dodge(Range),
    Burn(Range),
    Saturate,
    Desaturate,
}

impl Tonal {
    pub(super) fn apply(self, before: [f64; 4], strength: f64) -> [f64; 4] {
        if before[3] == 0. || strength == 0. {
            return before;
        }
        let luminance = before[0] * 0.2126 + before[1] * 0.7152 + before[2] * 0.0722;
        let mut result = before;
        match self {
            Self::Dodge(range) | Self::Burn(range) => {
                let exposure = strength
                    * range.weight(luminance)
                    * if matches!(self, Self::Burn(_)) {
                        -1.
                    } else {
                        1.
                    };
                for channel in 0..3 {
                    let linear = if before[channel] <= 0.04045 {
                        before[channel] / 12.92
                    } else {
                        ((before[channel] + 0.055) / 1.055).powf(2.4)
                    };
                    let linear = (linear * exposure.exp2()).clamp(0., 1.);
                    result[channel] = if linear <= 0.0031308 {
                        linear * 12.92
                    } else {
                        1.055 * linear.powf(1. / 2.4) - 0.055
                    };
                }
            }
            Self::Saturate | Self::Desaturate => {
                let saturation = 1. + strength * if self == Self::Saturate { 1. } else { -1. };
                for channel in 0..3 {
                    result[channel] =
                        (luminance + (before[channel] - luminance) * saturation).clamp(0., 1.);
                }
            }
        }
        result
    }

    pub(super) fn gpu_parameters(self) -> (u32, u32) {
        match self {
            Self::Dodge(range) => (3, range.code()),
            Self::Burn(range) => (4, range.code()),
            Self::Saturate => (5, 0),
            Self::Desaturate => (6, 0),
        }
    }
}

impl Range {
    fn code(self) -> u32 {
        match self {
            Self::All => 0,
            Self::Shadows => 1,
            Self::Midtones => 2,
            Self::Highlights => 3,
        }
    }

    fn weight(self, luminance: f64) -> f64 {
        let smooth = |value: f64| {
            let t = value.clamp(0., 1.);
            t * t * (3. - 2. * t)
        };
        match self {
            Self::All => 1.,
            Self::Shadows => 1. - smooth(luminance / 0.75),
            Self::Midtones => 4. * luminance * (1. - luminance),
            Self::Highlights => smooth((luminance - 0.25) / 0.75),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tonal_paint_uses_linear_exposure_and_preserves_alpha_and_neutral_colors() {
        let mid = [0.5, 0.5, 0.5, 0.37];
        let lighter = Tonal::Dodge(Range::All).apply(mid, 1.);
        let darker = Tonal::Burn(Range::All).apply(mid, 1.);
        assert!((lighter[0] - 0.6858).abs() < 0.0001);
        assert!((darker[0] - 0.3608).abs() < 0.0001);
        assert_eq!(lighter[3], mid[3]);
        assert_eq!(darker[3], mid[3]);
        for operation in [
            Tonal::Dodge(Range::All),
            Tonal::Burn(Range::All),
            Tonal::Saturate,
            Tonal::Desaturate,
        ] {
            assert_eq!(operation.apply(mid, 0.), mid);
            assert_eq!(
                operation.apply([0.2, 0.4, 0.6, 0.], 1.),
                [0.2, 0.4, 0.6, 0.]
            );
        }
        let gray = Tonal::Desaturate.apply([0.8, 0.2, 0.4, 0.37], 1.);
        assert_eq!(gray[0], gray[1]);
        assert_eq!(gray[1], gray[2]);
        assert_eq!(gray[3], 0.37);
        for value in Tonal::Saturate.apply(mid, 1.).iter().take(3) {
            assert!((*value - 0.5).abs() < 1e-12);
        }
    }

    #[test]
    fn tonal_ranges_protect_opposite_extremes() {
        assert_eq!(Range::Shadows.weight(1.), 0.);
        assert_eq!(Range::Highlights.weight(0.), 0.);
        assert_eq!(Range::Midtones.weight(0.), 0.);
        assert_eq!(Range::Midtones.weight(1.), 0.);
        assert_eq!(Range::Midtones.weight(0.5), 1.);
        assert!(Range::Shadows.weight(0.2) > Range::Shadows.weight(0.6));
        assert!(Range::Highlights.weight(0.8) > Range::Highlights.weight(0.4));
    }
}
