mod mapping;
pub(crate) use mapping::Mapping;

use crate::{
    Result,
    document::{Document, LayerContent, validate_size},
    geometry::{Point, Transform},
    invalid, render, transform,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(crate) struct Homography {
    matrix: [f64; 9],
}
impl Homography {
    pub(crate) fn new(c: [Point; 4]) -> Result<Self> {
        if c.iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
        {
            return Err(invalid("Distortion corners exceed supported bounds."));
        }
        let mut sign = 0_f64;
        for i in 0..4 {
            let a = c[i];
            let b = c[(i + 1) % 4];
            let d = c[(i + 2) % 4];
            let cross = (b[0] - a[0]) * (d[1] - b[1]) - (b[1] - a[1]) * (d[0] - b[0]);
            if cross.abs() <= 0.01 || (sign != 0. && cross.signum() != sign) {
                return Err(invalid(
                    "Distortion corners must form a convex shape without crossing edges.",
                ));
            }
            sign = cross.signum();
        }
        let sx = c[0][0] - c[1][0] + c[2][0] - c[3][0];
        let sy = c[0][1] - c[1][1] + c[2][1] - c[3][1];
        let dx1 = c[1][0] - c[2][0];
        let dx2 = c[3][0] - c[2][0];
        let dy1 = c[1][1] - c[2][1];
        let dy2 = c[3][1] - c[2][1];
        let denominator = dx1 * dy2 - dx2 * dy1;
        let (g, h) = if sx.abs() > 1e-9 || sy.abs() > 1e-9 {
            if denominator.abs() <= 1e-12 {
                return Err(invalid("Distortion collapsed to a line."));
            }
            (
                (sx * dy2 - dx2 * sy) / denominator,
                (dx1 * sy - sx * dy1) / denominator,
            )
        } else {
            (0., 0.)
        };
        Ok(Self {
            matrix: [
                c[1][0] - c[0][0] + g * c[1][0],
                c[3][0] - c[0][0] + h * c[3][0],
                c[0][0],
                c[1][1] - c[0][1] + g * c[1][1],
                c[3][1] - c[0][1] + h * c[3][1],
                c[0][1],
                g,
                h,
                1.,
            ],
        })
    }
    pub(crate) fn map(self, p: Point) -> Point {
        let m = self.matrix;
        let w = m[6] * p[0] + m[7] * p[1] + m[8];
        [
            (m[0] * p[0] + m[1] * p[1] + m[2]) / w,
            (m[3] * p[0] + m[4] * p[1] + m[5]) / w,
        ]
    }
    pub(crate) fn inverse(self) -> Result<Self> {
        let [a, b, c, d, e, f, g, h, i] = self.matrix;
        let determinant = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
        if determinant.abs() < 1e-12 {
            return Err(invalid("Distortion is not invertible."));
        }
        Ok(Self {
            matrix: [
                e * i - f * h,
                c * h - b * i,
                b * f - c * e,
                f * g - d * i,
                a * i - c * g,
                c * d - a * f,
                d * h - e * g,
                b * g - a * h,
                a * e - b * d,
            ]
            .map(|v| v / determinant),
        })
    }
}

/// Convex quads use perspective; folded quads use two nondegenerate triangles.
pub fn usable_corners(corners: [Point; 4]) -> bool {
    Mapping::new(corners).is_ok()
}

pub fn apply(
    doc: &mut Document,
    mut bounds: Transform,
    corners: [Point; 4],
    mask_target: bool,
) -> Result<()> {
    bounds.flip_x = false;
    bounds.flip_y = false;
    let warp = Mapping::new(corners)?;
    let ids = transform::target_ids(doc);
    let mask_target = mask_target && doc.selected.len() == 1;
    for layer in doc.layers.iter_mut().filter(|l| ids.contains(&l.id)) {
        let mask_only = mask_target && layer.mask.as_ref().is_some_and(|m| !m.linked);
        let old = if mask_only {
            layer
                .mask
                .as_ref()
                .and_then(|m| m.placement)
                .unwrap_or(layer.transform)
        } else {
            layer.transform
        };
        let points = warp.mapped_outline(
            [[0., 0.], [1., 0.], [1., 1.], [0., 1.]].map(|p| bounds.unit(old.geometry_point(p))),
        );
        let left = points
            .iter()
            .map(|p| p[0])
            .fold(f64::INFINITY, f64::min)
            .floor();
        let top = points
            .iter()
            .map(|p| p[1])
            .fold(f64::INFINITY, f64::min)
            .floor();
        let right = points
            .iter()
            .map(|p| p[0])
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil();
        let bottom = points
            .iter()
            .map(|p| p[1])
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil();
        if ![left, top, right, bottom].iter().all(|v| v.is_finite()) {
            return Err(invalid(
                "The distortion projects pixels beyond a finite image.",
            ));
        }
        let w = (right - left) as u32;
        let h = (bottom - top) as u32;
        validate_size(w, h)?;
        let placed = Transform {
            origin: [left, top],
            ..Transform::new(w, h)
        };
        let source_points =
            |x, y| warp.inverse_points([left + x as f64 + 0.5, top + y as f64 + 0.5]);
        if !mask_only {
            if let Some(image) = layer.raster() {
                let pixels = RgbaImage::from_fn(w, h, |x, y| {
                    let pixel =
                        source_points(x, y)
                            .into_iter()
                            .flatten()
                            .fold([0.; 4], |bottom, point| {
                                crate::blend::Blend::Normal.composite(
                                    bottom,
                                    render::pixel(
                                        image,
                                        old.unit(bounds.point(point)),
                                        old.sampling,
                                    ),
                                )
                            });
                    Rgba(pixel.map(|v| (v * 255.).round() as u8))
                });
                layer.content = LayerContent::Raster(Some(Arc::new(pixels)));
            }
            layer.transform = placed;
            layer.shape = None;
            layer.text = None;
        }
        if let Some(mask) = &mut layer.mask {
            if mask_only || mask.linked {
                let t = mask.placement.unwrap_or(old);
                let background = if mask.placement.is_some() {
                    mask.background()
                } else {
                    0.
                };
                let pixels = GrayImage::from_fn(w, h, |x, y| {
                    let Some(point) = source_points(x, y).into_iter().flatten().last() else {
                        return Luma([0]);
                    };
                    let u = t.unit(bounds.point(point));
                    if u.iter().any(|v| !(0. ..1.).contains(v)) {
                        Luma([(background * 255.) as u8])
                    } else {
                        mask.pixels[(
                            (u[0] * mask.pixels.width() as f64) as u32,
                            (u[1] * mask.pixels.height() as f64) as u32,
                        )]
                    }
                });
                mask.pixels = Arc::new(pixels);
                mask.placement = if mask_only { Some(placed) } else { None };
            } else if mask.placement.is_none() {
                mask.placement = Some(old);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projective_inverse_recovers_corners_and_interior() {
        let c = [[10., 20.], [150., 10.], [130., 100.], [30., 120.]];
        let h = Homography::new(c).unwrap();
        let inverse = h.inverse().unwrap();
        for p in [[0., 0.], [1., 0.], [1., 1.], [0., 1.], [0.3, 0.7]] {
            let q = inverse.map(h.map(p));
            assert!((q[0] - p[0]).abs() < 1e-9);
            assert!((q[1] - p[1]).abs() < 1e-9);
        }
        assert!(Homography::new([[0., 0.], [10., 10.], [10., 0.], [0., 10.]]).is_err());
    }
    #[test]
    fn identity_warp_preserves_source_pixels() {
        let mut doc = Document::new(4, 4).unwrap();
        crate::edits::fill(&mut doc, [200, 100, 50, 255], false, false).unwrap();
        let old = doc.layers[0].raster().unwrap().clone();
        apply(
            &mut doc,
            Transform::new(4, 4),
            [[0., 0.], [4., 0.], [4., 4.], [0., 4.]],
            false,
        )
        .unwrap();
        assert_eq!(doc.layers[0].raster().unwrap(), &old);
    }
}
