use crate::{
    Result,
    blend::Blend,
    clipboard,
    document::{Document, LayerContent},
    edits,
    geometry::{Point, Sampling, Transform},
    invalid, raster_extent, render,
};
use image::Rgba;
use std::sync::Arc;

/// A selected pixel snapshot. Every preview starts from the same source document.
pub struct FloatingPixels {
    original: Document,
    pixels: clipboard::PixelClipboard,
    pub placement: Transform,
}

impl FloatingPixels {
    pub fn lift(doc: &Document) -> Result<Self> {
        if doc.selection.is_none() {
            return Err(invalid("Select pixels before moving or transforming them."));
        }
        let pixels = clipboard::copy(doc, false, false)?;
        if pixels.pixels.pixels().all(|p| p[3] == 0) {
            return Err(invalid("The selection contains no visible image pixels."));
        }
        let placement = Transform {
            origin: pixels.origin,
            ..Transform::new(pixels.pixels.width(), pixels.pixels.height())
        };
        Ok(Self {
            original: doc.clone(),
            pixels,
            placement,
        })
    }

    pub fn preview(&self, transform: Transform, duplicate: bool) -> Result<Document> {
        if !transform.valid() {
            return Err(invalid(
                "The selected-pixel transform exceeds supported bounds.",
            ));
        }
        if transform == self.placement && !duplicate {
            return Ok(self.original.clone());
        }
        self.render(
            transform.bounds(),
            crate::distort::Mapping::Affine(transform),
            transform.sampling,
            duplicate,
        )
    }

    pub fn preview_distorted(&self, corners: [Point; 4], duplicate: bool) -> Result<Document> {
        let mapping = crate::distort::Mapping::new(corners)?;
        let bounds = [
            corners.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
            corners.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
            corners
                .iter()
                .map(|p| p[0])
                .fold(f64::NEG_INFINITY, f64::max),
            corners
                .iter()
                .map(|p| p[1])
                .fold(f64::NEG_INFINITY, f64::max),
        ];
        self.render(bounds, mapping, self.placement.sampling, duplicate)
    }

