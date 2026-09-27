//! Convenience editors whose output remains an ordinary upstream adjustment.
use super::{Adjustment, Kind, LevelRange, Levels};
use crate::{Result, invalid};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrightnessContrast {
    pub brightness: f64,
    pub contrast: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "settings",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum EditorHint {
    BrightnessContrast(BrightnessContrast),
}

impl BrightnessContrast {
    pub fn validate(self) -> Result<()> {
        if !(-150. ..=150.).contains(&self.brightness) || !(-50. ..=100.).contains(&self.contrast) {
            return Err(invalid(
                "Brightness must be between -150 and 150, and contrast between -50 and 100.",
            ));
        }
        Ok(())
    }

    /// Channel gamma is applied before master contrast, with no intermediate
    /// quantization. This is the same transfer function as a gamma brightness
    /// followed by linear contrast around 0.5, including endpoint clipping.
    pub fn adjustment(self) -> Result<Adjustment> {
        self.validate()?;
        let mut settings = Adjustment::new(Kind::Levels);
        settings.levels = self.levels();
        settings.editor_hint = Some(EditorHint::BrightnessContrast(self));
        Ok(settings)
    }

    fn levels(self) -> Levels {
        let factor = if self.contrast >= 0. {
            1. + self.contrast / 100. * 1.5
        } else {
            1. + self.contrast / 100.
        };
        let channel = LevelRange {
            gamma: 2_f64.powf(self.brightness / 150. * 1.6),
            ..LevelRange::default()
        };
        let master = if factor >= 1. {
            LevelRange {
                black: 255. * (0.5 - 0.5 / factor),
                white: 255. * (0.5 + 0.5 / factor),
                ..LevelRange::default()
            }
        } else {
            LevelRange {
                output_black: 255. * (1. - factor) / 2.,
                output_white: 255. * (1. + factor) / 2.,
                ..LevelRange::default()
            }
        };
        Levels {
            ranges: [master, channel, channel, channel],
            ..Levels::default()
        }
    }
}

impl Adjustment {
    /// Hints never override rendered settings, including after external edits.
    pub fn valid_editor_hint(&self) -> Option<EditorHint> {
        self.editor_hint.filter(|hint| match hint {
            EditorHint::BrightnessContrast(settings) => {
                self.kind == Kind::Levels
                    && settings.validate().is_ok()
                    && settings.levels().ranges == self.levels.ranges
            }
        })
    }

    pub fn brightness_contrast(&self) -> Option<BrightnessContrast> {
        self.valid_editor_hint().map(|hint| match hint {
            EditorHint::BrightnessContrast(settings) => settings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_match_brightness_then_contrast_without_intermediate_quantization() {
        for brightness in [-150., -61.5, 0., 47.25, 150.] {
            for contrast in [-50., -18.5, 0., 27.25, 100.] {
                let settings = BrightnessContrast {
                    brightness,
                    contrast,
                }
                .adjustment()
                .unwrap();
                settings.validate().unwrap();
                let factor = if contrast >= 0. {
                    1. + contrast / 100. * 1.5
                } else {
                    1. + contrast / 100.
                };
                for i in 0..=1024 {
                    let x = f64::from(i) / 1024.;
                    let expected = ((x.powf(2_f64.powf(-brightness / 150. * 1.6)) - 0.5) * factor
                        + 0.5)
                        .clamp(0., 1.);
                    let actual = settings.apply([x, x, x, 0.37], [0., 0.]);
                    assert!((actual[0] - expected).abs() < 1e-14);
                    assert_eq!(actual[3], 0.37);
                }
            }
        }
    }

    #[test]
    fn stale_hints_do_not_reinterpret_levels_and_are_not_serialized() {
        let mut adjustment = BrightnessContrast {
            brightness: 50.,
            contrast: -20.,
        }
        .adjustment()
        .unwrap();
        assert!(adjustment.brightness_contrast().is_some());
        let native = serde_json::to_string(&adjustment).unwrap();
        assert!(!native.contains("editor"));
        let decoded: Adjustment = serde_json::from_str(&native).unwrap();
        assert!(decoded.brightness_contrast().is_none());
        assert_eq!(decoded.levels, adjustment.levels);
        adjustment.levels.ranges[0].gamma = 1.5;
        assert!(adjustment.brightness_contrast().is_none());
        for value in [f64::NAN, f64::INFINITY, -151., 151.] {
            assert!(
                BrightnessContrast {
                    brightness: value,
                    contrast: 0.
                }
                .adjustment()
                .is_err()
            );
        }
    }
}
