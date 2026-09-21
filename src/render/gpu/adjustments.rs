//! Shader parameter serialization. Curves retain their exact cubic segments;
//! hue responses use the same integer-degree table as the Swift implementation.
use crate::adjustment::{Adjustment, ColorRange, HueSaturation, Kind, RangeAdjustment};

pub(super) fn encode(a: &Adjustment, values: &mut Vec<f32>) -> u32 {
    match a.kind {
        Kind::HueSaturation => {
            let fallback = HueSaturation {
                adjustments: vec![(
                    ColorRange::Master,
                    RangeAdjustment {
                        hue: a.hue,
                        saturation: a.saturation,
                        lightness: a.lightness,
                    },
                )],
                colorize: a.colorize,
                ..Default::default()
            };
            let settings = a.hsv_settings.as_ref().unwrap_or(&fallback);
            let color = settings
                .adjustments
                .iter()
                .find(|(r, _)| *r == settings.range)
                .map_or(RangeAdjustment::default(), |(_, a)| *a);
            values.extend([
                f32::from(settings.colorize),
                color.hue as f32,
                color.saturation as f32,
                color.lightness as f32,
            ]);
            for degree in 0..=360 {
                let mut response = [0.; 3];
                for (range, a) in &settings.adjustments {
                    let weight = settings.weight(*range, degree as f64);
                    for (out, value) in response.iter_mut().zip([a.hue, a.saturation, a.lightness])
                    {
                        *out += weight * value;
                    }
                }
                values.extend(response.map(|v| v as f32));
            }
            1
        }
        Kind::Levels => {
            for r in &a.levels.ranges {
                values.extend(
                    [r.black, r.gamma, r.white, r.output_black, r.output_white].map(|v| v as f32),
                );
            }
            2
        }
        Kind::Curves => {
            for points in &a.curves.channels {
                values.push(points.len() as f32);
                for (i, p) in points.iter().enumerate() {
                    let slope = |j: usize| {
                        (points[j + 1].y - points[j].y) / (points[j + 1].x - points[j].x)
                    };
                    let tangent = if i == 0 {
                        slope(0)
                    } else if i == points.len() - 1 {
                        slope(i - 1)
                    } else if slope(i - 1) * slope(i) <= 0. {
                        0.
                    } else {
                        2. / (1. / slope(i - 1) + 1. / slope(i))
                    };
                    values.extend([p.x as f32, p.y as f32, tangent as f32]);
                }
            }
            3
        }
        Kind::Exposure => {
            let s = a.exposure_settings.unwrap_or_default();
            values.extend([2_f64.powf(s.exposure), s.offset, 1. / s.gamma].map(|v| v as f32));
            4
        }
        Kind::GradientMap => {
            let s = a.gradient_map_settings.unwrap_or_default();
            values.extend(s.shadows.rgb().map(|v| v as f32));
            values.extend(s.highlights.rgb().map(|v| v as f32));
            values.push(f32::from(s.reversed));
            5
        }
        Kind::Grain => {
            let s = a.grain_settings.unwrap_or_default();
            values.extend([
                s.amount as f32,
                s.size as f32,
                s.roughness as f32,
                f32::from_bits(s.seed),
            ]);
            6
        }
    }
}
