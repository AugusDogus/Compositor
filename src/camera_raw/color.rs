use super::scalars::check;
use crate::Result;
#[derive(Clone, Debug, PartialEq)]
pub struct PointColor {
    pub hue: f64,
    pub saturation: f64,
    pub luminance: f64,
    pub hue_shift: f64,
    pub saturation_shift: f64,
    pub luminance_shift: f64,
    pub hue_range: f64,
    pub saturation_range: f64,
    pub luminance_range: f64,
}
impl Default for PointColor {
    fn default() -> Self {
        Self {
            hue: 0.,
            saturation: 0.,
            luminance: 0.,
            hue_shift: 0.,
            saturation_shift: 0.,
            luminance_shift: 0.,
            hue_range: 30.,
            saturation_range: 0.4,
            luminance_range: 0.4,
        }
    }
}
impl PointColor {
    pub fn validate(&self) -> Result<()> {
        check(self.hue, "Point hue", 0., 360.)?;
        for x in [self.saturation, self.luminance] {
            check(x, "Point color", 0., 1.)?;
        }
        for x in [self.hue_shift, self.saturation_shift, self.luminance_shift] {
            check(x, "Point shift", -100., 100.)?;
        }
        check(self.hue_range, "Hue range", 5., 180.)?;
        for x in [self.saturation_range, self.luminance_range] {
            check(x, "Point range", 0.05, 1.)?;
        }
        Ok(())
    }
    pub(super) fn floats(&self) -> [f32; 9] {
        [
            self.hue / 360.,
            self.saturation,
            self.luminance,
            self.hue_shift / 100.,
            self.saturation_shift / 100.,
            self.luminance_shift / 100.,
            self.hue_range / 360.,
            self.saturation_range,
            self.luminance_range,
        ]
        .map(|x| x as f32)
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColorMixer {
    pub hue: [f64; 8],
    pub saturation: [f64; 8],
    pub luminance: [f64; 8],
    pub points: Vec<PointColor>,
}
impl ColorMixer {
    pub const NAMES: [&'static str; 8] = [
        "Reds", "Oranges", "Yellows", "Greens", "Aquas", "Blues", "Purples", "Magentas",
    ];
    pub fn validate(&self) -> Result<()> {
        for &x in self
            .hue
            .iter()
            .chain(&self.saturation)
            .chain(&self.luminance)
        {
            check(x, "Mixer shift", -100., 100.)?;
        }
        if self.points.len() > 8 {
            return Err(crate::invalid("Use at most eight point colors."));
        }
        for p in &self.points {
            p.validate()?;
        }
        Ok(())
    }
    pub(super) fn floats(&self) -> Vec<f32> {
        self.hue
            .iter()
            .chain(&self.saturation)
            .chain(&self.luminance)
            .map(|x| (*x / 100.) as f32)
            .collect()
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wheel {
    pub hue: f64,
    pub saturation: f64,
    pub luminance: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ColorGrading {
    pub wheels: [Wheel; 4],
    pub blending: f64,
    pub balance: f64,
}
impl Default for ColorGrading {
    fn default() -> Self {
        Self {
            wheels: Default::default(),
            blending: 50.,
            balance: 0.,
        }
    }
}
impl ColorGrading {
    pub fn validate(&self) -> Result<()> {
        for w in &self.wheels {
            check(w.hue, "Grading hue", 0., 360.)?;
            check(w.saturation, "Grading saturation", 0., 100.)?;
            check(w.luminance, "Grading luminance", -100., 100.)?;
        }
        check(self.blending, "Blending", 0., 100.)?;
        check(self.balance, "Balance", -100., 100.)
    }
    pub(super) fn floats(&self) -> Vec<f32> {
        self.wheels
            .iter()
            .flat_map(|w| [w.hue / 360., w.saturation / 100., w.luminance / 100.].map(|x| x as f32))
            .collect()
    }
}
pub(super) fn tables(s: &super::Settings) -> [[f32; 256]; 4] {
    use crate::adjustment::Channel;
    std::array::from_fn(|channel| {
        std::array::from_fn(|i| {
            let mut tone = i as f64 / 255.;
            if channel == 0 {
                let c = &s.curve;
                let shadow = c.shadow_split / 100.;
                let dark = c.dark_split / 100.;
                let light = c.light_split / 100.;
                let (amount, lo, hi) = if tone < shadow {
                    (c.shadows, 0., shadow)
                } else if tone < dark {
                    (c.darks, shadow, dark)
                } else if tone < light {
                    (c.lights, dark, light)
                } else {
                    (c.highlights, light, 1.)
                };
                let span = (hi - lo).max(0.02);
                let weight = (1. - (tone - (lo + hi) / 2.).abs() / (span / 2.)).max(0.);
                tone = (tone + amount / 100. * weight * 0.22).clamp(0., 1.);
            }
            let channel = [Channel::RGB, Channel::Red, Channel::Green, Channel::Blue][channel];
            (s.curves.sample(tone * 255., channel) / 255.) as f32
        })
    })
}
