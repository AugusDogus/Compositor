use super::forms::parse_color;
use compositor::{Result, adjustment::*, invalid};

pub(super) fn channel_index(channel: Channel) -> usize {
    match channel {
        Channel::RGB => 0,
        Channel::Red => 1,
        Channel::Green => 2,
        Channel::Blue => 3,
    }
}

pub(super) fn fields(a: &Adjustment) -> Vec<(&'static str, String)> {
    let n = |label, value: f64| (label, value.to_string());
    let color = |c: Color| {
        format!(
            "#{:02X}{:02X}{:02X}",
            (c.red * 255.).round() as u8,
            (c.green * 255.).round() as u8,
            (c.blue * 255.).round() as u8
        )
    };
    match a.kind {
        Kind::AddNoise => vec![
            n("Amount", a.noise_amount.unwrap_or(10.)),
            (
                "Gaussian (0 or 1)",
                u8::from(a.noise_gaussian.unwrap_or(false)).to_string(),
            ),
            (
                "Monochromatic (0 or 1)",
                u8::from(a.noise_monochromatic.unwrap_or(false)).to_string(),
            ),
        ],
        Kind::Invert => vec![],
        Kind::GaussianBlur => vec![n("Radius", a.blur_radius.unwrap_or(10.))],
        Kind::MotionBlur => vec![
            n("Distance", a.motion_distance.unwrap_or(10.)),
            n("Angle", a.motion_angle.unwrap_or(0.)),
        ],
        Kind::BlackWhite => {
            let s = a.black_white_settings.unwrap_or_default();
            vec![
                n("Reds", s.reds),
                n("Yellows", s.yellows),
                n("Greens", s.greens),
                n("Cyans", s.cyans),
                n("Blues", s.blues),
                n("Magentas", s.magentas),
                ("Tint (0 or 1)", u8::from(s.tint).to_string()),
                n("Tint hue", s.tint_hue),
                n("Tint saturation", s.tint_saturation),
            ]
        }
        Kind::ColorBalance => {
            let s = a.color_balance_settings.unwrap_or_default();
            vec![
                n("Shadow Cyan / Red", s.shadow_cyan_red),
                n("Shadow Magenta / Green", s.shadow_magenta_green),
                n("Shadow Yellow / Blue", s.shadow_yellow_blue),
                n("Midtone Cyan / Red", s.mid_cyan_red),
                n("Midtone Magenta / Green", s.mid_magenta_green),
                n("Midtone Yellow / Blue", s.mid_yellow_blue),
                n("Highlight Cyan / Red", s.highlight_cyan_red),
                n("Highlight Magenta / Green", s.highlight_magenta_green),
                n("Highlight Yellow / Blue", s.highlight_yellow_blue),
                (
                    "Preserve luminosity (0 or 1)",
                    u8::from(s.preserve_luminosity).to_string(),
                ),
            ]
        }
        Kind::HueSaturation => {
            let hsv = a.hsv_settings.clone().unwrap_or_default();
            let adjustment = hsv
                .adjustments
                .iter()
                .find(|(r, _)| *r == hsv.range)
                .map_or(RangeAdjustment::default(), |(_, a)| *a);
            let mut fields = vec![
                n("Hue", adjustment.hue),
                n("Saturation", adjustment.saturation),
                n("Lightness", adjustment.lightness),
            ];
            if hsv.range != ColorRange::Master && !hsv.colorize {
                let band = hsv
                    .bands
                    .iter()
                    .find(|(r, _)| *r == hsv.range)
                    .map_or(hsv.range.default_band(), |(_, b)| *b);
                fields.extend([
                    n("Falloff start", band.falloff_start),
                    n("Range start", band.range_start),
                    n("Range end", band.range_end),
                    n("Falloff end", band.falloff_end),
                ]);
            }
            fields
        }
        Kind::Levels => {
            let r = a.levels.ranges[channel_index(a.levels.channel)];
            vec![
                n("Input black", r.black),
                n("Gamma", r.gamma),
                n("Input white", r.white),
                n("Output black", r.output_black),
                n("Output white", r.output_white),
            ]
        }
        Kind::Curves => vec![(
            "Points (input:output, comma separated)",
            a.curves.channels[channel_index(a.curves.channel)]
                .iter()
                .map(|p| format!("{}:{}", p.x, p.y))
                .collect::<Vec<_>>()
                .join(", "),
        )],
        Kind::Exposure => {
            let s = a.exposure_settings.unwrap_or_default();
            vec![
                n("Exposure (stops)", s.exposure),
                n("Offset", s.offset),
                n("Gamma", s.gamma),
            ]
        }
        Kind::Grain => {
            let s = a.grain_settings.unwrap_or_default();
            vec![
                n("Amount", s.amount),
                n("Size", s.size),
                n("Roughness", s.roughness),
                ("Seed", s.seed.to_string()),
            ]
        }
        Kind::GradientMap => {
            let s = a.gradient_map_settings.unwrap_or_default();
            vec![
                ("Shadows (#RRGGBB)", color(s.shadows)),
                ("Highlights (#RRGGBB)", color(s.highlights)),
                ("Reverse (0 or 1)", u8::from(s.reversed).to_string()),
            ]
        }
    }
}

