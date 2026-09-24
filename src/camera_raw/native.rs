use super::*;
use crate::native_pixels;
unsafe extern "C" {
    fn adjust_camera_raw_clip_overlay(
        rgba: *mut u8,
        width: usize,
        height: usize,
        stride: usize,
        shadows: i32,
        highlights: i32,
    );
    fn adjust_camera_raw_sharpen_mask_overlay(
        rgba: *mut u8,
        width: usize,
        height: usize,
        stride: usize,
        radius: f64,
        detail: f64,
        masking: f64,
        scale: f64,
    );
    fn adjust_camera_raw(
        rgba: *mut u8,
        width: usize,
        height: usize,
        stride: usize,
        redGain: f64,
        greenGain: f64,
        blueGain: f64,
        exposure: f64,
        contrast: f64,
        highlights: f64,
        shadows: f64,
        whites: f64,
        blacks: f64,
        vibrance: f64,
        saturation: f64,
        clipping: i32,
    );
    fn adjust_camera_raw_effects(
        rgba: *mut u8,
        width: usize,
        height: usize,
        stride: usize,
        texture: f64,
        clarity: f64,
        dehaze: f64,
        glow: f64,
        glowStyle: i32,
        glowRange: f64,
        glowSpread: f64,
        glowWarmth: f64,
        vignetteAmount: f64,
        vignetteMidpoint: f64,
        vignetteRoundness: f64,
        vignetteFeather: f64,
        vignetteHighlights: f64,
        vignetteStyle: i32,
        scale: f64,
    );
    fn adjust_camera_raw_curve_color(
        rgba: *mut u8,
        width: usize,
        height: usize,
        stride: usize,
        lumaLut: *const f32,
        redLut: *const f32,
        greenLut: *const f32,
        blueLut: *const f32,
        refineSaturation: f64,
        mixer: *const f32,
        pointCount: i32,
        points: *const f32,
        grade: *const f32,
        blending: f64,
        balance: f64,
        visualize: i32,
    );
    fn adjust_camera_raw_detail(
        rgba: *mut u8,
        width: usize,
        height: usize,
        stride: usize,
        sharpenAmount: f64,
        sharpenRadius: f64,
        sharpenDetail: f64,
        sharpenMasking: f64,
        noiseLuminance: f64,
        noiseLuminanceDetail: f64,
        noiseLuminanceContrast: f64,
        noiseColor: f64,
        noiseColorDetail: f64,
        noiseColorSmoothness: f64,
        scale: f64,
    );
    fn adjust_camera_raw_optics(
        rgba: *mut u8,
        width: usize,
        height: usize,
        stride: usize,
        removeChromatic: i32,
        lensProfile: i32,
        profileDistortion: f64,
        profileVignetting: f64,
        distortionK: f64,
        purpleAmount: f64,
        purpleHueLow: f64,
        purpleHueHigh: f64,
        greenAmount: f64,
        greenHueLow: f64,
        greenHueHigh: f64,
        vignetteAmount: f64,
        vignetteMidpoint: f64,
        scale: f64,
    );
    fn adjust_camera_raw_calibration(
        rgba: *mut u8,
        width: usize,
        height: usize,
        stride: usize,
        shadowTint: f64,
        redHue: f64,
        redSaturation: f64,
        greenHue: f64,
        greenSaturation: f64,
        blueHue: f64,
        blueSaturation: f64,
        processVersion: i32,
    );
    fn adjust_grain(
        rgba: *mut u8,
        width: usize,
        height: usize,
        stride: usize,
        amount: f64,
        size: f64,
        roughness: f64,
        seed: u32,
        originX: f64,
        originY: f64,
        unitsPerPixel: f64,
    );
}
pub(super) fn render(
    image: &RgbaImage,
    s: &Settings,
    scale: f64,
    options: Preview,
) -> Result<RgbaImage> {
    let mut pixels = native_pixels::premultiply(image);
    let (w, h) = (image.width() as usize, image.height() as usize);
    let p = pixels.as_mut_ptr();
    let stride = w * 4;
    // SAFETY: caller validates all dimensions/settings; the tightly packed RGBA buffer
    // remains alive and uniquely borrowed throughout. LUT/mixer buffers below have
    // exactly the lengths specified in AdjustPixels.h. No kernel retains a pointer.
    unsafe {
        if options.clipping.is_none() && !options.sharpen_mask && s.adjusts(Group::Calibration) {
            let c = &s.calibration;
            adjust_camera_raw_calibration(
                p,
                w,
                h,
                stride,
                c.shadow_tint,
                c.red_hue,
                c.red_saturation,
                c.green_hue,
                c.green_saturation,
                c.blue_hue,
                c.blue_saturation,
                match s.process {
                    Process::One => 1,
                    Process::Two => 2,
                    Process::Three => 3,
                    Process::Four => 4,
                    Process::Five => 5,
                    Process::Six => 6,
                },
            );
        }
        if s.adjusts(Group::Light) || s.adjusts(Group::Color) || options.clipping.is_some() {
            let l = &s.light;
            let c = &s.color;
            let warm = c.temperature / 100.;
            let tint = c.tint / 100.;
            adjust_camera_raw(
                p,
                w,
                h,
                stride,
                1. + 0.35 * warm + 0.15 * tint,
                1. - 0.30 * tint,
                1. - 0.35 * warm + 0.15 * tint,
                l.exposure,
                l.contrast,
                l.highlights,
                l.shadows,
                l.whites,
                l.blacks,
                c.vibrance,
                c.saturation,
                match options.clipping {
                    Some(Clipping::Shadows) => 2,
                    Some(Clipping::Highlights) => 1,
                    None => 0,
                },
            );
        }
        if options.clipping.is_some() {
            return Ok(native_pixels::unpremultiply(pixels));
        }
        if options.sharpen_mask {
            let d = &s.detail;
            adjust_camera_raw_sharpen_mask_overlay(
                p,
                w,
                h,
                stride,
                d.sharpen_radius,
                d.sharpen_detail,
                d.sharpen_masking,
                scale,
            );
            return Ok(native_pixels::unpremultiply(pixels));
        }
        if s.adjusts(Group::Curve)
            || s.adjusts(Group::Mixer)
            || s.adjusts(Group::Grading)
            || options.point_color.is_some()
        {
            let tables = super::color::tables(s);
            let mixer = s.mixer.floats();
            let points: Vec<f32> = s.mixer.points.iter().flat_map(PointColor::floats).collect();
            let grade = s.grading.floats();
            adjust_camera_raw_curve_color(
                p,
                w,
                h,
                stride,
                tables[0].as_ptr(),
                tables[1].as_ptr(),
                tables[2].as_ptr(),
                tables[3].as_ptr(),
                s.curve.refine_saturation / 100.,
                mixer.as_ptr(),
                s.mixer.points.len() as i32,
                points.as_ptr(),
                grade.as_ptr(),
                s.grading.blending / 100.,
                s.grading.balance / 100.,
                options
                    .point_color
                    .filter(|i| *i < s.mixer.points.len())
                    .map_or(-1, |i| i as i32),
            );
        }
        if s.adjusts(Group::Effects) {
            let e = &s.effects;
            adjust_camera_raw_effects(
                p,
                w,
                h,
                stride,
                e.texture,
                e.clarity,
                e.dehaze,
                e.glow,
                match s.glow_style {
                    GlowStyle::Diffusion => 0,
                    GlowStyle::Bloom => 1,
                    GlowStyle::Halation => 2,
                },
                e.glow_range,
                e.glow_spread,
                e.glow_warmth,
                e.vignette_amount,
                e.vignette_midpoint,
                e.vignette_roundness,
                e.vignette_feather,
                e.vignette_highlights,
                match s.vignette_style {
                    VignetteStyle::HighlightPriority => 0,
                    VignetteStyle::ColorPriority => 1,
                    VignetteStyle::PaintOverlay => 2,
                },
                scale,
            );
            if e.grain_amount > 0. {
                adjust_grain(
                    p,
                    w,
                    h,
                    stride,
                    e.grain_amount,
                    0.5 + e.grain_size / 100. * 19.5,
                    e.grain_roughness,
                    0,
                    0.,
                    0.,
                    1. / scale,
                );
            }
        }
        if s.adjusts(Group::Optics) {
            let o = &s.optics;
            let k = (o.distortion
                + if s.lens_profile {
                    o.profile_distortion
                } else {
                    0.
                })
                / 100.
                * 0.35;
            adjust_camera_raw_optics(
                p,
                w,
                h,
                stride,
                i32::from(s.remove_chromatic),
                i32::from(s.lens_profile),
                o.profile_distortion,
                o.profile_vignetting,
                k,
                o.purple_amount,
                o.purple_hue_low,
                o.purple_hue_high,
                o.green_amount,
                o.green_hue_low,
                o.green_hue_high,
                o.vignette_amount,
                o.vignette_midpoint,
                scale,
            );
        }
        if s.adjusts(Group::Detail) {
            let d = &s.detail;
            adjust_camera_raw_detail(
                p,
                w,
                h,
                stride,
                d.sharpen_amount,
                d.sharpen_radius,
                d.sharpen_detail,
                d.sharpen_masking,
                d.noise_luminance,
                d.noise_luminance_detail,
                d.noise_luminance_contrast,
                d.noise_color,
                d.noise_color_detail,
                d.noise_color_smoothness,
                scale,
            );
        }
    }
    // SAFETY: pixels remains a checked, tightly packed RGBA image.
    unsafe {
        adjust_camera_raw_clip_overlay(
            p,
            w,
            h,
            stride,
            i32::from(options.shadow_overlay),
            i32::from(options.highlight_overlay),
        );
    }
    Ok(native_pixels::unpremultiply(pixels))
}
