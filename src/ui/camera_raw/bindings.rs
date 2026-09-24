//! Typed numeric bindings. The form index only identifies a visible text input.
use super::*;
use compositor::camera_raw::{
    ColorGrading, Guide, PointColor, Wheel,
    scalars::{self, Parameter},
};

#[derive(Clone, Copy)]
pub(super) enum Binding {
    Light(&'static Parameter<scalars::Light>),
    Color(&'static Parameter<scalars::Color>),
    Effects(&'static Parameter<scalars::Effects>),
    Curve(&'static Parameter<scalars::Curve>),
    Detail(&'static Parameter<scalars::Detail>),
    Optics(&'static Parameter<scalars::Optics>),
    Geometry(&'static Parameter<scalars::Geometry>),
    Calibration(&'static Parameter<scalars::Calibration>),
    MixerHue(&'static Parameter<f64>, usize),
    MixerSaturation(&'static Parameter<f64>, usize),
    MixerLuminance(&'static Parameter<f64>, usize),
    Point(&'static Parameter<PointColor>, usize),
    Wheel(&'static Parameter<Wheel>, usize),
    Grading(&'static Parameter<ColorGrading>),
    Guide(&'static Parameter<Guide>, usize),
}
impl Binding {
    pub fn value(self, s: &Settings) -> f64 {
        match self {
            Self::Light(p) => (p.get)(&s.light),
            Self::Color(p) => (p.get)(&s.color),
            Self::Effects(p) => (p.get)(&s.effects),
            Self::Curve(p) => (p.get)(&s.curve),
            Self::Detail(p) => (p.get)(&s.detail),
            Self::Optics(p) => (p.get)(&s.optics),
            Self::Geometry(p) => (p.get)(&s.geometry),
            Self::Calibration(p) => (p.get)(&s.calibration),
            Self::MixerHue(p, index) => (p.get)(&s.mixer.hue[index]),
            Self::MixerSaturation(p, index) => (p.get)(&s.mixer.saturation[index]),
            Self::MixerLuminance(p, index) => (p.get)(&s.mixer.luminance[index]),
            Self::Point(p, index) => (p.get)(&s.mixer.points[index]),
            Self::Wheel(p, index) => (p.get)(&s.grading.wheels[index]),
            Self::Grading(p) => (p.get)(&s.grading),
            Self::Guide(p, index) => (p.get)(&s.guides[index]),
        }
    }
    pub fn metadata(self) -> (&'static str, f64, f64, f64) {
        match self {
            Self::Light(p) => (p.label, p.min, p.max, p.default),
            Self::Color(p) => (p.label, p.min, p.max, p.default),
            Self::Effects(p) => (p.label, p.min, p.max, p.default),
            Self::Curve(p) => (p.label, p.min, p.max, p.default),
            Self::Detail(p) => (p.label, p.min, p.max, p.default),
            Self::Optics(p) => (p.label, p.min, p.max, p.default),
            Self::Geometry(p) => (p.label, p.min, p.max, p.default),
            Self::Calibration(p) => (p.label, p.min, p.max, p.default),
            Self::MixerHue(p, _) => (p.label, p.min, p.max, p.default),
            Self::MixerSaturation(p, _) => (p.label, p.min, p.max, p.default),
            Self::MixerLuminance(p, _) => (p.label, p.min, p.max, p.default),
            Self::Point(p, _) => (p.label, p.min, p.max, p.default),
            Self::Wheel(p, _) => (p.label, p.min, p.max, p.default),
            Self::Grading(p) => (p.label, p.min, p.max, p.default),
            Self::Guide(p, _) => (p.label, p.min, p.max, p.default),
        }
    }
    pub fn set(self, s: &mut Settings, text: &str) -> Result<()> {
        match self {
            Self::Light(p) => (p.set)(&mut s.light, p.parse(text)?),
            Self::Color(p) => (p.set)(&mut s.color, p.parse(text)?),
            Self::Effects(p) => (p.set)(&mut s.effects, p.parse(text)?),
            Self::Curve(p) => (p.set)(&mut s.curve, p.parse(text)?),
            Self::Detail(p) => (p.set)(&mut s.detail, p.parse(text)?),
            Self::Optics(p) => (p.set)(&mut s.optics, p.parse(text)?),
            Self::Geometry(p) => (p.set)(&mut s.geometry, p.parse(text)?),
            Self::Calibration(p) => (p.set)(&mut s.calibration, p.parse(text)?),
            Self::MixerHue(p, index) => (p.set)(&mut s.mixer.hue[index], p.parse(text)?),
            Self::MixerSaturation(p, index) => {
                (p.set)(&mut s.mixer.saturation[index], p.parse(text)?)
            }
            Self::MixerLuminance(p, index) => {
                (p.set)(&mut s.mixer.luminance[index], p.parse(text)?)
            }
            Self::Point(p, index) => (p.set)(&mut s.mixer.points[index], p.parse(text)?),
            Self::Wheel(p, index) => (p.set)(&mut s.grading.wheels[index], p.parse(text)?),
            Self::Grading(p) => (p.set)(&mut s.grading, p.parse(text)?),
            Self::Guide(p, index) => (p.set)(&mut s.guides[index], p.parse(text)?),
        }
        Ok(())
    }
}
macro_rules! parameters {
    ($name:ident: $ty:ty { $($field:ident: ($label:literal, $default:expr, $min:expr, $max:expr)),* $(,)? }) => {
        const $name: &[Parameter<$ty>] = &[$(Parameter {
            label: $label,
            min: $min,
            max: $max,
            default: $default,
            get: |target| target.$field,
            set: |target, value| target.$field = value,
        }),*];
    };
}
parameters! { POINT: PointColor {
    hue: ("Hue", 0., 0., 360.),
    saturation: ("Saturation", 0., 0., 1.),
    luminance: ("Luminance", 0., 0., 1.),
    hue_shift: ("Hue shift", 0., -100., 100.),
    saturation_shift: ("Saturation shift", 0., -100., 100.),
    luminance_shift: ("Luminance shift", 0., -100., 100.),
    hue_range: ("Hue range", 30., 5., 180.),
    saturation_range: ("Saturation range", 0.4, 0.05, 1.),
    luminance_range: ("Luminance range", 0.4, 0.05, 1.),
} }
parameters! { WHEEL: Wheel {
    hue: ("Hue", 0., 0., 360.),
    saturation: ("Saturation", 0., 0., 100.),
    luminance: ("Luminance", 0., -100., 100.),
} }
parameters! { GRADING: ColorGrading {
    blending: ("Blending", 50., 0., 100.),
    balance: ("Balance", 0., -100., 100.),
} }
const FAMILY: &[Parameter<f64>] = &[
    Parameter {
        label: "Hue",
        default: 0.,
        min: -100.,
        max: 100.,
        get: |v| *v,
        set: |t, v| *t = v,
    },
    Parameter {
        label: "Saturation",
        default: 0.,
        min: -100.,
        max: 100.,
        get: |v| *v,
        set: |t, v| *t = v,
    },
    Parameter {
        label: "Luminance",
        default: 0.,
        min: -100.,
        max: 100.,
        get: |v| *v,
        set: |t, v| *t = v,
    },
];
const GUIDE: &[Parameter<Guide>] = &[
    Parameter {
        label: "Guide start X",
        default: 0.25,
        min: 0.,
        max: 1.,
        get: |g| g.start[0],
        set: |g, v| g.start[0] = v,
    },
    Parameter {
        label: "Guide start Y",
        default: 0.25,
        min: 0.,
        max: 1.,
        get: |g| g.start[1],
        set: |g, v| g.start[1] = v,
    },
    Parameter {
        label: "Guide end X",
        default: 0.75,
        min: 0.,
        max: 1.,
        get: |g| g.end[0],
        set: |g, v| g.end[0] = v,
    },
    Parameter {
        label: "Guide end Y",
        default: 0.25,
        min: 0.,
        max: 1.,
        get: |g| g.end[1],
        set: |g, v| g.end[1] = v,
    },
];
impl Edit {
    pub(super) fn bindings(&self) -> Vec<Binding> {
        match self.group {
            Group::Light => scalars::Light::PARAMETERS
                .iter()
                .map(Binding::Light)
                .collect(),
            Group::Color => scalars::Color::PARAMETERS
                .iter()
                .map(Binding::Color)
                .collect(),
            Group::Effects => scalars::Effects::PARAMETERS
                .iter()
                .map(Binding::Effects)
                .collect(),
            Group::Curve => scalars::Curve::PARAMETERS
                .iter()
                .map(Binding::Curve)
                .collect(),
            Group::Detail => scalars::Detail::PARAMETERS
                .iter()
                .map(Binding::Detail)
                .collect(),
            Group::Optics => scalars::Optics::PARAMETERS
                .iter()
                .map(Binding::Optics)
                .collect(),
            Group::Calibration => scalars::Calibration::PARAMETERS
                .iter()
                .map(Binding::Calibration)
                .collect(),
            Group::Geometry => scalars::Geometry::PARAMETERS
                .iter()
                .map(Binding::Geometry)
                .chain(
                    (0..self.settings.guides.len())
                        .flat_map(|i| GUIDE.iter().map(move |p| Binding::Guide(p, i))),
                )
                .collect(),
            Group::Mixer if self.mixer_page == MixerPage::Families => vec![
                Binding::MixerHue(&FAMILY[0], self.color),
                Binding::MixerSaturation(&FAMILY[1], self.color),
                Binding::MixerLuminance(&FAMILY[2], self.color),
            ],
            Group::Mixer => {
                if self.point < self.settings.mixer.points.len() {
                    POINT
                        .iter()
                        .map(|p| Binding::Point(p, self.point))
                        .collect()
                } else {
                    Vec::new()
                }
            }
            Group::Grading => WHEEL
                .iter()
                .map(|p| Binding::Wheel(p, self.wheel))
                .chain(GRADING.iter().map(Binding::Grading))
                .collect(),
        }
    }
}
