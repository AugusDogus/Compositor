use super::*;
use compositor::{filters::Filter, invalid};

impl Editor {
    pub(super) fn filter_fields(filter: Filter) -> (&'static str, Vec<(&'static str, String)>) {
        match filter {
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
            Filter::ContentFill => Filter::ContentFill,
        })
    }
}
