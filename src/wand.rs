use crate::{Result, document::Document, geometry::Point, invalid, render, selection::Selection};
use image::{Rgba, RgbaImage};

#[derive(Clone, Copy)]
pub struct Settings {
    pub tolerance: u8,
    pub contiguous: bool,
    pub radius: usize,
    pub sample_all: bool,
}

struct Axis {
    start: u32,
    end: u32,
    canvas: u32,
    before: u32,
    after: u32,
}

impl Axis {
    fn new(start: u32, end: u32, canvas: u32, radius: u32) -> Self {
        // Two sampling radii preserve the complete average near real pixels.
        // More distant empty space has the same color and connectivity.
        let border = 2 * radius + 1;
        Self {
            start,
            end,
            canvas,
            before: start.min(border),
            after: (canvas - end).min(border),
        }
    }
    fn size(&self) -> u32 {
        self.before + self.end - self.start + self.after
    }
    fn seed(&self, coordinate: f64) -> u32 {
        (coordinate.floor() - self.start as f64 + self.before as f64)
            .clamp(0., (self.size() - 1) as f64) as u32
    }
    fn forward(&self, coordinate: f64) -> f64 {
        let before = self.before as f64;
        let end = (self.before + self.end - self.start) as f64;
        if coordinate < before {
            coordinate * self.start as f64 / before
        } else if coordinate > end {
            self.end as f64
                + (coordinate - end) * (self.canvas - self.end) as f64 / self.after as f64
        } else {
            self.start as f64 + coordinate - before
        }
    }
    fn inverse(&self, coordinate: f64) -> f64 {
        if coordinate < self.start as f64 {
            coordinate * self.before as f64 / self.start as f64
        } else if coordinate > self.end as f64 {
            (self.before + self.end - self.start) as f64
                + (coordinate - self.end as f64) * self.after as f64
                    / (self.canvas - self.end) as f64
        } else {
            coordinate - self.start as f64 + self.before as f64
        }
    }
}

