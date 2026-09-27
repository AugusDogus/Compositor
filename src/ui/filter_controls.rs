use super::*;
use compositor::{filters::Filter, invalid};

pub(super) mod posterize;
pub(super) mod threshold;
pub(super) mod vibrance;

impl Editor {
    pub(super) fn filter_fields(filter: Filter) -> (&'static str, Vec<(&'static str, String)>) {
        match filter {
            Filter::Posterize(settings) => ("Posterize", posterize::fields(settings)),
            Filter::Vibrance(settings) => ("Vibrance", vibrance::fields(settings)),
            Filter::Threshold(settings) => ("Threshold", threshold::fields(settings)),
            Filter::SelectiveColor(settings) => (
                "Selective Color",
                super::selective_color_controls::fields(settings),
            ),
            Filter::ChannelMixer(settings) => (
                "Channel Mixer",
                super::channel_mixer_controls::fields(settings),
            ),

            Filter::PhotoFilter(settings) => (
                "Photo Filter",
                super::photo_filter_controls::fields(settings),
            ),

            Filter::Radial(settings) => (
                "Radial Blur",
                vec![
                    (
                        "Mode (spin or zoom)",
                        match settings.mode {
                            compositor::filters::radial::Mode::Spin => "spin",
                            compositor::filters::radial::Mode::Zoom => "zoom",
                        }
                        .into(),
                    ),
                    ("Amount", settings.amount.to_string()),
                    ("Center X", (settings.center[0] * 100.).to_string()),
                    ("Center Y", (settings.center[1] * 100.).to_string()),
                ],
            ),
            Filter::LuminositySharpen(settings) => (
                "Luminosity Sharpen",
                vec![
                    ("Amount (%)", settings.amount.to_string()),
                    ("Radius", settings.radius.to_string()),
                    ("Reduce Noise (%)", settings.noise.to_string()),
                ],
            ),
            Filter::HighPass { radius } => ("High Pass", vec![("Radius", radius.to_string())]),
            Filter::UnsharpMask {
                amount,
                radius,
                threshold,
            } => (
                "Unsharp Mask",
                vec![
                    ("Amount (%)", amount.to_string()),
                    ("Radius", radius.to_string()),
                    ("Threshold", threshold.to_string()),
                ],
            ),
            Filter::Gaussian { radius } => ("Gaussian Blur", vec![("Radius", radius.to_string())]),
            Filter::Motion { distance, angle } => (
                "Motion Blur",
                vec![
                    ("Distance", distance.to_string()),
                    ("Angle", angle.to_string()),
                ],
            ),
            Filter::Noise {
                amount,
                gaussian,
                monochromatic,
                ..
            } => (
                "Add Noise",
                vec![
                    ("Amount (%)", amount.to_string()),
                    ("Gaussian (0 or 1)", u8::from(gaussian).to_string()),
                    (
                        "Monochromatic (0 or 1)",
                        u8::from(monochromatic).to_string(),
                    ),
                ],
            ),
            Filter::Lens { distortion } => (
                "Lens Correction",
                vec![("Remove distortion (-100 to 100)", distortion.to_string())],
            ),
            Filter::Vignette(s) => (
                "Vignette",
                vec![
                    ("Amount", s.amount.to_string()),
                    ("Midpoint", s.midpoint.to_string()),
                    ("Roundness", s.roundness.to_string()),
                    ("Feather", s.feather.to_string()),
                    ("Highlights", s.highlights.to_string()),
                    ("Red", (s.color[0] * 255.).to_string()),
                    ("Green", (s.color[1] * 255.).to_string()),
                    ("Blue", (s.color[2] * 255.).to_string()),
                ],
            ),
            Filter::Bloom { amount, radius } => (
                "Bloom / Glow",
                vec![
                    ("Amount", amount.to_string()),
                    ("Radius", radius.to_string()),
                ],
            ),
            Filter::TonalContrast {
                amount,
                radius,
                tones,
            } => (
                "Tonal Contrast",
                vec![
                    ("Amount", amount.to_string()),
                    ("Radius", radius.to_string()),
                    ("Shadows", tones[0].to_string()),
                    ("Midtones", tones[1].to_string()),
                    ("Highlights", tones[2].to_string()),
                ],
            ),
            Filter::ContentFill => ("Content-Aware Fill", Vec::new()),
        }
    }
    pub(super) fn filter_values(filter: Filter, values: &[String]) -> Result<Filter> {
        let n = |i: usize| -> Result<f64> {
            let n = values
                .get(i)
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|n| n.is_finite())
                .ok_or_else(|| invalid("Enter a finite number for every filter parameter."))?;
            Ok(n)
        };
        let flag = |i| -> Result<bool> {
            match n(i)? {
                0. => Ok(false),
                1. => Ok(true),
                _ => Err(invalid("Enter 0 for off or 1 for on.")),
            }
        };
        Ok(match filter {
            Filter::Posterize(_) => Filter::Posterize(posterize::parse(values)?),
            Filter::Vibrance(_) => Filter::Vibrance(vibrance::parse(values)?),
            Filter::Threshold(_) => Filter::Threshold(threshold::parse(values)?),
            Filter::SelectiveColor(_) => {
                Filter::SelectiveColor(super::selective_color_controls::parse(values)?)
            }
            Filter::ChannelMixer(_) => {
                Filter::ChannelMixer(super::channel_mixer_controls::parse(values)?)
            }

            Filter::PhotoFilter(original) => {
                Filter::PhotoFilter(super::photo_filter_controls::parse(values, original)?)
            }

            Filter::Radial(_) => {
                let mode = match values.first().map(String::as_str) {
                    Some("spin") => compositor::filters::radial::Mode::Spin,
                    Some("zoom") => compositor::filters::radial::Mode::Zoom,
                    _ => return Err(invalid("Choose Spin or Zoom for radial blur.")),
                };
                let settings = compositor::filters::radial::Radial {
                    mode,
                    amount: n(1)?,
                    center: [n(2)? / 100., n(3)? / 100.],
                };
                settings.validate()?;
                Filter::Radial(settings)
            }
            Filter::LuminositySharpen(_) => {
                let settings = compositor::filters::luminosity_sharpen::Settings {
                    amount: n(0)?,
                    radius: n(1)?,
                    noise: n(2)?,
                };
                settings.validate()?;
                Filter::LuminositySharpen(settings)
            }
            Filter::HighPass { .. } => Filter::HighPass { radius: n(0)? },
            Filter::UnsharpMask { .. } => Filter::UnsharpMask {
                amount: n(0)?,
                radius: n(1)?,
                threshold: n(2)?,
            },
            Filter::Gaussian { .. } => Filter::Gaussian { radius: n(0)? },
            Filter::Motion { .. } => Filter::Motion {
                distance: n(0)?,
                angle: n(1)?,
            },
            Filter::Noise { seed, .. } => Filter::Noise {
                amount: n(0)? as f32,
                gaussian: flag(1)?,
                monochromatic: flag(2)?,
                seed,
            },
            Filter::Lens { .. } => Filter::Lens { distortion: n(0)? },
            Filter::Vignette(_) => Filter::Vignette(compositor::filters::Vignette {
                amount: n(0)?,
                midpoint: n(1)?,
                roundness: n(2)?,
                feather: n(3)?,
                highlights: n(4)?,
                color: [n(5)? / 255., n(6)? / 255., n(7)? / 255.],
            }),
            Filter::Bloom { .. } => Filter::Bloom {
                amount: n(0)?,
                radius: n(1)?,
            },
            Filter::TonalContrast { .. } => Filter::TonalContrast {
                amount: n(0)?,
                radius: n(1)?,
                tones: [n(2)?, n(3)?, n(4)?],
            },
            Filter::ContentFill => Filter::ContentFill,
        })
    }
}
