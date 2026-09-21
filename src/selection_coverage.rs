use crate::{geometry::Point, selection_geometry::SelectionGeometry};
use image::GrayImage;
use std::sync::Arc;

/// Large geometric selections keep their paths instead of a canvas-sized mask.
/// Ordinary and imported raster masks retain their exact original samples.
#[derive(Clone, Debug, PartialEq)]
pub struct Coverage(Storage);

#[derive(Clone, Debug, PartialEq)]
enum Storage {
    Raster(GrayImage),
    Geometric {
        geometry: Arc<SelectionGeometry>,
        path: Option<tiny_skia::Path>,
        size: [u32; 2],
        flipped: [bool; 2],
    },
}

impl Coverage {
    /// Rasterize a geometric outline only at the displayed resolution. The view
    /// must not sample every polygon edge separately for every screen pixel.
    pub fn rasterize_geometry(
        &self,
        dimensions: [u32; 2],
        origin: Point,
        step: f64,
    ) -> crate::Result<Option<GrayImage>> {
        let Storage::Geometric {
            geometry,
            path,
            size,
            flipped,
        } = &self.0
        else {
            return Ok(None);
        };
        crate::document::validate_size(dimensions[0], dimensions[1])?;
        if !step.is_finite() || step <= 0. || origin.iter().any(|v| !v.is_finite()) {
            return Err(crate::invalid(
                "The selection viewport coordinates are invalid.",
            ));
        }
        let mut mask = tiny_skia::Mask::new(dimensions[0], dimensions[1]).ok_or_else(|| {
            crate::invalid(
                "Cannot allocate the selection outline. Reduce the window size and retry.",
            )
        })?;
        if let Some(path) = path {
            let scale = flipped.map(|flip| (if flip { -1. } else { 1. }) / step);
            let shift: [f64; 2] = std::array::from_fn(|axis| {
                ((if flipped[axis] { size[axis] as f64 } else { 0. }) - origin[axis]) / step
            });
            mask.fill_path(
                path,
                tiny_skia::FillRule::Winding,
                geometry.antialiased,
                tiny_skia::Transform::from_row(
                    scale[0] as f32,
                    0.,
                    0.,
                    scale[1] as f32,
                    shift[0] as f32,
                    shift[1] as f32,
                ),
            );
        }
        GrayImage::from_raw(dimensions[0], dimensions[1], mask.data().to_vec())
            .map(Some)
            .ok_or_else(|| {
                crate::invalid("Selection outline dimensions do not match its coverage.")
            })
    }

    pub(crate) fn raster(image: GrayImage) -> Self {
        Self(Storage::Raster(image))
    }

    pub(crate) fn geometric(geometry: Arc<SelectionGeometry>, size: [u32; 2]) -> Self {
        let path = geometry.path();
        Self(Storage::Geometric {
            geometry,
            path,
            size,
            flipped: [false; 2],
        })
    }

    pub fn dense(&self) -> Option<&GrayImage> {
        match &self.0 {
            Storage::Raster(image) => Some(image),
            Storage::Geometric { .. } => None,
        }
    }

    pub fn dimensions(&self) -> (u32, u32) {
        match &self.0 {
            Storage::Raster(image) => image.dimensions(),
            Storage::Geometric { size, .. } => (size[0], size[1]),
        }
    }
    pub fn width(&self) -> u32 {
        self.dimensions().0
    }
    pub fn height(&self) -> u32 {
        self.dimensions().1
    }

    pub(crate) fn mirrored(&self, horizontal: bool) -> Self {
        match &self.0 {
            Storage::Raster(image) => Self::raster(if horizontal {
                image::imageops::flip_horizontal(image)
            } else {
                image::imageops::flip_vertical(image)
            }),
            Storage::Geometric { .. } => {
                let mut result = self.clone();
                if let Storage::Geometric { flipped, .. } = &mut result.0 {
                    flipped[usize::from(!horizontal)] ^= true;
                }
                result
            }
        }
    }