/// Preserve empty-margin connectivity while running the original matching kernel
/// only over the bounded image content. Expand the traced outline back afterward.
pub fn select(doc: &Document, point: Point, settings: Settings) -> Result<Selection> {
    if settings.radius > 2 || point.iter().any(|v| !v.is_finite()) {
        return Err(invalid(
            "Wand coordinates must be finite, with point, 3 by 3, or 5 by 5 sampling.",
        ));
    }
    crate::document::validate_canvas_size(doc.width, doc.height)?;
    if point[0] < 0.
        || point[1] < 0.
        || point[0] >= doc.width as f64
        || point[1] >= doc.height as f64
    {
        return Selection::marquee(doc.width, doc.height, [0.; 2], [0.; 2], false, true);
    }
    let active = doc.active_layer();
    if !settings.sample_all && active.is_none() {
        return Err(invalid(
            "Select a layer, or enable All layers for the wand.",
        ));
    }
    let mut bounds = [doc.width as f64, doc.height as f64, 0., 0.];
    for layer in doc.layers.iter().filter(|layer| {
        layer.raster().is_some() && (settings.sample_all || Some(layer.id) == doc.active)
    }) {
        let b = layer.transform.bounds();
        bounds = [
            bounds[0].min(b[0]),
            bounds[1].min(b[1]),
            bounds[2].max(b[2]),
            bounds[3].max(b[3]),
        ];
    }
    bounds = [
        bounds[0].floor().max(0.),
        bounds[1].floor().max(0.),
        bounds[2].ceil().min(doc.width as f64),
        bounds[3].ceil().min(doc.height as f64),
    ];
    if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
        return Selection::marquee(
            doc.width,
            doc.height,
            [0.; 2],
            [doc.width as f64, doc.height as f64],
            false,
            true,
        );
    }
    let axes = [
        Axis::new(
            bounds[0] as u32,
            bounds[2] as u32,
            doc.width,
            settings.radius as u32,
        ),
        Axis::new(
            bounds[1] as u32,
            bounds[3] as u32,
            doc.height,
            settings.radius as u32,
        ),
    ];
    let size = [axes[0].size(), axes[1].size()];
    crate::document::validate_size(size[0], size[1])?;
    let sampler = settings.sample_all.then(|| render::Sampler::new(doc));
    let image = RgbaImage::from_fn(size[0], size[1], |x, y| {
        let point = [
            x as f64 + (axes[0].start - axes[0].before) as f64 + 0.5,
            y as f64 + (axes[1].start - axes[1].before) as f64 + 0.5,
        ];
        let color = match &sampler {
            Some(sampler) => sampler.sample(point),
            None => active
                .and_then(|layer| {
                    layer.raster().map(|pixels| {
                        render::pixel(
                            pixels,
                            layer.transform.unit(point),
                            layer.transform.sampling,
                        )
                    })
                })
                .unwrap_or([0.; 4]),
        };
        Rgba(color.map(|v| (v * 255.).round() as u8))
    });
    let selection = Selection::wand(
        &image,
        axes[0].seed(point[0]),
        axes[1].seed(point[1]),
        settings.tolerance,
        settings.contiguous,
        settings.radius,
    )?;
    selection.mapped(
        [0., 0., doc.width as f64, doc.height as f64],
        |p| [axes[0].forward(p[0]), axes[1].forward(p[1])],
        |p| [axes[0].inverse(p[0]), axes[1].inverse(p[1])],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document::LayerContent, geometry::Transform};
    use std::sync::Arc;

    #[test]
    fn compressed_empty_margins_preserve_wand_matching_averages_and_connectivity() {
        for (origin, size) in [
            ([10., 15.], [12, 14]),
            ([0., 15.], [60, 5]),
            ([0., 0.], [12, 14]),
        ] {
            let mut doc = Document::new(60, 50).unwrap();
            let layer = &mut doc.layers[0];
            layer.transform = Transform {
                origin,
                ..Transform::new(size[0], size[1])
            };
            layer.opacity = 0.6;
            layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(
                size[0],
                size[1],
                |x, y| {
                    Rgba([
                        120,
                        (x * 3) as u8,
                        200,
                        if (x + y).is_multiple_of(4) { 0 } else { 255 },
                    ])
                },
            ))));
            for sample_all in [false, true] {
                let full = if sample_all {
                    render::render(&doc, 60, 50)
                } else {
                    let layer = &doc.layers[0];
                    RgbaImage::from_fn(60, 50, |x, y| {
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
                for radius in 0..=2 {
                    for contiguous in [false, true] {
                        for point in [
                            [0., 0.],
                            [8., 15.],
                            [9., 14.],
                            [10., 15.],
                            [12., 18.],
                            [21., 26.],
                            [59., 49.],
                        ] {
                            for tolerance in [0, 34, 255] {
                                let actual = select(
                                    &doc,
                                    point,
                                    Settings {
                                        tolerance,
                                        contiguous,
                                        radius,
                                        sample_all,
                                    },
                                )
                                .unwrap();
                                let expected = Selection::wand(
                                    &full,
                                    point[0] as u32,
                                    point[1] as u32,
                                    tolerance,
                                    contiguous,
                                    radius,
                                )
                                .unwrap();
                                for y in 0..50 {
                                    for x in 0..60 {
                                        let p = [x as f64 + 0.5, y as f64 + 0.5];
                                        assert_eq!(
                                            actual.coverage(p),
                                            expected.coverage(p),
                                            "{point:?}, radius {radius}, tolerance {tolerance}, contiguous {contiguous}, all {sample_all}, pixel {p:?}"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn wand_on_large_sparse_canvas_selects_small_content_or_connected_empty_surroundings() {
        let mut doc = Document::new(30_000, 30_000).unwrap();
        let settings = Settings {
            tolerance: 0,
            contiguous: true,
            radius: 0,
            sample_all: false,
        };
        assert_eq!(
            select(&doc, [20_000., 20_000.], settings).unwrap().bounds(),
            Some([0., 0., 30_000., 30_000.])
        );
        doc.layers[0].transform = Transform {
            origin: [15_000., 15_000.],
            ..Transform::new(10, 10)
        };
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            10,
            10,
            Rgba([12, 34, 56, 255]),
        ))));
        let content = select(&doc, [15_005., 15_005.], settings).unwrap();
        assert_eq!(content.bounds(), Some([15_000., 15_000., 15_010., 15_010.]));
        let outside = select(&doc, [100., 100.], settings).unwrap();
        assert!(outside.pixels.dense().is_none());
        assert_eq!(outside.coverage([15_005., 15_005.]), 0.);
        assert_eq!(outside.coverage([29_999., 29_999.]), 1.);
        assert_eq!(
            outside.invert(30_000, 30_000).unwrap().bounds(),
            content.bounds()
        );
    }
}
