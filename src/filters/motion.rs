//! Shared premultiplied CPU motion blur for filters and adjustment previews.
use crate::{geometry::Sampling, render::pixel};
use image::{Rgba, RgbaImage};

pub(crate) fn apply(source: &RgbaImage, distance: f64, angle: f64) -> RgbaImage {
    let (sin, cos) = (-angle).to_radians().sin_cos();
    let steps = distance.ceil().max(1.) as usize + 1;
    let (w, h) = source.dimensions();
    RgbaImage::from_fn(w, h, |x, y| {
        let mut sum = [0.; 4];
        for i in 0..steps {
            let t = (i as f64 / (steps - 1) as f64 - 0.5) * distance;
            let p = pixel(
                source,
                [
                    (f64::from(x) + 0.5 + t * cos) / f64::from(w),
                    (f64::from(y) + 0.5 + t * sin) / f64::from(h),
                ],
                Sampling::Smooth,
            );
            for k in 0..3 {
                sum[k] += p[k] * p[3];
            }
            sum[3] += p[3];
        }
        if sum[3] > 0. {
            for k in 0..3 {
                sum[k] /= sum[3];
            }
        }
        sum[3] /= steps as f64;
        Rgba(sum.map(|v| (v.clamp(0., 1.) * 255.).round() as u8))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_spreads_alpha_without_hidden_color_at_full_and_subpixel_distances() {
        let mut source = RgbaImage::from_pixel(5, 5, Rgba([0, 0, 255, 0]));
        source[(2, 2)] = Rgba([255, 0, 0, 128]);
        for (distance, center_alpha, neighbor_alpha) in [(0.5, 96, 16), (2., 43, 43)] {
            for (angle, neighbor) in [(0., (3, 2)), (90., (2, 3))] {
                let result = apply(&source, distance, angle);
                assert_eq!(result[(2, 2)], Rgba([255, 0, 0, center_alpha]));
                assert_eq!(result[neighbor], Rgba([255, 0, 0, neighbor_alpha]));
                assert_eq!(result[(0, 0)], Rgba([0; 4]));
            }
        }
    }
}
