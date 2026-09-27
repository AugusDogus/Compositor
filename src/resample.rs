mod footprint;
pub(crate) use footprint::projective_size;

use crate::{
    geometry::{Point, Sampling, Transform},
    native_pixels, render,
};
use image::RgbaImage;
use std::borrow::Cow;

/// Prefilters reductions before reconstructing transformed pixels. Filtering premultiplied
/// color prevents invisible RGB values from bleeding into the result.
pub(crate) struct RasterSampler<'a> {
    image: Cow<'a, RgbaImage>,
    sampling: Sampling,
}

impl<'a> RasterSampler<'a> {
    pub fn new(source: &'a RgbaImage, transform: Transform, scale: Point) -> Self {
        let mut image = Cow::Borrowed(source);
        if transform.sampling == Sampling::High {
            let size = if transform.warp.is_some() {
                projective_size(transform, source.dimensions(), scale)
            } else {
                let (sin, cos) = transform.rotation.to_radians().sin_cos();
                let w = transform.size[0] * (cos * scale[0]).hypot(sin * scale[1]);
                let h = transform.size[1] * (sin * scale[0]).hypot(cos * scale[1]);
                (
                    (w.ceil().max(1.) as u32).min(source.width()),
                    (h.ceil().max(1.) as u32).min(source.height()),
                )
            };
            if size != source.dimensions() {
                image = Cow::Owned(native_pixels::unpremultiply(image::imageops::resize(
                    &native_pixels::premultiply(source),
                    size.0,
                    size.1,
                    image::imageops::FilterType::Lanczos3,
                )));
            }
        }
        Self {
            image,
            sampling: transform.sampling,
        }
    }

    pub fn sample(&self, unit: Point) -> [f64; 4] {
        if self.sampling != Sampling::High {
            return render::pixel(&self.image, unit, self.sampling);
        }
        if unit.iter().any(|v| !(0. ..1.).contains(v)) {
            return [0.; 4];
        }
        let x = unit[0] * self.image.width() as f64 - 0.5;
        let y = unit[1] * self.image.height() as f64 - 0.5;
        let mut sum = [0.; 4];
        let mut weight_sum = 0.;
        for iy in y.floor() as i64 - 2..=y.floor() as i64 + 3 {
            let wy = lanczos(y - iy as f64);
            for ix in x.floor() as i64 - 2..=x.floor() as i64 + 3 {
                let weight = wy * lanczos(x - ix as f64);
                let p = self.image[(
                    ix.clamp(0, self.image.width() as i64 - 1) as u32,
                    iy.clamp(0, self.image.height() as i64 - 1) as u32,
                )]
                    .0
                    .map(|v| v as f64 / 255.);
                for k in 0..3 {
                    sum[k] += p[k] * p[3] * weight;
                }
                sum[3] += p[3] * weight;
                weight_sum += weight;
            }
        }
        if sum[3] <= 0. || weight_sum <= 0. {
            return [0.; 4];
        }
        for k in 0..3 {
            sum[k] = (sum[k] / sum[3]).clamp(0., 1.);
        }
        sum[3] = (sum[3] / weight_sum).clamp(0., 1.);
        sum
    }
}

fn lanczos(x: f64) -> f64 {
    if x.abs() < 1e-10 {
        return 1.;
    }
    if x.abs() >= 3. {
        return 0.;
    }
    let x = x * std::f64::consts::PI;
    x.sin() / x * (x / 3.).sin() / (x / 3.)
}

#[cfg(test)]
mod projective_tests {
    use super::*;
    use crate::geometry::projective::Projective;
    use image::Rgba;

    #[test]
    fn projective_high_downsampling_prefilters_raster_sampler() {
        let source = RgbaImage::from_fn(255, 255, |x, y| {
            let v = if (x + y) % 2 == 0 { 0 } else { 255 };
            Rgba([v, v, v, 255])
        });
        for warp in [
            Projective::IDENTITY,
            Projective::new([[0., 0.], [1., 0.], [0.99, 1.], [0.01, 1.]]).unwrap(),
        ] {
            let transform = Transform {
                warp: Some(warp),
                ..Transform::new(255, 255)
            };
            let sampler = RasterSampler::new(&source, transform, [1. / 15.; 2]);
            for y in 0..17 {
                for x in 0..17 {
                    let point = [(f64::from(x) + 0.5) * 15., (f64::from(y) + 0.5) * 15.];
                    let sample = sampler.sample(transform.unit(point));
                    assert!((0.47..=0.53).contains(&sample[0]), "{sample:?}");
                }
            }
        }
    }
}