    pub(crate) fn bounds(&self) -> Option<[f64; 4]> {
        match &self.0 {
            Storage::Raster(image) => {
                let mut b = [image.width(), image.height(), 0, 0];
                for (x, y, p) in image.enumerate_pixels() {
                    if p[0] > 0 {
                        b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
                    }
                }
                (b[0] < b[2] && b[1] < b[3]).then(|| b.map(f64::from))
            }
            Storage::Geometric {
                geometry,
                size,
                flipped,
                ..
            } => {
                let mut bounds = geometry.bounds()?;
                for axis in 0..2 {
                    let left = bounds[axis].floor().max(0.);
                    let right = bounds[axis + 2].ceil().min(size[axis] as f64);
                    bounds[axis] = if flipped[axis] {
                        size[axis] as f64 - right
                    } else {
                        left
                    };
                    bounds[axis + 2] = if flipped[axis] {
                        size[axis] as f64 - left
                    } else {
                        right
                    };
                }
                (bounds[0] < bounds[2] && bounds[1] < bounds[3]).then_some(bounds)
            }
        }
    }

    pub(crate) fn value(&self, x: u32, y: u32) -> u8 {
        if x >= self.width() || y >= self.height() {
            return 0;
        }
        match &self.0 {
            Storage::Raster(image) => image[(x, y)][0],
            Storage::Geometric {
                geometry,
                path,
                size,
                flipped,
            } => {
                let pixel = [
                    if flipped[0] { size[0] - 1 - x } else { x },
                    if flipped[1] { size[1] - 1 - y } else { y },
                ];
                let p = [pixel[0] as f64 + 0.5, pixel[1] as f64 + 0.5];
                let mut winding = 0;
                let mut edge = false;
                for contour in &geometry.contours {
                    for (a, b) in contour
                        .iter()
                        .zip(contour.iter().cycle().skip(1))
                        .take(contour.len())
                    {
                        let d = [b[0] - a[0], b[1] - a[1]];
                        let length = d[0] * d[0] + d[1] * d[1];
                        let t = if length > 0. {
                            (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length).clamp(0., 1.)
                        } else {
                            0.
                        };
                        // Only pixels close to an edge need antialias rasterization.
                        edge |= (p[0] - a[0] - t * d[0]).powi(2) + (p[1] - a[1] - t * d[1]).powi(2)
                            <= 0.500_001;
                        let cross = d[0] * (p[1] - a[1]) - d[1] * (p[0] - a[0]);
                        if a[1] <= p[1] && b[1] > p[1] && cross > 0. {
                            winding += 1;
                        }
                        if a[1] > p[1] && b[1] <= p[1] && cross < 0. {
                            winding -= 1;
                        }
                    }
                }
                if !edge {
                    return if winding != 0 { 255 } else { 0 };
                }
                let Some(path) = path else {
                    return 0;
                };
                // These constant nonzero dimensions always form a valid mask.
                let mut mask =
                    tiny_skia::Mask::new(1, 1).expect("one-pixel mask dimensions are valid");
                mask.fill_path(
                    path,
                    tiny_skia::FillRule::Winding,
                    geometry.antialiased,
                    tiny_skia::Transform::from_translate(-(pixel[0] as f32), -(pixel[1] as f32)),
                );
                mask.data()[0]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_coverage_matches_dense_interior_holes_and_antialiased_edges() {
        let rectangle = SelectionGeometry::rectangle([2., 3., 25., 27.], true);
        let hole = SelectionGeometry::rectangle([7., 8., 18., 19.], true);
        for geometry in [
            rectangle.combine(&hole, crate::selection::SelectionMode::Subtract),
            SelectionGeometry::ellipse([2., 3., 27., 29.], true),
            SelectionGeometry::polygon(&[[1., 2.], [28., 8.], [5., 27.]], true),
        ] {
            let dense = geometry.rasterize(30, 30).unwrap();
            let sparse = Coverage::geometric(Arc::new(geometry), [30, 30]);
            for (x, y, expected) in dense.enumerate_pixels() {
                let actual = sparse.value(x, y);
                assert!(
                    actual.abs_diff(expected[0]) <= 1,
                    "({x},{y}): {actual} vs {}",
                    expected[0]
                );
                assert_eq!(sparse.mirrored(true).value(29 - x, y), actual);
                assert_eq!(sparse.mirrored(false).value(x, 29 - y), actual);
            }
        }
    }
}
