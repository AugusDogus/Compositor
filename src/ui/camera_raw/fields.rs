use super::*;
use compositor::{
    adjustment::CurvePoint,
    camera_raw::{PointColor, scalars},
    invalid,
};
impl Edit {
    pub(in crate::ui) fn fields(&self) -> Vec<(&'static str, String)> {
        let s = &self.settings;
        match self.group {
            Group::Light => s.light.fields(),
            Group::Color => s.color.fields(),
            Group::Effects => s.effects.fields(),
            Group::Curve => {
                let mut fields = s.curve.fields();
                fields.push((
                    "Point curve (input:output)",
                    s.curves.channels[self.channel]
                        .iter()
                        .map(|p| format!("{}:{}", p.x, p.y))
                        .collect::<Vec<_>>()
                        .join(", "),
                ));
                fields
            }
            Group::Mixer => match self.mixer_page {
                MixerPage::Families => vec![
                    ("Hue", s.mixer.hue[self.color].to_string()),
                    ("Saturation", s.mixer.saturation[self.color].to_string()),
                    ("Luminance", s.mixer.luminance[self.color].to_string()),
                ],
                MixerPage::Points => s.mixer.points.get(self.point).map_or_else(Vec::new, |p| {
                    vec![
                        ("Hue", p.hue.to_string()),
                        ("Saturation", p.saturation.to_string()),
                        ("Luminance", p.luminance.to_string()),
                        ("Hue shift", p.hue_shift.to_string()),
                        ("Saturation shift", p.saturation_shift.to_string()),
                        ("Luminance shift", p.luminance_shift.to_string()),
                        ("Hue range", p.hue_range.to_string()),
                        ("Saturation range", p.saturation_range.to_string()),
                        ("Luminance range", p.luminance_range.to_string()),
                    ]
                }),
            },
            Group::Grading => {
                let w = &s.grading.wheels[self.wheel];
                vec![
                    ("Hue", w.hue.to_string()),
                    ("Saturation", w.saturation.to_string()),
                    ("Luminance", w.luminance.to_string()),
                    ("Blending", s.grading.blending.to_string()),
                    ("Balance", s.grading.balance.to_string()),
                ]
            }
            Group::Detail => s.detail.fields(),
            Group::Optics => s.optics.fields(),
            Group::Geometry => {
                let mut fields = s.geometry.fields();
                for g in &s.guides {
                    fields.extend([
                        ("Guide start X", g.start[0].to_string()),
                        ("Guide start Y", g.start[1].to_string()),
                        ("Guide end X", g.end[0].to_string()),
                        ("Guide end Y", g.end[1].to_string()),
                    ]);
                }
                fields
            }
            Group::Calibration => s.calibration.fields(),
        }
    }
    pub(in crate::ui) fn parse_fields(&mut self, v: &[String]) -> Result<Settings> {
        let mut s = self.settings.clone();
        let n = |i: usize| -> Result<f64> {
            v.get(i)
                .and_then(|s| s.parse().ok())
                .filter(|n: &f64| n.is_finite())
                .ok_or_else(|| invalid("Enter a finite number for every Camera Raw parameter."))
        };
        match self.group {
            Group::Light => s.light = scalars::Light::parse(v)?,
            Group::Color => s.color = scalars::Color::parse(v)?,
            Group::Effects => s.effects = scalars::Effects::parse(v)?,
            Group::Curve => {
                s.curve = scalars::Curve::parse(v)?;
                let points = v
                    .get(8)
                    .ok_or_else(|| invalid("Enter at least two curve points."))?;
                s.curves.channels[self.channel]=points.split(',').map(|point|{let (x,y)=point.trim().split_once(':').ok_or_else(||invalid("Enter curve points as input:output, separated by commas (0:0, 255:255)."))?;Ok(CurvePoint{x:x.trim().parse().map_err(|_|invalid("Curve input must be a number from 0 to 255."))?,y:y.trim().parse().map_err(|_|invalid("Curve output must be a number from 0 to 255."))?})}).collect::<Result<Vec<_>>>()?;
            }
            Group::Mixer => match self.mixer_page {
                MixerPage::Families => {
                    s.mixer.hue[self.color] = n(0)?;
                    s.mixer.saturation[self.color] = n(1)?;
                    s.mixer.luminance[self.color] = n(2)?;
                }
                MixerPage::Points => {
                    if let Some(point) = s.mixer.points.get_mut(self.point) {
                        *point = PointColor {
                            hue: n(0)?,
                            saturation: n(1)?,
                            luminance: n(2)?,
                            hue_shift: n(3)?,
                            saturation_shift: n(4)?,
                            luminance_shift: n(5)?,
                            hue_range: n(6)?,
                            saturation_range: n(7)?,
                            luminance_range: n(8)?,
                        };
                    }
                }
            },
            Group::Grading => {
                let w = &mut s.grading.wheels[self.wheel];
                w.hue = n(0)?;
                w.saturation = n(1)?;
                w.luminance = n(2)?;
                s.grading.blending = n(3)?;
                s.grading.balance = n(4)?;
            }
            Group::Detail => s.detail = scalars::Detail::parse(v)?,
            Group::Optics => s.optics = scalars::Optics::parse(v)?,
            Group::Geometry => {
                s.geometry = scalars::Geometry::parse(v)?;
                for (i, g) in s.guides.iter_mut().enumerate() {
                    let i = 7 + i * 4;
                    g.start = [n(i)?, n(i + 1)?];
                    g.end = [n(i + 2)?, n(i + 3)?];
                }
            }
            Group::Calibration => s.calibration = scalars::Calibration::parse(v)?,
        }
        s.validate()?;
        self.settings = s.clone();
        Ok(s)
    }
}
