//! Editable path geometry and style, with a replaceable raster preview.
use crate::{
    Result,
    document::validate_size,
    geometry::Transform,
    invalid,
    vector_path::{BezierPath, Closure, FlattenOptions},
};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

mod layers;
pub use layers::{Gesture, create, layer_path, rasterize, update};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Stroke {
    /// Centered width in source geometry pixels. Caps and joins are round.
    pub width: f64,
    pub color: [u8; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Style {
    /// Open paths close implicitly for filling, but remain open for stroking.
    pub fill: Option<[u8; 4]>,
    pub stroke: Option<Stroke>,
}

impl Style {
    pub fn validate(self) -> Result<()> {
        if self.fill.is_none() && self.stroke.is_none() {
            return Err(invalid(
                "A path shape needs a fill or stroke. Choose a color before creating it.",
            ));
        }
        if self.stroke.is_some_and(|stroke| {
            !stroke.width.is_finite() || !(0.01..=30_000.).contains(&stroke.width)
        }) {
            return Err(invalid(
                "Path stroke width must be between 0.01 and 30,000 pixels.",
            ));
        }
        Ok(())
    }

    fn padding(self) -> f64 {
        1. + self.stroke.map_or(0., |stroke| stroke.width / 2.)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Source {
    pub geometry: BezierPath,
    /// Local view box, independent of the current raster preview resolution.
    pub size: [u32; 2],
    pub style: Style,
}

impl Source {
    pub fn validate(&self) -> Result<()> {
        self.style.validate()?;
        validate_size(self.size[0], self.size[1])?;
        self.geometry.validate()?;
        if self.geometry.anchors.len() < 2 {
            return Err(invalid("A path shape needs at least two anchors."));
        }
        let bounds = self
            .geometry
            .bounds()?
            .ok_or_else(|| invalid("The shape path is empty."))?;
        let padding = self.style.padding();
        if bounds[0] < padding - 1e-7
            || bounds[1] < padding - 1e-7
            || bounds[2] + padding > f64::from(self.size[0]) + 1e-7
            || bounds[3] + padding > f64::from(self.size[1]) + 1e-7
        {
            return Err(invalid(
                "Path geometry and its stroke must fit inside the source view box. Recreate the shape from its original path.",
            ));
        }
        Ok(())
    }

    /// Uses the existing global path-segment budget before allocating pixels.
    pub fn rasterize(&self, size: [u32; 2]) -> Result<RgbaImage> {
        self.validate()?;
        validate_size(size[0], size[1])?;
        let scale = [
            f64::from(size[0]) / f64::from(self.size[0]),
            f64::from(size[1]) / f64::from(self.size[1]),
        ];
        let points = self
            .geometry
            .mapped(|p| [p[0] * scale[0], p[1] * scale[1]])?
            .flatten(FlattenOptions {
                tolerance: 0.1,
                ..Default::default()
            })?;
        let mut builder = tiny_skia::PathBuilder::new();
        for (index, point) in points.into_iter().enumerate() {
            let [x, y] = [(point[0] / scale[0]) as f32, (point[1] / scale[1]) as f32];
            if index == 0 {
                builder.move_to(x, y);
            } else {
                builder.line_to(x, y);
            }
        }
        if self.geometry.closure == Closure::Closed {
            builder.close();
        }
        let path = builder.finish().ok_or_else(|| {
            invalid("The shape could not be rasterized. Its source geometry is unchanged.")
        })?;
        let mut pixmap = tiny_skia::Pixmap::new(size[0], size[1]).ok_or_else(|| {
            invalid("There is not enough memory for the path shape preview. Reduce its dimensions.")
        })?;
        let mut paint = tiny_skia::Paint::default();
        let transform = tiny_skia::Transform::from_scale(scale[0] as f32, scale[1] as f32);
        if let Some([r, g, b, a]) = self.style.fill {
            paint.set_color_rgba8(r, g, b, a);
            pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
        }
        if let Some(stroke) = self.style.stroke {
            let [r, g, b, a] = stroke.color;
            paint.set_color_rgba8(r, g, b, a);
            pixmap.stroke_path(
                &path,
                &paint,
                &tiny_skia::Stroke {
                    width: stroke.width as f32,
                    line_cap: tiny_skia::LineCap::Round,
                    line_join: tiny_skia::LineJoin::Round,
                    ..Default::default()
                },
                transform,
                None,
            );
        }
        let bytes = pixmap
            .pixels()
            .iter()
            .flat_map(|pixel| {
                let pixel = pixel.demultiply();
                [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]
            })
            .collect();
        RgbaImage::from_raw(size[0], size[1], bytes)
            .ok_or_else(|| invalid("The path preview has invalid pixel dimensions."))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Content {
    source: Arc<Source>,
    pixels: Arc<RgbaImage>,
}

impl Content {
    pub(crate) fn from_cached_source(source: Source, pixels: Arc<RgbaImage>) -> Result<Self> {
        source.validate()?;
        validate_size(pixels.width(), pixels.height())?;
        Ok(Self {
            source: Arc::new(source),
            pixels,
        })
    }
    pub fn from_source(source: Source) -> Result<Self> {
        let pixels = Arc::new(source.rasterize(source.size)?);
        Ok(Self {
            source: Arc::new(source),
            pixels,
        })
    }

    pub fn source(&self) -> &Source {
        &self.source
    }
    pub fn pixels(&self) -> &Arc<RgbaImage> {
        &self.pixels
    }

    pub fn with_resolution(&self, size: [u32; 2]) -> Result<Self> {
        Ok(Self {
            source: self.source.clone(),
            pixels: Arc::new(self.source.rasterize(size)?),
        })
    }

    pub(crate) fn resized_preview(&self, transform: Transform) -> Result<Self> {
        validate_transform(transform)?;
        let size = transform.size.map(|v| v.round().max(1.) as u32);
        if self.pixels.dimensions() == (size[0], size[1]) {
            return Ok(self.clone());
        }
        self.with_resolution(size)
    }

    /// Create a local view box without clipping geometry to the canvas.
    pub fn from_document_path(geometry: BezierPath, style: Style) -> Result<(Self, Transform)> {
        let (source, offset) = normalized(geometry, style)?;
        let transform = Transform {
            origin: offset,
            ..Transform::new(source.size[0], source.size[1])
        };
        validate_transform(transform)?;
        Ok((Self::from_source(source)?, transform))
    }

    pub fn document_path(&self, transform: Transform) -> Result<BezierPath> {
        validate_transform(transform)?;
        self.source.geometry.mapped(|p| {
            transform.point([
                p[0] / f64::from(self.source.size[0]),
                p[1] / f64::from(self.source.size[1]),
            ])
        })
    }

    /// Rebound edited geometry while preserving its rotated/flipped document placement.
    /// Stroke width remains in the existing source coordinate system.
    pub fn edited(
        &self,
        geometry: BezierPath,
        style: Style,
        transform: Transform,
    ) -> Result<(Self, Transform)> {
        validate_transform(transform)?;
        geometry.validate()?;
        let previous_size = self.source.size.map(f64::from);
        let local = geometry.mapped(|p| {
            let unit = transform.unit(p);
            [unit[0] * previous_size[0], unit[1] * previous_size[1]]
        })?;
        let (source, offset) = normalized(local, style)?;
        let anchor = transform.point([offset[0] / previous_size[0], offset[1] / previous_size[1]]);
        let mut next = Transform {
            size: [
                transform.size[0] * f64::from(source.size[0]) / previous_size[0],
                transform.size[1] * f64::from(source.size[1]) / previous_size[1],
            ],
            ..transform
        };
        let current = next.point([0., 0.]);
        next.origin[0] += anchor[0] - current[0];
        next.origin[1] += anchor[1] - current[1];
        validate_transform(next)?;
        let size = next.size.map(|v| v.round().max(1.) as u32);
        let pixels = Arc::new(source.rasterize(size)?);
        Ok((
            Self {
                source: Arc::new(source),
                pixels,
            },
            next,
        ))
    }
}

fn validate_transform(transform: Transform) -> Result<()> {
    if !transform.valid() {
        return Err(invalid(
            "The path shape transform exceeds supported bounds. Its geometry is unchanged.",
        ));
    }
    Ok(())
}

fn normalized(geometry: BezierPath, style: Style) -> Result<(Source, [f64; 2])> {
    style.validate()?;
    let [left, top, right, bottom] = geometry
        .bounds()?
        .ok_or_else(|| invalid("A path shape needs at least two anchors."))?;
    let padding = style.padding();
    let offset = [(left - padding).floor(), (top - padding).floor()];
    let size = [
        (right + padding).ceil() - offset[0],
        (bottom + padding).ceil() - offset[1],
    ];
    if size
        .iter()
        .any(|dimension| !(1. ..=30_000.).contains(dimension))
    {
        return Err(invalid(
            "The path shape exceeds 30,000 pixels per side. Reduce its bounds or stroke width.",
        ));
    }
    let size = size.map(|value| value as u32);
    validate_size(size[0], size[1])?;
    let source = Source {
        geometry: geometry.mapped(|p| [p[0] - offset[0], p[1] - offset[1]])?,
        size,
        style,
    };
    source.validate()?;
    Ok((source, offset))
}

#[cfg(test)]
mod layer_tests;
#[cfg(test)]
mod tests;
