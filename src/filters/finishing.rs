//! Upstream's finishing filters. Parameters use the same ranges as FilterSheet.swift.
use crate::{Result, document::validate_size, invalid, native_pixels};
use image::RgbaImage;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vignette {
    pub amount: f64,
    pub color: [f64; 3],
    pub midpoint: f64,
    pub roundness: f64,
    pub feather: f64,
    pub highlights: f64,
}
impl Default for Vignette {
    fn default() -> Self {
        Self {
            amount: 35.,
            color: [0.; 3],
            midpoint: 50.,
            roundness: 100.,
            feather: 60.,
            highlights: 25.,
        }
    }
}
unsafe extern "C" {
    fn adjust_colored_vignette(
        rgba: *mut u8,
        width: usize,
        height: usize,
        stride: usize,
        frame_x: f64,
        frame_y: f64,
        frame_width: f64,
        frame_height: f64,
        fills_clear: i32,
        amount: f64,
        midpoint: f64,
        roundness: f64,
        feather: f64,
        highlights: f64,
        red: f64,
        green: f64,
        blue: f64,
    );
    fn adjust_tonal_contrast(
        rgba: *mut u8,
        blurred: *const u8,
        width: usize,
        height: usize,
        stride: usize,
        blurred_stride: usize,
        amount: f64,
        shadows: f64,
        midtones: f64,
        highlights: f64,
    );
}

pub(super) fn range(value: f64, min: f64, max: f64) -> Result<()> {
    if !value.is_finite() || !(min..=max).contains(&value) {
        Err(invalid(format!(
            "Filter value must be between {min} and {max}. The original pixels are preserved."
        )))
    } else {
        Ok(())
    }
}

pub(super) fn vignette(image: &RgbaImage, s: Vignette, fills_clear: bool) -> Result<RgbaImage> {
    validate_size(image.width(), image.height())?;
    for value in [s.amount, s.midpoint, s.feather, s.highlights] {
        range(value, 0., 100.)?;
    }
    range(s.roundness, -100., 100.)?;
    for value in s.color {
        range(value, 0., 1.)?;
    }
    if s.amount == 0. {
        return Ok(image.clone());
    }
    let mut pixels = native_pixels::premultiply(image);
    let (w, h) = pixels.dimensions();
    // SAFETY: the checked image owns width*height tightly packed RGBA pixels;
    // the upstream kernel only mutates those pixels and retains no pointers.
    unsafe {
        adjust_colored_vignette(
            pixels.as_mut_ptr(),
            w as usize,
            h as usize,
            w as usize * 4,
            0.,
            0.,
            w as f64,
            h as f64,
            i32::from(fills_clear),
            s.amount,
            s.midpoint,
            s.roundness,
            s.feather,
            s.highlights,
            s.color[0],
            s.color[1],
            s.color[2],
        );
    }
    Ok(native_pixels::unpremultiply(pixels))
}

pub(super) fn bloom(image: &RgbaImage, amount: f64, radius: f64) -> Result<RgbaImage> {
    range(amount, 0., 100.)?;
    range(radius, 1., 150.)?;
    if amount == 0. {
        return Ok(image.clone());
    }
    let base = native_pixels::premultiply(image);
    let blurred = blur(&base, radius as f32)?;
    let mut result = image.clone();
    for ((dst, src), glow) in result.pixels_mut().zip(base.pixels()).zip(blurred.pixels()) {
        let alpha = f64::from(src[3]) / 255.;
        if alpha == 0. {
            continue;
        }
        for c in 0..3 {
            // Screen the blurred radiance over the source, as a bloom blend.
            let color = f64::from(src[c]) / 255. / alpha;
            let light = (f64::from(glow[c]) / 255. * amount / 50.).min(1.);
            dst[c] = ((1. - (1. - color) * (1. - light)) * 255.).round() as u8;
        }
    }
    Ok(result)
}

pub(super) fn blur(image: &RgbaImage, radius: f32) -> Result<RgbaImage> {
    validate_size(image.width(), image.height())?;
    let mut result = RgbaImage::new(image.width(), image.height());
    for c in 0..4 {
        let plane = image::GrayImage::from_fn(image.width(), image.height(), |x, y| {
            image::Luma([image[(x, y)][c]])
        });
        let blurred = match crate::render::gpu_coverage_blur(&plane, radius)? {
            Some(pixels) => pixels,
            None => image::imageops::blur(&plane, radius),
        };
        for (pixel, value) in result.pixels_mut().zip(blurred.pixels()) {
            pixel[c] = value[0];
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    #[test]
    fn vignette_frames_empty_layers_and_preserves_existing_alpha() {
        let clear = RgbaImage::new(31, 31);
        let settings = Vignette {
            amount: 100.,
            highlights: 0.,
            color: [1., 0., 0.],
            ..Default::default()
        };
        let painted = vignette(&clear, settings, true).unwrap();
        assert_eq!(painted[(15, 15)], Rgba([0; 4]));
        assert!(painted[(0, 0)][3] > 200);
        assert_eq!(painted[(0, 0)][0], 255);
        let original = RgbaImage::from_pixel(31, 31, Rgba([40, 120, 180, 128]));
        let graded = vignette(&original, settings, false).unwrap();
        assert!(graded.pixels().all(|p| p[3] == 128));
        assert!(graded[(0, 0)][0] > original[(0, 0)][0]);
    }
    #[test]
    fn zero_strength_preserves_exact_pixels() {
        let image = RgbaImage::from_pixel(9, 9, Rgba([71, 83, 111, 37]));
        assert_eq!(
            vignette(
                &image,
                Vignette {
                    amount: 0.,
                    ..Default::default()
                },
                false
            )
            .unwrap(),
            image
        );
        assert_eq!(bloom(&image, 0., 24.).unwrap(), image);
        assert!(bloom(&image, 40., f64::INFINITY).is_err());
    }
}