    fn render(
        &self,
        bounds: [f64; 4],
        mapping: crate::distort::Mapping,
        sampling: Sampling,
        duplicate: bool,
    ) -> Result<Document> {
        let mut doc = self.original.clone();
        if !duplicate {
            edits::fill(&mut doc, [0; 4], true, false)?;
        }
        let layer = doc
            .active_layer_mut()
            .ok_or_else(|| invalid("The source layer is missing."))?;
        raster_extent::expand(layer, bounds)?;
        let placement = layer.transform;
        let LayerContent::Raster(Some(pixels)) = &mut layer.content else {
            return Err(invalid("The source layer no longer contains pixels."));
        };
        let (w, h) = pixels.dimensions();
        for (x, y, p) in Arc::make_mut(pixels).enumerate_pixels_mut() {
            let point = placement.point([(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64]);
            let top = mapping.inverse_points(point).into_iter().flatten().fold(
                [0.; 4],
                |bottom, unit| {
                    Blend::Normal
                        .composite(bottom, render::pixel(&self.pixels.pixels, unit, sampling))
                },
            );
            if top[3] > 0. {
                *p = Rgba(
                    Blend::Normal
                        .composite(p.0.map(|v| v as f64 / 255.), top)
                        .map(|v| (v * 255.).round() as u8),
                );
            }
        }
        layer.shape = None;
        layer.text = None;
        doc.selection = self
            .original
            .selection
            .as_ref()
            .map(|s| {
                if matches!(mapping, crate::distort::Mapping::Folded(_)) {
                    crate::selection::Selection::rasterize(bounds, |point| {
                        mapping
                            .inverse_points(point)
                            .into_iter()
                            .flatten()
                            .map(|unit| s.coverage(self.placement.point(unit)))
                            .fold(0., f64::max)
                    })
                } else {
                    s.mapped(
                        bounds,
                        |point| mapping.map(self.placement.unit(point)),
                        |point| {
                            self.placement
                                .point(mapping.inverse_points(point)[0].unwrap_or([f64::NAN; 2]))
                        },
                    )
                }
            })
            .transpose()?;
        Ok(doc)
    }
}

pub fn move_pixels(doc: &mut Document, delta: [f64; 2], duplicate: bool) -> Result<()> {
    let pixels = FloatingPixels::lift(doc)?;
    let mut transform = pixels.placement;
    transform.origin[0] += delta[0].round();
    transform.origin[1] += delta[1].round();
    *doc = pixels.preview(transform, duplicate)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::Selection;
    #[test]
    fn transformed_selections_rasterize_the_transformed_outline_without_stair_steps() {
        let mut doc = Document::new(80, 80).unwrap();
        edits::fill(&mut doc, [40, 80, 120, 255], false, false).unwrap();
        doc.selection =
            Some(Selection::polygon(80, 80, &[[10., 10.], [20., 10.], [10., 20.]], true).unwrap());
        let source = FloatingPixels::lift(&doc).unwrap();
        let transform = Transform {
            origin: [25.3, 21.7],
            size: [30., 22.],
            rotation: 30.,
            flip_x: true,
            ..source.placement
        };
        let moved = source.preview(transform, false).unwrap();
        let points = [[0., 0.], [1., 0.], [0., 1.]].map(|p| transform.point(p));
        let expected = Selection::polygon(80, 80, &points, true).unwrap();
        let actual = moved.selection.as_ref().unwrap();
        for y in 0..80 {
            for x in 0..80 {
                let p = [x as f64 + 0.5, y as f64 + 0.5];
                assert!(
                    (actual.coverage(p) - expected.coverage(p)).abs() <= 1. / 255.,
                    "pixel {x}, {y}"
                );
            }
        }
        let corners = [[25.3, 21.7], [57.4, 19.8], [52.1, 55.3], [28.5, 48.2]];
        let distorted = source.preview_distorted(corners, false).unwrap();
        let expected =
            Selection::polygon(80, 80, &[corners[0], corners[1], corners[3]], true).unwrap();
        for y in 0..80 {
            for x in 0..80 {
                let p = [x as f64 + 0.5, y as f64 + 0.5];
                assert!(
                    (distorted.selection.as_ref().unwrap().coverage(p) - expected.coverage(p))
                        .abs()
                        <= 1. / 255.
                );
            }
        }
        assert_eq!(source.preview(source.placement, false).unwrap(), doc);
    }
    #[test]
    fn distorted_pixels_follow_the_quad_without_touching_unselected_pixels() {
        let mut doc = Document::new(8, 8).unwrap();
        edits::fill(&mut doc, [10, 20, 30, 255], false, false).unwrap();
        doc.selection = Some(Selection::rectangle(8, 8, [0., 0.], [2., 2.], false));
        edits::fill(&mut doc, [200, 50, 100, 255], false, false).unwrap();
        let source = FloatingPixels::lift(&doc).unwrap();
        let moved = source
            .preview_distorted([[3., 3.], [7., 3.], [6., 7.], [4., 7.]], false)
            .unwrap();
        let pixels = moved.layers[0].raster().unwrap();
        assert_eq!(pixels[(0, 0)][3], 0);
        assert_eq!(pixels[(2, 2)], Rgba([10, 20, 30, 255]));
        assert_eq!(pixels[(4, 4)], Rgba([200, 50, 100, 255]));
        assert_eq!(moved.selection.as_ref().unwrap().coverage([4.5, 4.5]), 1.);
        assert_eq!(moved.selection.as_ref().unwrap().coverage([3.5, 6.5]), 0.);
        assert!(
            source
                .preview_distorted([[0., 0.], [2., 2.], [2., 0.], [0., 2.]], false)
                .is_ok()
        );
        assert!(
            source
                .preview_distorted([[0., 0.], [1., 0.], [2., 0.], [0., 2.]], false)
                .is_err()
        );
    }
    #[test]
    fn selected_pixels_can_move_outside_the_canvas_and_back() {
        let mut doc = Document::new(4, 4).unwrap();
        doc.selection = Some(Selection::rectangle(4, 4, [1., 1.], [3., 3.], false));
        edits::fill(&mut doc, [60, 120, 180, 255], false, false).unwrap();
        let before = render::render(&doc, 4, 4).unwrap();
        move_pixels(&mut doc, [-8., 6.], false).unwrap();
        assert_eq!(
            doc.selection.as_ref().unwrap().bounds(),
            Some([-7., 7., -5., 9.])
        );
        move_pixels(&mut doc, [8., -6.], false).unwrap();
        for (actual, expected) in render::render(&doc, 4, 4)
            .unwrap()
            .pixels()
            .zip(before.pixels())
        {
            assert_eq!(actual[3], expected[3]);
            if expected[3] > 0 {
                assert_eq!(actual, expected);
            }
        }
    }
    #[test]
    fn moving_selection_cuts_or_duplicates_without_changing_layer_appearance() {
        for duplicate in [false, true] {
            let mut doc = Document::new(8, 4).unwrap();
            doc.selection = Some(Selection::rectangle(8, 4, [1., 1.], [3., 3.], false));
            edits::fill(&mut doc, [255, 0, 0, 128], false, false).unwrap();
            doc.layers[0].opacity = 0.5;
            move_pixels(&mut doc, [4., 0.], duplicate).unwrap();
            let p = doc.layers[0].raster().unwrap();
            assert_eq!(p[(1, 1)][3], if duplicate { 128 } else { 0 });
            assert_eq!(p[(5, 1)], Rgba([255, 0, 0, 128]));
            assert_eq!(doc.layers[0].opacity, 0.5);
            assert_eq!(doc.selection.as_ref().unwrap().coverage([5.5, 1.5]), 1.);
            assert_eq!(doc.selection.as_ref().unwrap().coverage([1.5, 1.5]), 0.);
        }
    }
    #[test]
    fn returning_a_floating_selection_to_its_origin_restores_exact_pixels() {
        let mut doc = Document::new(4, 4).unwrap();
        edits::fill(&mut doc, [40, 80, 120, 128], false, false).unwrap();
        doc.selection = Some(Selection::rectangle(4, 4, [0., 0.], [2., 2.], false));
        let float = FloatingPixels::lift(&doc).unwrap();
        assert_eq!(float.preview(float.placement, false).unwrap(), doc);
    }
}
