use crate::{Result, document::validate_size};
use image::{ImageBuffer, Luma, RgbaImage};

type Plane = ImageBuffer<Luma<f32>, Vec<f32>>;

fn blur(plane: &Plane, radius: f32) -> Result<Vec<f32>> {
    Ok(match crate::render::gpu_float_blur(plane, radius)? {
        Some(blurred) => blurred.into_raw(),
        None => crate::effects::cpu::gaussian(
            plane.as_raw(),
            plane.width() as usize,
            plane.height() as usize,
            radius,
        ),
    })
}

/// Gaussian blur in premultiplied color, accelerated by Vulkan when available.
/// Keep fractional coverage until the final conversion to avoid color shifts at low alpha.
pub fn gaussian_rgba(image: &RgbaImage, radius: f32) -> Result<RgbaImage> {
    super::finishing::range(f64::from(radius), 0.1, 250.)?;
    validate_size(image.width(), image.height())?;
    let mut plane = Plane::from_fn(image.width(), image.height(), |x, y| {
        Luma([f32::from(image[(x, y)][3]) / 255.])
    });
    let alpha = blur(&plane, radius)?;
    let mut result = RgbaImage::new(image.width(), image.height());
    for (pixel, alpha) in result.pixels_mut().zip(&alpha) {
        pixel[3] = (alpha.clamp(0., 1.) * 255.).round() as u8;
    }
    for c in 0..3 {
        for (value, pixel) in plane.pixels_mut().zip(image.pixels()) {
            value[0] = f32::from(pixel[c]) / 255. * f32::from(pixel[3]) / 255.;
        }
        let colors = blur(&plane, radius)?;
        for ((pixel, color), alpha) in result.pixels_mut().zip(colors).zip(&alpha) {
            pixel[c] = if *alpha > 0. {
                ((color / alpha).clamp(0., 1.) * 255.).round() as u8
            } else {
                0
            };
        }
    }
    Ok(result)
}
