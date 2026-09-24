//! Scalar controls, defaults and validated ranges from upstream CameraRaw settings.
use crate::{Result, invalid};

macro_rules! group {
    ($name:ident { $( $field:ident: ($label:literal, $default:expr, $min:expr, $max:expr) ),* $(,)? }) => {
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name { $(pub $field: f64),* }
        impl Default for $name { fn default() -> Self { Self { $($field: $default),* } } }
        impl $name {
            pub fn ranges() -> Vec<(&'static str, f64, f64)> { vec![$(($label, $min, $max)),*] }
            pub fn fields(&self) -> Vec<(&'static str, String)> { vec![$(($label, self.$field.to_string())),*] }
            pub fn parse(values: &[String]) -> Result<Self> {
                let mut values = values.iter();
                Ok(Self {$($field: parse_number(values.next(), $label, $min, $max)?),*})
            }
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
pub(super) fn parse_number(value: Option<&String>, label: &str, min: f64, max: f64) -> Result<f64> {
    let value = value
        .and_then(|s| s.parse::<f64>().ok())
        .ok_or_else(|| invalid(format!("Enter a number for {label}.")))?;
    check(value, label, min, max)?;
    Ok(value)
}

group! { Light {
    exposure: ("Exposure", 0., -5., 5.),
    contrast: ("Contrast", 0., -100., 100.),
    highlights: ("Highlights", 0., -100., 100.),
    shadows: ("Shadows", 0., -100., 100.),
    whites: ("Whites", 0., -100., 100.),
    blacks: ("Blacks", 0., -100., 100.),
} }

group! { Color {
    temperature: ("Temperature", 0., -100., 100.),
    tint: ("Tint", 0., -100., 100.),
    vibrance: ("Vibrance", 0., -100., 100.),
    saturation: ("Saturation", 0., -100., 100.),
} }

group! { Effects {
    texture: ("Texture", 0., -100., 100.),
    clarity: ("Clarity", 0., -100., 100.),
    dehaze: ("Dehaze", 0., -100., 100.),
    glow: ("Glow", 0., 0., 100.),
    glow_range: ("Glow range", 0., -100., 100.),
    glow_spread: ("Glow spread", 0., -100., 100.),
    glow_warmth: ("Glow warmth", 0., -100., 100.),
    vignette_amount: ("Vignette", 0., -100., 100.),
    vignette_midpoint: ("Midpoint", 50., 0., 100.),
    vignette_roundness: ("Roundness", 0., -100., 100.),
    vignette_feather: ("Feather", 50., 0., 100.),
    vignette_highlights: ("Protect highlights", 0., 0., 100.),
    grain_amount: ("Grain", 0., 0., 100.),
    grain_size: ("Grain size", 25., 0., 100.),
    grain_roughness: ("Roughness", 50., 0., 100.),
} }

group! { Curve {
    shadows: ("Shadows", 0., -100., 100.),
    darks: ("Darks", 0., -100., 100.),
    lights: ("Lights", 0., -100., 100.),
    highlights: ("Highlights", 0., -100., 100.),
    shadow_split: ("Shadow split", 25., 5., 90.),
    dark_split: ("Dark split", 50., 7., 95.),
    light_split: ("Light split", 75., 9., 98.),
    refine_saturation: ("Refine saturation", 0., -100., 100.),
} }

group! { Detail {
    sharpen_amount: ("Sharpening", 0., 0., 150.),
    sharpen_radius: ("Sharpening radius", 10., 0., 100.),
    sharpen_detail: ("Sharpening detail", 25., 0., 100.),
    sharpen_masking: ("Sharpening masking", 0., 0., 100.),
    noise_luminance: ("Luminance noise", 0., 0., 100.),
    noise_luminance_detail: ("Luminance detail", 50., 0., 100.),
    noise_luminance_contrast: ("Luminance contrast", 0., 0., 100.),
    noise_color: ("Color noise", 0., 0., 100.),
    noise_color_detail: ("Color detail", 50., 0., 100.),
    noise_color_smoothness: ("Color smoothness", 50., 0., 100.),
} }

group! { Optics {
    profile_distortion: ("Profile distortion", 100., 0., 100.),
    profile_vignetting: ("Profile vignetting", 100., 0., 100.),
    distortion: ("Manual distortion", 0., -100., 100.),
    purple_amount: ("Purple amount", 0., 0., 100.),
    purple_hue_low: ("Purple hue low", 270., 0., 360.),
    purple_hue_high: ("Purple hue high", 310., 0., 360.),
    green_amount: ("Green amount", 0., 0., 100.),
    green_hue_low: ("Green hue low", 60., 0., 360.),
    green_hue_high: ("Green hue high", 120., 0., 360.),
    vignette_amount: ("Lens vignette", 0., -100., 100.),
    vignette_midpoint: ("Lens midpoint", 50., 0., 100.),
} }

group! { Geometry {
    vertical: ("Vertical", 0., -100., 100.),
    horizontal: ("Horizontal", 0., -100., 100.),
    rotate: ("Rotate", 0., -45., 45.),
    aspect: ("Aspect", 0., -100., 100.),
    scale: ("Scale", 0., -99., 100.),
    offset_x: ("Offset X", 0., -100., 100.),
    offset_y: ("Offset Y", 0., -100., 100.),
} }

group! { Calibration {
    shadow_tint: ("Shadow tint", 0., -100., 100.),
    red_hue: ("Red hue", 0., -100., 100.),
    red_saturation: ("Red saturation", 0., -100., 100.),
    green_hue: ("Green hue", 0., -100., 100.),
    green_saturation: ("Green saturation", 0., -100., 100.),
    blue_hue: ("Blue hue", 0., -100., 100.),
    blue_saturation: ("Blue saturation", 0., -100., 100.),
} }
