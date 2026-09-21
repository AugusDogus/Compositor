use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Feather {
    pub source: Arc<Selection>,
    pub radius: f64,
}

impl Feather {
    pub(super) fn translated(&self, delta: Point) -> Result<Self> {
        Ok(Self {
            source: Arc::new(self.source.translated(delta)?),
            radius: self.radius,
        })
    }
}

impl Selection {
    pub fn feathered(&self, amount: u16, width: u32, height: u32) -> Result<Self> {
        validate_canvas_size(width, height)?;
        if !(1..=250).contains(&amount) {
            return Err(invalid(
                "Feather amount must be 1 to 250 pixels. The selection is unchanged.",
            ));
        }
        let (source, radius) = match &self.feather {
            Some(feather) => (
                feather.source.as_ref(),
                feather.radius.hypot(f64::from(amount)).min(250.),
            ),
            None => (self, f64::from(amount)),
        };
        source.with_feather_radius(radius, [0., 0., width as f64, height as f64])
    }

    /// Retain the hard outline so feathering accumulates as Gaussian variance and
    /// geometry operations never trace a blurred mask back into a hard selection.
    pub(super) fn with_feather_radius(&self, radius: f64, extent: [f64; 4]) -> Result<Self> {
        let Some(bounds) = self.bounds() else {
            return Ok(self.clone());
        };
        let margin = (radius * 2.).ceil();
        let bounds = [
            (bounds[0] - margin).floor().max(extent[0]),
            (bounds[1] - margin).floor().max(extent[1]),
            (bounds[2] + margin).ceil().min(extent[2]),
            (bounds[3] + margin).ceil().min(extent[3]),
        ];
        if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
            return Ok(Self::from_mask(GrayImage::new(1, 1)));
        }
        let mut result = Self::rasterize(bounds, |p| self.coverage(p))?;
        let source = result.pixels.dense().ok_or_else(|| {
            invalid("Cannot feather this selection because its mask could not be rasterized.")
        })?;
        let blurred = crate::effects::gaussian_mask(source, (radius / 2.) as f32)?;
        result.pixels = Arc::new(Coverage::raster(blurred));
        result.geometry = self.geometry.as_ref().map(|geometry| {
            Arc::new(geometry.translated([
                self.origin[0] - result.origin[0],
                self.origin[1] - result.origin[1],
            ]))
        });
        result.feather = Some(Feather {
            source: Arc::new(self.clone()),
            radius,
        });
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feather_softens_both_sides_keeps_outline_and_survives_geometry_edits() {
        let hard = Selection::rectangle(100, 100, [30., 30.], [70., 70.], false);
        let soft = hard.feathered(8, 100, 100).unwrap();
        assert!(soft.coverage([29.5, 50.5]) > 0.);
        assert!(soft.coverage([30.5, 50.5]) < 1.);
        assert_eq!(soft.coverage([50.5, 50.5]), 1.);
        let outline = soft.outline_contours().unwrap();
        assert_eq!(
            outline[0][0][0] + soft.origin[0],
            hard.outline_contours().unwrap()[0][0][0]
        );
        let moved = soft.translated([5., 3.]).unwrap();
        assert_eq!(moved.coverage([34.5, 53.5]), soft.coverage([29.5, 50.5]));
        let expanded = soft.resized(2, 100, 100).unwrap();
        assert!(expanded.coverage([29.5, 50.5]) > soft.coverage([29.5, 50.5]));
        assert!(expanded.coverage([29.5, 50.5]) < 1.);
        let inverse = soft.invert(100, 100).unwrap();
        assert!(
            (inverse.coverage([29.5, 50.5]) + soft.coverage([29.5, 50.5]) - 1.).abs() <= 1. / 255.
        );
        let twice = soft.feathered(6, 100, 100).unwrap();
        assert_eq!(twice.pixels, hard.feathered(10, 100, 100).unwrap().pixels);
    }

    #[test]
    fn feather_admits_small_regions_on_sparse_canvases_and_rejects_huge_masks() {
        let small = Selection::rectangle(30_000, 30_000, [100., 100.], [120., 120.], false);
        assert!(small.feathered(10, 30_000, 30_000).is_ok());
        let all = Selection::rectangle(30_000, 30_000, [0., 0.], [30_000., 30_000.], false);
        assert!(all.feathered(10, 30_000, 30_000).is_err());
        assert!(small.feathered(251, 30_000, 30_000).is_err());
    }
}
