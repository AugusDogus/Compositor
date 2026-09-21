use crate::{
    document::{Document, Layer},
    geometry::{Point, Sampling},
    render,
};

pub(super) enum Source {
    Blur(Box<super::blur::Blur>),
    Clone { canvas: [u32; 2], input: CloneInput },
}

pub(super) enum CloneInput {
    Layer(Box<Layer>),
    Composite(Box<render::Sampler<'static>>),
}

impl Source {
    pub fn clone_snapshot(doc: &Document, layer: &Layer, sample_all: bool) -> crate::Result<Self> {
        Ok(Self::Clone {
            canvas: [doc.width, doc.height],
            input: if sample_all {
                CloneInput::Composite(Box::new(render::Sampler::new(doc)?))
            } else {
                CloneInput::Layer(Box::new(layer.clone()))
            },
        })
    }

    pub fn sample(&self, point: Point, sampling: Sampling) -> [f64; 4] {
        let (canvas, input) = match self {
            Self::Blur(source) => return source.sample(point, sampling),
            Self::Clone { canvas, input } => (canvas, input),
        };
        if point
            .iter()
            .zip(canvas)
            .any(|(p, size)| !(0. ..*size as f64).contains(p))
        {
            return [0.; 4];
        }
        // Reconstruct the immutable document-resolution snapshot lazily. Match
        // its 8-bit quantization and premultiplied interpolation at pixel centers.
        let sample = |x: f64, y: f64| {
            let p = [
                x.clamp(0., canvas[0] as f64 - 1.) + 0.5,
                y.clamp(0., canvas[1] as f64 - 1.) + 0.5,
            ];
            let color = match input {
                CloneInput::Composite(sampler) => sampler.sample(p),
                CloneInput::Layer(layer) => layer.raster().map_or([0.; 4], |image| {
                    render::pixel(image, layer.transform.unit(p), layer.transform.sampling)
                }),
            };
            color.map(|v| (v.clamp(0., 1.) * 255.).round() / 255.)
        };
        if sampling == Sampling::Nearest {
            return sample(point[0].floor(), point[1].floor());
        }
        let [x, y] = [point[0] - 0.5, point[1] - 0.5];
        let [ix, iy] = [x.floor(), y.floor()];
        let [fx, fy] = [x - ix, y - iy];
        let mut out = [0.; 4];
        for (dx, dy, weight) in [
            (0., 0., (1. - fx) * (1. - fy)),
            (1., 0., fx * (1. - fy)),
            (0., 1., (1. - fx) * fy),
            (1., 1., fx * fy),
        ] {
            let p = sample(ix + dx, iy + dy);
            for channel in 0..3 {
                out[channel] += p[channel] * p[3] * weight;
            }
            out[3] += p[3] * weight;
        }
        if out[3] > 0. {
            for channel in 0..3 {
                out[channel] /= out[3];
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{LayerContent, Mask};
    use image::{GrayImage, Luma, Rgba, RgbaImage};
    use std::sync::Arc;

    #[test]
    fn lazy_clone_matches_immutable_full_canvas_sampling() {
        let mut doc = Document::new(23, 19).unwrap();
        let layer = &mut doc.layers[0];
        layer.transform = crate::geometry::Transform::new(14, 12);
        layer.transform.origin = [3., 2.];
        layer.transform.rotation = 27.;
        layer.transform.flip_x = true;
        layer.opacity = 0.6;
        layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(31, 27, |x, y| {
            Rgba([(x * 7) as u8, (y * 9) as u8, 83, (x * 8) as u8])
        }))));
        layer.mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([123]))),
            enabled: true,
            linked: true,
            placement: None,
        });
        for all in [false, true] {
            let layer = &doc.layers[0];
            let source = Source::clone_snapshot(&doc, layer, all).unwrap();
            let expected = if all {
                render::render(&doc, doc.width, doc.height).unwrap()
            } else {
                RgbaImage::from_fn(doc.width, doc.height, |x, y| {
                    Rgba(
                        render::pixel(
                            layer.raster().unwrap(),
                            layer.transform.unit([x as f64 + 0.5, y as f64 + 0.5]),
                            layer.transform.sampling,
                        )
                        .map(|v| (v * 255.).round() as u8),
                    )
                })
            };
            for sampling in [Sampling::Nearest, Sampling::Smooth, Sampling::High] {
                for y in 0..22 {
                    for x in 0..26 {
                        let point = [x as f64 - 1.3, y as f64 - 0.7];
                        let actual = source.sample(point, sampling);
                        let expected = render::pixel(
                            &expected,
                            [point[0] / doc.width as f64, point[1] / doc.height as f64],
                            sampling,
                        );
                        for (a, b) in actual.into_iter().zip(expected) {
                            assert!((a - b).abs() < 1e-10, "{point:?}: {a} vs {b}");
                        }
                    }
                }
            }
        }
    }
}
