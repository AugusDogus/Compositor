use crate::{
    Result,
    document::{validate_canvas_size, validate_size},
    geometry::Point,
    invalid,
};
use crate::{selection_coverage::Coverage, selection_geometry::SelectionGeometry};
use image::{GrayImage, Luma, RgbaImage};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelectionMode {
    Replace,
    Add,
    Subtract,
    Intersect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    pub pixels: Arc<Coverage>,
    pub origin: Point,
    geometry: Option<Arc<SelectionGeometry>>,
}

impl Selection {
    /// Pixel-local contours for selection outlines. Geometry-backed selections
    /// retain subpixel edges; imported raster masks use the source's 50% threshold.
    pub fn outline_contours(&self) -> Result<std::borrow::Cow<'_, [Vec<Point>]>> {
        match &self.geometry {
            Some(geometry) => Ok(std::borrow::Cow::Borrowed(&geometry.contours)),
            None => Ok(std::borrow::Cow::Owned(
                SelectionGeometry::trace(self.pixels.dense().ok_or_else(|| {
                    invalid("Cannot draw the selection: its coverage has no outline geometry.")
                })?)?
                .contours,
            )),
        }
    }

    pub fn mirror(&mut self, horizontal: bool, axis: f64) {
        let index = usize::from(!horizontal);
        let size = if horizontal {
            self.pixels.width()
        } else {
            self.pixels.height()
        } as f64;
        self.origin[index] = 2. * axis - self.origin[index] - size;
        if let Some(geometry) = &mut self.geometry {
            for point in Arc::make_mut(geometry).contours.iter_mut().flatten() {
                point[index] = size - point[index];
            }
        }
        self.pixels = Arc::new(self.pixels.mirrored(horizontal));
    }

    pub fn from_coverage(
        pixels: &GrayImage,
        transform: crate::geometry::Transform,
        width: u32,
        height: u32,
        antialiased: bool,
    ) -> Result<Self> {
        validate_canvas_size(width, height)?;
        if !transform.valid() || pixels.width() == 0 || pixels.height() == 0 {
            return Err(invalid(
                "Selection source dimensions or transform are invalid.",
            ));
        }
        let mut geometry = SelectionGeometry::trace(pixels)?;
        geometry.antialiased = antialiased;
        for point in geometry.contours.iter_mut().flatten() {
            *point = transform.point([
                point[0] / pixels.width() as f64,
                point[1] / pixels.height() as f64,
            ]);
        }
        Self::from_geometry(
            geometry.clipped(width, height),
            [0., 0., width as f64, height as f64],
        )
    }

    pub fn from_mask(pixels: GrayImage) -> Self {
        Self {
            pixels: Arc::new(Coverage::raster(pixels)),
            origin: [0.; 2],
            geometry: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.pixels.dense().is_some() {
            validate_size(self.pixels.width(), self.pixels.height())?;
        } else {
            validate_canvas_size(self.pixels.width(), self.pixels.height())?;
        }
        if self
            .origin
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000. || v.fract() != 0.)
        {
            return Err(invalid(
                "Selection origin exceeds supported whole-pixel bounds.",
            ));
        }
        Ok(())
    }

    /// Rasterize a document-space region, including coverage outside the canvas.
    pub fn rasterize(bounds: [f64; 4], coverage: impl Fn(Point) -> f64) -> Result<Self> {
        if bounds
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
        {
            return Err(invalid("Selection bounds exceed supported coordinates."));
        }
        let origin = [bounds[0].floor(), bounds[1].floor()];
        let width = (bounds[2].ceil() - origin[0]).max(1.) as u32;
        let height = (bounds[3].ceil() - origin[1]).max(1.) as u32;
        validate_size(width, height)?;
        Ok(Self {
            pixels: Arc::new(Coverage::raster(GrayImage::from_fn(
                width,
                height,
                |x, y| {
                    Luma([
                        (coverage([origin[0] + x as f64 + 0.5, origin[1] + y as f64 + 0.5])
                            .clamp(0., 1.)
                            * 255.)
                            .round() as u8,
                    ])
                },
            ))),
            origin,
            geometry: None,
        })
    }

    pub fn bounds(&self) -> Option<[f64; 4]> {
        self.pixels.bounds().map(|b| {
            [
                b[0] + self.origin[0],
                b[1] + self.origin[1],
                b[2] + self.origin[0],
                b[3] + self.origin[1],
            ]
        })
    }

    pub fn coverage(&self, point: Point) -> f64 {
        let [x, y] = [point[0] - self.origin[0], point[1] - self.origin[1]];
        if !x.is_finite() || !y.is_finite() || x < 0. || y < 0. {
            return 0.;
        }
        self.pixels.value(x.floor() as u32, y.floor() as u32) as f64 / 255.
    }

    pub fn rectangle(width: u32, height: u32, a: Point, b: Point, ellipse: bool) -> Self {
        let left = a[0].min(b[0]);
        let top = a[1].min(b[1]);
        let right = a[0].max(b[0]);
        let bottom = a[1].max(b[1]);
        let geometry = if [left, top, right, bottom]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1_000_000.)
            && right > left
            && bottom > top
        {
            let bounds = [left, top, right, bottom];
            let geometry = if ellipse {
                SelectionGeometry::ellipse(bounds, false)
            } else {
                SelectionGeometry::rectangle(bounds, false)
            };
            Some(Arc::new(geometry.clipped(width, height)))
        } else {
            None
        };
        if u64::from(width) * u64::from(height) > crate::document::MAX_PIXELS {
            let geometry =
                geometry.unwrap_or_else(|| Arc::new(SelectionGeometry::polygon(&[], false)));
            return Self {
                pixels: Arc::new(Coverage::geometric(geometry.clone(), [width, height])),
                origin: [0.; 2],
                geometry: Some(geometry),
            };
        }
        let mut result = Self::from_mask(GrayImage::from_fn(width, height, |x, y| {
            let x = x as f64 + 0.5;
            let y = y as f64 + 0.5;
            let inside = if ellipse && right > left && bottom > top {
                ((x - (left + right) / 2.) / ((right - left) / 2.)).powi(2)
                    + ((y - (top + bottom) / 2.) / ((bottom - top) / 2.)).powi(2)
                    <= 1.
            } else {
                x >= left && x < right && y >= top && y < bottom
            };
            Luma([if inside { 255 } else { 0 }])
        }));
        result.geometry = geometry;
        result
    }

    /// Draw a marquee with optional smooth edges. Coordinates are whole document pixels.
    pub fn marquee(
        width: u32,
        height: u32,
        a: Point,
        b: Point,
        ellipse: bool,
        antialiased: bool,
    ) -> Result<Self> {
        validate_canvas_size(width, height)?;
        Self::validate_points(&[a, b])?;
        let a = a.map(f64::round);
        let b = b.map(f64::round);
        let bounds = [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[0].max(b[0]),
            a[1].max(b[1]),
        ];
        let geometry = if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
            SelectionGeometry::polygon(&[], antialiased)
        } else if ellipse {
            SelectionGeometry::ellipse(bounds, antialiased)
        } else {
            SelectionGeometry::rectangle(bounds, antialiased)
        };
        Self::from_geometry(
            geometry.clipped(width, height),
            [0., 0., width as f64, height as f64],
        )
    }

    pub fn polygon(width: u32, height: u32, points: &[Point], antialiased: bool) -> Result<Self> {
        validate_canvas_size(width, height)?;
        Self::validate_points(points)?;
        let geometry = SelectionGeometry::polygon(points, antialiased).clipped(width, height);
        Self::from_geometry(geometry, [0., 0., width as f64, height as f64])
    }

    fn validate_points(points: &[Point]) -> Result<()> {
        if points
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
        {
            return Err(invalid(
                "Selection coordinates exceed supported bounds. Draw closer to the canvas.",
            ));
        }
        Ok(())
    }

    fn from_geometry(geometry: SelectionGeometry, mut bounds: [f64; 4]) -> Result<Self> {
        Self::validate_points(&[[bounds[0], bounds[1]], [bounds[2], bounds[3]]])?;
        // Sparse canvases do not require a canvas-sized selection mask. Keep
        // ordinary masks unchanged and trim large ones to the drawn contours.
        let area = (bounds[2].ceil() - bounds[0].floor()) * (bounds[3].ceil() - bounds[1].floor());
        if area > crate::document::MAX_PIXELS as f64 {
            bounds = match geometry.bounds() {
                Some(b) => [
                    bounds[0].max(b[0]),
                    bounds[1].max(b[1]),
                    bounds[2].min(b[2]),
                    bounds[3].min(b[3]),
                ],
                None => [0., 0., 1., 1.],
            };
        }
        let origin = [bounds[0].floor(), bounds[1].floor()];
        let width = (bounds[2].ceil() - origin[0]).max(1.) as u32;
        let height = (bounds[3].ceil() - origin[1]).max(1.) as u32;
        validate_canvas_size(width, height)?;
        let geometry = Arc::new(geometry.translated([-origin[0], -origin[1]]));
        let pixels = if u64::from(width) * u64::from(height) > crate::document::MAX_PIXELS {
            Coverage::geometric(geometry.clone(), [width, height])
        } else {
            Coverage::raster(geometry.rasterize(width, height)?)
        };
        Ok(Self {
            pixels: Arc::new(pixels),
            origin,
            geometry: Some(geometry),
        })
    }

    pub fn resized(&self, amount: i32, width: u32, height: u32) -> Result<Self> {
        validate_canvas_size(width, height)?;
        if amount == 0 || !(-500..=500).contains(&amount) {
            return Err(invalid(
                "Selection expansion or contraction must be 1 to 500 pixels.",
            ));
        }
        let geometry = match &self.geometry {
            Some(geometry) => geometry.as_ref().clone(),
            None => SelectionGeometry::trace(
                self.pixels
                    .dense()
                    .ok_or_else(|| invalid("Selection geometry is missing."))?,
            )?,
        }
        .translated(self.origin)
        .resized(amount as f64);
        let geometry = if amount > 0 {
            geometry.clipped(width, height)
        } else {
            geometry
        };
        let bounds = if amount > 0 {
            [0., 0., width as f64, height as f64]
        } else {
            [
                self.origin[0],
                self.origin[1],
                self.origin[0] + self.pixels.width() as f64,
                self.origin[1] + self.pixels.height() as f64,
            ]
        };
        Self::from_geometry(geometry, bounds)
    }

    pub fn combine(&self, other: &Self, mode: SelectionMode) -> Result<Self> {
        if mode == SelectionMode::Replace {
            return Ok(other.clone());
        }
        let extent = |s: &Self| {
            [
                s.origin[0],
                s.origin[1],
                s.origin[0] + s.pixels.width() as f64,
                s.origin[1] + s.pixels.height() as f64,
            ]
        };
        let a = extent(self);
        let b = extent(other);
        let bounds = if mode == SelectionMode::Add {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        } else {
            a
        };
        if let (Some(a), Some(b)) = (&self.geometry, &other.geometry) {
            let geometry = a
                .translated(self.origin)
                .combine(&b.translated(other.origin), mode);
            return Self::from_geometry(geometry, bounds);
        }
        Self::rasterize(bounds, |p| {
            let a = self.coverage(p);
            let b = other.coverage(p);
            match mode {
                SelectionMode::Replace => b,
                SelectionMode::Add => a.max(b),
                SelectionMode::Subtract => (a - b).max(0.),
                SelectionMode::Intersect => a.min(b),
            }
        })
    }

    pub fn translated(&self, delta: Point) -> Result<Self> {
        let result = Self {
            pixels: self.pixels.clone(),
            geometry: self.geometry.clone(),
            origin: [
                self.origin[0] + delta[0].round(),
                self.origin[1] + delta[1].round(),
            ],
        };
        result.validate()?;
        Ok(result)
    }

    /// Retain geometric edges through affine or perspective pixel transforms.
    /// Raster-only coverage keeps its existing inverse-sampling behavior.
    pub fn mapped(
        &self,
        bounds: [f64; 4],
        forward: impl Fn(Point) -> Point,
        inverse: impl Fn(Point) -> Point,
    ) -> Result<Self> {
        if let Some(geometry) = &self.geometry {
            let mut geometry = geometry.translated(self.origin);
            for contour in &mut geometry.contours {
                for point in contour.iter_mut() {
                    *point = forward(*point);
                }
                Self::validate_points(contour)?;
            }
            Self::from_geometry(geometry, bounds)
        } else {
            Self::rasterize(bounds, |point| self.coverage(inverse(point)))
        }
    }

    pub fn invert(&self, width: u32, height: u32) -> Result<Self> {
        if let Some(geometry) = &self.geometry {
            let bounds = [0., 0., width as f64, height as f64];
            let canvas = SelectionGeometry::rectangle(bounds, geometry.antialiased);
            return Self::from_geometry(
                canvas.combine(&geometry.translated(self.origin), SelectionMode::Subtract),
                bounds,
            );
        }
        Self::rasterize([0., 0., width as f64, height as f64], |p| {
            1. - self.coverage(p)
        })
    }

    pub fn wand(
        image: &RgbaImage,
        x: u32,
        y: u32,
        tolerance: u8,
        contiguous: bool,
        radius: usize,
    ) -> crate::Result<Self> {
        let selection = crate::native_pixels::wand(image, x, y, tolerance, contiguous, radius)?;
        Self::from_coverage(
            &selection,
            crate::geometry::Transform::new(image.width(), image.height()),
            image.width(),
            image.height(),
            true,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    #[test]
    fn canvas_flips_keep_selection_geometry_in_sync_for_subsequent_edits() {
        for horizontal in [false, true] {
            let mut doc = crate::document::Document::new(100, 80).unwrap();
            let points = [[10., 10.], [30., 10.], [10., 30.]];
            doc.selection = Some(Selection::polygon(100, 80, &points, true).unwrap());
            let original = doc.selection.clone();
            crate::edits::flip_canvas(&mut doc, horizontal);
            let mirrored = points.map(|[x, y]| {
                if horizontal {
                    [100. - x, y]
                } else {
                    [x, 80. - y]
                }
            });
            let expected = Selection::polygon(100, 80, &mirrored, true).unwrap();
            let actual = doc.selection.as_ref().unwrap();
            let original_pixels = original.as_ref().unwrap().pixels.dense().unwrap();
            let mirrored_pixels = if horizontal {
                image::imageops::flip_horizontal(original_pixels)
            } else {
                image::imageops::flip_vertical(original_pixels)
            };
            assert_eq!(actual.pixels.dense().unwrap(), &mirrored_pixels);
            assert_eq!(
                actual.invert(100, 80).unwrap().pixels,
                expected.invert(100, 80).unwrap().pixels
            );
            assert_eq!(
                actual.resized(3, 100, 80).unwrap().pixels,
                expected.resized(3, 100, 80).unwrap().pixels
            );
            crate::edits::flip_canvas(&mut doc, horizontal);
            assert_eq!(doc.selection, original);
        }
    }

    #[test]
    fn adjacent_smooth_polygons_combine_without_a_half_coverage_seam() {
        let a = Selection::polygon(10, 10, &[[0., 0.], [10., 0.], [0., 10.]], true).unwrap();
        let b = Selection::polygon(10, 10, &[[10., 0.], [10., 10.], [0., 10.]], true).unwrap();
        assert!(
            a.pixels
                .dense()
                .unwrap()
                .pixels()
                .any(|p| p[0] > 0 && p[0] < 255)
        );
        let union = a.combine(&b, SelectionMode::Add).unwrap();
        assert!(union.pixels.dense().unwrap().pixels().all(|p| p[0] == 255));
        assert!(
            a.combine(&b, SelectionMode::Intersect)
                .unwrap()
                .bounds()
                .is_none()
        );
        assert_eq!(
            a.combine(&b, SelectionMode::Subtract).unwrap().pixels,
            a.pixels
        );
        let moved = union.translated([-20., 3.]).unwrap();
        assert_eq!(
            moved
                .combine(&a, SelectionMode::Add)
                .unwrap()
                .coverage([-15., 8.]),
            1.
        );
    }

    #[test]
    fn expand_and_contract_use_round_outlines_and_keep_explicit_empty_selections() {
        let square = Selection::marquee(100, 100, [40., 40.], [60., 60.], false, true).unwrap();
        let grown = square.resized(5, 100, 100).unwrap();
        assert_eq!(grown.bounds(), Some([35., 35., 65., 65.]));
        assert_eq!(grown.coverage([37., 50.]), 1.);
        assert_eq!(grown.coverage([35., 35.]), 0.);
        let shrunk = grown.resized(-8, 100, 100).unwrap();
        assert_eq!(shrunk.bounds(), Some([43., 43., 57., 57.]));
        let all = Selection::rectangle(100, 100, [0., 0.], [100., 100.], false);
        assert_eq!(all.resized(10, 100, 100).unwrap().bounds(), all.bounds());
        let inner = all.resized(-10, 100, 100).unwrap();
        assert_eq!(inner.bounds(), Some([10., 10., 90., 90.]));
        assert!(inner.resized(-45, 100, 100).unwrap().bounds().is_none());
        assert!(all.resized(501, 100, 100).is_err());
        let moved = square.translated([-50., 0.]).unwrap();
        assert_eq!(
            moved.resized(-2, 100, 100).unwrap().bounds(),
            Some([-8., 42., 8., 58.])
        );
    }

    #[test]
    fn expansion_traces_raster_masks_at_half_coverage_and_preserves_holes() {
        let mask = Selection::from_mask(GrayImage::from_fn(30, 30, |x, y| {
            Luma([
                if (5..25).contains(&x)
                    && (5..25).contains(&y)
                    && !((10..20).contains(&x) && (10..20).contains(&y))
                {
                    128
                } else {
                    127
                },
            ])
        }));
        let grown = mask.resized(2, 30, 30).unwrap();
        assert_eq!(grown.bounds(), Some([3., 3., 27., 27.]));
        assert_eq!(grown.coverage([10., 15.]), 1.);
        assert_eq!(grown.coverage([15., 15.]), 0.);
    }
    #[test]
    fn path_edges_can_be_smooth_or_hard_and_use_nonzero_winding() {
        let triangle = [[0., 0.], [100., 0.], [0., 100.]];
        let smooth = Selection::polygon(100, 100, &triangle, true).unwrap();
        let hard = Selection::polygon(100, 100, &triangle, false).unwrap();
        assert!(
            (0..100).any(|x| (1..255).contains(&smooth.pixels.dense().unwrap()[(x, 99 - x)][0]))
        );
        assert!(
            hard.pixels
                .dense()
                .unwrap()
                .pixels()
                .all(|p| p[0] == 0 || p[0] == 255)
        );
        let twice = [
            [1., 1.],
            [5., 1.],
            [5., 5.],
            [1., 5.],
            [1., 1.],
            [5., 1.],
            [5., 5.],
            [1., 5.],
        ];
        assert_eq!(
            Selection::polygon(6, 6, &twice, true)
                .unwrap()
                .coverage([3., 3.]),
            1.
        );
        let ellipse = Selection::marquee(20, 20, [0., 0.], [20., 20.], true, true).unwrap();
        assert_eq!(ellipse.coverage([0., 0.]), 0.);
        assert_eq!(ellipse.coverage([10., 10.]), 1.);
        assert!(
            ellipse
                .pixels
                .dense()
                .unwrap()
                .pixels()
                .any(|p| (1..255).contains(&p[0]))
        );
        assert!(Selection::polygon(20, 20, &[[f64::NAN, 0.]], true).is_err());
    }
    #[test]
    fn translated_selection_retains_coverage_without_copying_pixels() {
        let source = Selection::from_mask(GrayImage::from_raw(3, 1, vec![0, 128, 255]).unwrap());
        let moved = source.translated([-10., 7.]).unwrap();
        assert!(Arc::ptr_eq(&source.pixels, &moved.pixels));
        assert_eq!(moved.bounds(), Some([-9., 7., -7., 8.]));
        assert_eq!(moved.coverage([-8.5, 7.5]), 128. / 255.);
        assert_eq!(moved.translated([10., -7.]).unwrap(), source);
        assert!(source.translated([f64::NAN, 0.]).is_err());
        assert!(source.translated([1_000_001., 0.]).is_err());
    }

    #[test]
    fn boolean_operations_align_displaced_masks_and_invert_within_canvas() {
        let a = Selection::from_mask(GrayImage::from_pixel(2, 1, Luma([255])));
        let b = a.translated([-1., 0.]).unwrap();
        let union = a.combine(&b, SelectionMode::Add).unwrap();
        assert_eq!(union.bounds(), Some([-1., 0., 2., 1.]));
        assert_eq!(
            a.combine(&b, SelectionMode::Subtract).unwrap().bounds(),
            Some([1., 0., 2., 1.])
        );
        assert_eq!(
            a.combine(&b, SelectionMode::Intersect).unwrap().bounds(),
            Some([0., 0., 1., 1.])
        );
        assert_eq!(
            b.invert(3, 1).unwrap().pixels.dense().unwrap().as_raw(),
            &[0, 255, 255]
        );
        let distant = a.translated([40_000., 0.]).unwrap();
        assert!(a.combine(&distant, SelectionMode::Add).is_err());
    }
    #[test]
    fn wand_averages_premultiplied_colors_and_checks_seed_bounds() {
        let pixels =
            RgbaImage::from_fn(3, 1, |x, _| Rgba([if x == 1 { 100 } else { 0 }, 0, 0, 255]));
        assert_eq!(
            Selection::wand(&pixels, 1, 0, 34, false, 1)
                .unwrap()
                .pixels
                .dense()
                .unwrap()
                .as_raw(),
            &[255, 0, 255]
        );
        assert_eq!(
            Selection::wand(&pixels, 1, 0, 34, true, 1)
                .unwrap()
                .pixels
                .dense()
                .unwrap()
                .as_raw(),
            &[0, 0, 0]
        );
        assert_eq!(
            Selection::wand(&pixels, 3, 0, 255, true, 0)
                .unwrap()
                .pixels
                .dense()
                .unwrap()
                .as_raw(),
            &[0, 0, 0]
        );
        let transparent =
            RgbaImage::from_fn(2, 1, |x, _| Rgba([if x == 0 { 255 } else { 0 }, 0, 0, 0]));
        assert_eq!(
            Selection::wand(&transparent, 0, 0, 0, true, 0)
                .unwrap()
                .pixels
                .dense()
                .unwrap()
                .as_raw(),
            &[255, 255]
        );
    }
    #[test]
    fn wand_does_not_cross_different_pixels_unless_noncontiguous() {
        let pixels =
            RgbaImage::from_fn(3, 1, |x, _| Rgba([if x == 1 { 0 } else { 255 }, 0, 0, 255]));
        assert_eq!(
            Selection::wand(&pixels, 0, 0, 0, true, 0)
                .unwrap()
                .pixels
                .dense()
                .unwrap()
                .as_raw(),
            &[255, 0, 0]
        );
        assert_eq!(
            Selection::wand(&pixels, 0, 0, 0, false, 0)
                .unwrap()
                .pixels
                .dense()
                .unwrap()
                .as_raw(),
            &[255, 0, 255]
        );
    }
    #[test]
    fn subtract_preserves_the_unselected_region() {
        let a = Selection::rectangle(4, 1, [0., 0.], [4., 1.], false);
        let b = Selection::rectangle(4, 1, [1., 0.], [3., 1.], false);
        assert_eq!(
            a.combine(&b, SelectionMode::Subtract)
                .unwrap()
                .pixels
                .dense()
                .unwrap()
                .as_raw(),
            &[255, 0, 0, 255]
        );
    }
}
