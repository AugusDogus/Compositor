use crate::{
    Result,
    adjustment::{LevelRange, Levels},
    invalid,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LevelsSample {
    Black,
    Gray,
    White,
}

impl LevelsSample {
    pub fn label(self) -> &'static str {
        match self {
            Self::Black => "Black",
            Self::Gray => "Gray",
            Self::White => "White",
        }
    }

    /// Calibrate all channels from unpremultiplied original RGB, matching the macOS editor.
    pub fn apply(self, levels: &Levels, rgb: [f64; 3]) -> Result<Levels> {
        if rgb
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err(invalid(
                "The sampled color is invalid. Choose a pixel inside the image.",
            ));
        }
        let mut result = levels.clone();
        result.ranges[0] = LevelRange::default();
        for (index, value) in rgb.into_iter().enumerate() {
            let range = &mut result.ranges[index + 1];
            let value = value * 255.;
            match self {
                Self::Black => range.black = value.max(0.).min(range.white - 1.),
                Self::White => range.white = value.min(255.).max(range.black + 1.),
                Self::Gray => {
                    let fraction = (value - range.black) / (range.white - range.black);
                    if fraction <= 0. || fraction >= 1. {
                        continue;
                    }
                    range.gamma = (fraction.ln() / 0.5_f64.ln()).clamp(0.1, 9.99);
                }
            }
            range.output_black = 0.;
            range.output_white = 255.;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adjustment::{Adjustment, Kind};

    #[test]
    fn sampled_endpoints_calibrate_channels_and_gray_becomes_neutral() {
        let original = Levels::default();
        let rgb = [0.2, 0.4, 0.6];
        let black = LevelsSample::Black.apply(&original, rgb).unwrap();
        assert_eq!(black.ranges[1].black, 51.);
        assert_eq!(black.ranges[3].black, 153.);
        let white = LevelsSample::White.apply(&original, rgb).unwrap();
        assert_eq!(white.ranges[2].white, 102.);
        let gray = LevelsSample::Gray.apply(&original, rgb).unwrap();
        let mut adjustment = Adjustment::new(Kind::Levels);
        adjustment.levels = gray;
        let output = adjustment.apply([rgb[0], rgb[1], rgb[2], 1.], [0., 0.]);
        assert!(output[..3].iter().all(|v| (v - 0.5).abs() < 1e-9));
        assert_eq!(
            LevelsSample::Black
                .apply(&original, [1.; 3])
                .unwrap()
                .ranges[1]
                .black,
            254.
        );
        assert_eq!(
            LevelsSample::White
                .apply(&original, [0.; 3])
                .unwrap()
                .ranges[1]
                .white,
            1.
        );
        assert_eq!(
            LevelsSample::Gray.apply(&original, [0., 1., 0.]).unwrap(),
            original
        );
        assert!(LevelsSample::Gray.apply(&original, [f64::NAN; 3]).is_err());
    }
}
