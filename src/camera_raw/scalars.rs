//! Scalar controls, defaults and validated ranges from upstream CameraRaw settings.
use crate::{Result, invalid};

/// One numeric input's metadata and typed accessors. Text parsing stays at the UI boundary.
pub struct Parameter<T, Id = ()> {
    pub id: Id,
    pub label: &'static str,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    pub get: fn(&T) -> f64,
    pub set: fn(&mut T, f64),
}
impl<T, Id> Parameter<T, Id> {
    pub fn parse(&self, text: &str) -> Result<f64> {
        let value = text
            .parse::<f64>()
            .map_err(|_| invalid(format!("Enter a number for {}.", self.label)))?;
        check(value, self.label, self.min, self.max)?;
        Ok(value)
    }
}
macro_rules! group {
    ($name:ident, $identity:ident { $( $field:ident: $variant:ident ($label:literal, $default:expr, $min:expr, $max:expr) ),* $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $identity { $($variant),* }
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name { $(pub $field: f64),* }
        impl Default for $name { fn default() -> Self { Self { $($field: $default),* } } }
        impl $name {
            pub const PARAMETERS: &'static [Parameter<Self, $identity>] = &[$(Parameter {
                id: $identity::$variant, label: $label, default: $default, min: $min, max: $max,
                get: |value| value.$field,
                set: |target, value| target.$field = value,
            }),*];
            pub fn validate(&self) -> Result<()> { $(check(self.$field, $label, $min, $max)?;)* Ok(()) }
        }
    };
}
pub(super) fn check(value: f64, label: &str, min: f64, max: f64) -> Result<()> {
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(invalid(format!(
            "{label} must be between {min} and {max}. Your original image is preserved."
        )));
    }
    Ok(())
}
group! { Light, LightParameter {
    exposure: Exposure ("Exposure", 0., -5., 5.),
    contrast: Contrast ("Contrast", 0., -100., 100.),
    highlights: Highlights ("Highlights", 0., -100., 100.),
    shadows: Shadows ("Shadows", 0., -100., 100.),
    whites: Whites ("Whites", 0., -100., 100.),
    blacks: Blacks ("Blacks", 0., -100., 100.),
} }

group! { Color, ColorParameter {
    temperature: Temperature ("Temperature", 0., -100., 100.),
    tint: Tint ("Tint", 0., -100., 100.),
    vibrance: Vibrance ("Vibrance", 0., -100., 100.),
    saturation: Saturation ("Saturation", 0., -100., 100.),
} }

group! { Effects, EffectsParameter {
    texture: Texture ("Texture", 0., -100., 100.),
    clarity: Clarity ("Clarity", 0., -100., 100.),
    dehaze: Dehaze ("Dehaze", 0., -100., 100.),
    glow: Glow ("Glow", 0., 0., 100.),
    glow_range: GlowRange ("Glow range", 0., -100., 100.),
    glow_spread: GlowSpread ("Glow spread", 0., -100., 100.),
    glow_warmth: GlowWarmth ("Glow warmth", 0., -100., 100.),
    vignette_amount: VignetteAmount ("Vignette", 0., -100., 100.),
    vignette_midpoint: VignetteMidpoint ("Midpoint", 50., 0., 100.),
    vignette_roundness: VignetteRoundness ("Roundness", 0., -100., 100.),
    vignette_feather: VignetteFeather ("Feather", 50., 0., 100.),
    vignette_highlights: VignetteHighlights ("Protect highlights", 0., 0., 100.),
    grain_amount: GrainAmount ("Grain", 0., 0., 100.),
    grain_size: GrainSize ("Grain size", 25., 0., 100.),
    grain_roughness: GrainRoughness ("Roughness", 50., 0., 100.),
} }

group! { Curve, CurveParameter {
    shadows: Shadows ("Shadows", 0., -100., 100.),
    darks: Darks ("Darks", 0., -100., 100.),
    lights: Lights ("Lights", 0., -100., 100.),
    highlights: Highlights ("Highlights", 0., -100., 100.),
    shadow_split: ShadowSplit ("Shadow split", 25., 5., 90.),
    dark_split: DarkSplit ("Dark split", 50., 7., 95.),
    light_split: LightSplit ("Light split", 75., 9., 98.),
    refine_saturation: RefineSaturation ("Refine saturation", 0., -100., 100.),
} }

group! { Detail, DetailParameter {
    sharpen_amount: SharpenAmount ("Sharpening", 0., 0., 150.),
    sharpen_radius: SharpenRadius ("Sharpening radius", 10., 0., 100.),
    sharpen_detail: SharpenDetail ("Sharpening detail", 25., 0., 100.),
    sharpen_masking: SharpenMasking ("Sharpening masking", 0., 0., 100.),
    noise_luminance: NoiseLuminance ("Luminance noise", 0., 0., 100.),
    noise_luminance_detail: NoiseLuminanceDetail ("Luminance detail", 50., 0., 100.),
    noise_luminance_contrast: NoiseLuminanceContrast ("Luminance contrast", 0., 0., 100.),
    noise_color: NoiseColor ("Color noise", 0., 0., 100.),
    noise_color_detail: NoiseColorDetail ("Color detail", 50., 0., 100.),
    noise_color_smoothness: NoiseColorSmoothness ("Color smoothness", 50., 0., 100.),
} }

group! { Optics, OpticsParameter {
    profile_distortion: ProfileDistortion ("Profile distortion", 100., 0., 100.),
    profile_vignetting: ProfileVignetting ("Profile vignetting", 100., 0., 100.),
    distortion: Distortion ("Manual distortion", 0., -100., 100.),
    purple_amount: PurpleAmount ("Purple amount", 0., 0., 100.),
    purple_hue_low: PurpleHueLow ("Purple hue low", 270., 0., 360.),
    purple_hue_high: PurpleHueHigh ("Purple hue high", 310., 0., 360.),
    green_amount: GreenAmount ("Green amount", 0., 0., 100.),
    green_hue_low: GreenHueLow ("Green hue low", 60., 0., 360.),
    green_hue_high: GreenHueHigh ("Green hue high", 120., 0., 360.),
    vignette_amount: VignetteAmount ("Lens vignette", 0., -100., 100.),
    vignette_midpoint: VignetteMidpoint ("Lens midpoint", 50., 0., 100.),
} }

group! { Geometry, GeometryParameter {
    vertical: Vertical ("Vertical", 0., -100., 100.),
    horizontal: Horizontal ("Horizontal", 0., -100., 100.),
    rotate: Rotate ("Rotate", 0., -45., 45.),
    aspect: Aspect ("Aspect", 0., -100., 100.),
    scale: Scale ("Scale", 0., -99., 100.),
    offset_x: OffsetX ("Offset X", 0., -100., 100.),
    offset_y: OffsetY ("Offset Y", 0., -100., 100.),
} }

group! { Calibration, CalibrationParameter {
    shadow_tint: ShadowTint ("Shadow tint", 0., -100., 100.),
    red_hue: RedHue ("Red hue", 0., -100., 100.),
    red_saturation: RedSaturation ("Red saturation", 0., -100., 100.),
    green_hue: GreenHue ("Green hue", 0., -100., 100.),
    green_saturation: GreenSaturation ("Green saturation", 0., -100., 100.),
    blue_hue: BlueHue ("Blue hue", 0., -100., 100.),
    blue_saturation: BlueSaturation ("Blue saturation", 0., -100., 100.),
} }