pub(super) fn parse(base: &Adjustment, values: &[String]) -> Result<Adjustment> {
    let value = |i: usize| {
        values
            .get(i)
            .map(String::as_str)
            .ok_or_else(|| invalid("An adjustment field is missing."))
    };
    let number = |i| -> Result<f64> {
        let v: f64 = value(i)?
            .trim()
            .parse()
            .map_err(|_| invalid("Enter a valid number in each adjustment field."))?;
        if v.is_finite() {
            Ok(v)
        } else {
            Err(invalid("Adjustment values must be finite."))
        }
    };
    let boolean = |i| match value(i)?.trim() {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(invalid("Toggle fields accept 0 or 1.")),
    };
    let mut a = base.clone();
    match a.kind {
        Kind::GaussianBlur => a.blur_radius = Some(number(0)?),
        Kind::MotionBlur => {
            a.motion_distance = Some(number(0)?);
            a.motion_angle = Some(number(1)?);
        }
        Kind::AddNoise => {
            a.noise_amount = Some(number(0)?);
            a.noise_gaussian = Some(boolean(1)?);
            a.noise_monochromatic = Some(boolean(2)?);
        }
        Kind::Invert => {}
        Kind::BlackWhite => {
            a.black_white_settings = Some(BlackWhite {
                reds: number(0)?,
                yellows: number(1)?,
                greens: number(2)?,
                cyans: number(3)?,
                blues: number(4)?,
                magentas: number(5)?,
                tint: boolean(6)?,
                tint_hue: number(7)?,
                tint_saturation: number(8)?,
            });
        }
        Kind::ColorBalance => {
            a.color_balance_settings = Some(ColorBalance {
                shadow_cyan_red: number(0)?,
                shadow_magenta_green: number(1)?,
                shadow_yellow_blue: number(2)?,
                mid_cyan_red: number(3)?,
                mid_magenta_green: number(4)?,
                mid_yellow_blue: number(5)?,
                highlight_cyan_red: number(6)?,
                highlight_magenta_green: number(7)?,
                highlight_yellow_blue: number(8)?,
                preserve_luminosity: boolean(9)?,
            });
        }

        Kind::HueSaturation => {
            let hsv = a.hsv_settings.get_or_insert_with(HueSaturation::default);
            let adjustment = RangeAdjustment {
                hue: number(0)?,
                saturation: number(1)?,
                lightness: number(2)?,
            };
            hsv.adjustments.retain(|(r, _)| *r != hsv.range);
            hsv.adjustments.push((hsv.range, adjustment));
            let hue_range = if hsv.colorize {
                0. ..=360.
            } else {
                -180. ..=180.
            };
            let saturation_range = if hsv.colorize {
                0. ..=100.
            } else {
                -100. ..=100.
            };
            if !hue_range.contains(&adjustment.hue)
                || !saturation_range.contains(&adjustment.saturation)
                || !(-100. ..=100.).contains(&adjustment.lightness)
            {
                return Err(invalid(if hsv.colorize {
                    "Colorize needs hue from 0 to 360, saturation from 0 to 100, and lightness from -100 to 100."
                } else {
                    "Hue must be -180 to 180, and saturation and lightness must be -100 to 100."
                }));
            }
            if hsv.range != ColorRange::Master && !hsv.colorize {
                let band = HueBand {
                    falloff_start: number(3)?,
                    range_start: number(4)?,
                    range_end: number(5)?,
                    falloff_end: number(6)?,
                };
                let span = (band.falloff_end - band.falloff_start).rem_euclid(360.);
                let start = (band.range_start - band.falloff_start).rem_euclid(360.);
                let end = (band.range_end - band.falloff_start).rem_euclid(360.);
                if span <= 1. || span > 350. || start > end || end > span {
                    return Err(invalid(
                        "Hue range handles must stay in order around a band smaller than 350 degrees.",
                    ));
                }
                hsv.bands.retain(|(r, _)| *r != hsv.range);
                hsv.bands.push((hsv.range, band));
            }
        }
        Kind::Levels => {
            a.levels.ranges[channel_index(a.levels.channel)] = LevelRange {
                black: number(0)?,
                gamma: number(1)?,
                white: number(2)?,
                output_black: number(3)?,
                output_white: number(4)?,
            };
        }
        Kind::Curves => {
            let points: Result<Vec<_>> = value(0)?.split(',').map(|point| {
                let (x, y) = point.trim().split_once(':').ok_or_else(|| invalid("Enter curve points as input:output pairs, such as 0:0, 128:150, 255:255."))?;
                let parse = |v: &str| v.trim().parse::<f64>().map_err(|_| invalid("Curve coordinates must be numbers from 0 to 255."));
                Ok(CurvePoint { x: parse(x)?, y: parse(y)? })
            }).collect();
            a.curves.channels[channel_index(a.curves.channel)] = points?;
        }
        Kind::Exposure => {
            a.exposure_settings = Some(Exposure {
                exposure: number(0)?,
                offset: number(1)?,
                gamma: number(2)?,
            })
        }
        Kind::Grain => {
            a.grain_settings = Some(Grain {
                amount: number(0)?,
                size: number(1)?,
                roughness: number(2)?,
                seed: value(3)?.trim().parse().map_err(|_| {
                    invalid("Grain seed must be a whole number from 0 to 4294967295.")
                })?,
            })
        }
        Kind::GradientMap => {
            let c = |v| {
                let p = parse_color(v)?;
                Ok::<Color, compositor::Error>(Color {
                    red: p[0] as f64 / 255.,
                    green: p[1] as f64 / 255.,
                    blue: p[2] as f64 / 255.,
                })
            };
            a.gradient_map_settings = Some(GradientMap {
                shadows: c(value(0)?)?,
                highlights: c(value(1)?)?,
                reversed: boolean(2)?,
            });
        }
    }
    a.validate()?;
    Ok(a)
}

#[cfg(test)]
mod noise_tests {
    use super::*;
    #[test]
    fn noise_controls_preserve_seed_and_reject_invalid_settings() {
        let mut settings = Adjustment::new(Kind::AddNoise);
        settings.noise_seed = Some(u32::MAX);
        assert_eq!(fields(&settings).len(), 3);
        let next = parse(&settings, &["37".into(), "1".into(), "1".into()]).unwrap();
        assert_eq!(next.noise_seed, settings.noise_seed);
        assert_eq!(next.noise_amount, Some(37.));
        assert_eq!(next.noise_gaussian, Some(true));
        assert_eq!(next.noise_monochromatic, Some(true));
        assert!(parse(&settings, &["401".into(), "0".into(), "0".into()]).is_err());
    }
}
