//! Reduce complex marching-ants contours at low zoom without changing coverage.
use super::*;
use std::borrow::Cow;

impl Selection {
    pub fn display_contours(&self, scale: f64) -> Result<Cow<'_, [Vec<Point>]>> {
        let complex = self.geometry.as_ref().map_or_else(
            || u64::from(self.pixels.width()) * u64::from(self.pixels.height()) > 1_000_000,
            |geometry| geometry.contours.iter().map(Vec::len).sum::<usize>() > 20_000,
        );
        if !complex || !scale.is_finite() || scale <= 0. || scale >= 1. {
            return self.outline_contours();
        }
        let mut step = 2_f64.powf(scale.max(1. / 4096.).log2().ceil()).min(1.);
        let area = f64::from(self.pixels.width()) * f64::from(self.pixels.height());
        step = step.min((40_000_000. / area).sqrt());
        let width = (f64::from(self.pixels.width()) * step).ceil().max(1.) as u32;
        let height = (f64::from(self.pixels.height()) * step).ceil().max(1.) as u32;
        validate_size(width, height)?;
        let mask = if let Some(geometry) = &self.geometry {
            let mut geometry = geometry.as_ref().clone();
            geometry.antialiased = true;
            for point in geometry.contours.iter_mut().flatten() {
                point[0] *= step;
                point[1] *= step;
            }
            let mut pixels = geometry.rasterize(width, height)?;
            for pixel in pixels.pixels_mut() {
                pixel[0] = if pixel[0] > 0 { 255 } else { 0 };
            }
            pixels
        } else if let Some(source) = self.pixels.dense() {
            // Max-pool the selected side of the edge, retaining thin foreground
            // pieces instead of dropping them when they occupy less than one pixel.
            let mut pixels = GrayImage::new(width, height);
            for (x, y, value) in source.enumerate_pixels() {
                if value[0] >= 128 {
                    let sx = ((f64::from(x) * step) as u32).min(width - 1);
                    let sy = ((f64::from(y) * step) as u32).min(height - 1);
                    pixels[(sx, sy)] = Luma([255]);
                }
            }
            pixels
        } else {
            return self.outline_contours();
        };
        let mut contours = SelectionGeometry::trace(&mask)?.contours;
        for point in contours.iter_mut().flatten() {
            point[0] /= step;
            point[1] /= step;
        }
        Ok(Cow::Owned(contours))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn low_zoom_coalesces_dense_edges_and_retains_thin_selected_parts() {
        let mask = GrayImage::from_fn(1100, 1000, |x, y| {
            Luma([if y == 250 || ((200..800).contains(&y) && x % 2 == 0) {
                255
            } else {
                0
            }])
        });
        let selection = Selection::from_mask(mask);
        let before = selection.pixels.clone();
        let outline = selection.display_contours(0.125).unwrap();
        assert!(outline.iter().map(Vec::len).sum::<usize>() < 100);
        assert!(
            outline
                .iter()
                .flatten()
                .any(|point| point[0] == 0. && point[1] <= 250.)
        );
        assert!(Arc::ptr_eq(&before, &selection.pixels));
    }
    #[test]
    fn simple_geometry_keeps_exact_subpixel_edges_at_any_zoom() {
        let selection = Selection::rectangle(100, 100, [10.25, 15.5], [80.5, 85.25], true);
        assert_eq!(
            selection.display_contours(0.125).unwrap(),
            selection.outline_contours().unwrap()
        );
    }
}
