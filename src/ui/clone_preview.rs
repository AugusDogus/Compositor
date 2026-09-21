//! Bounded Clone Stamp hover rendering. One worker follows the latest requested source.
use super::*;
use compositor::{
    blend::Blend,
    brush::{Brush, PaintMode, Stroke},
    geometry::{Point, Sampling},
    invalid, render,
};
use image::RgbaImage;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Key {
    session: Uuid,
    revision: u64,
    layer: Option<Uuid>,
    center: Point,
    diameter: f64,
    hardness: f64,
    side: u32,
    all_layers: bool,
}

#[derive(Default)]
enum Work {
    #[default]
    Idle,
    Running(Key),
    Failed(Key),
}

struct Tip {
    diameter: f64,
    hardness: f64,
    pixels: Arc<RgbaImage>,
}

#[derive(Default)]
struct Cache {
    tip: Option<Tip>,
    render: render::DownsampleCache,
}

#[derive(Default)]
pub(super) struct ClonePreview {
    desired: Option<Key>,
    ready: Option<(Key, Image)>,
    work: Work,
    cache: Cache,
}

impl Editor {
    /// Shared by painting and preview, including Swift's whole-pixel source offset.
    pub(super) fn clone_stroke_offset(&self, point: Point) -> Option<Point> {
        let source = self.tools.clone_source?;
        Some(
            self.tools
                .clone_offset
                .filter(|_| self.tools.clone_aligned)
                .unwrap_or([
                    (source[0] - point[0]).round(),
                    (source[1] - point[1]).round(),
                ]),
        )
    }

    fn clone_preview_key(&self, zoom: f64, offset: Point) -> Option<Key> {
        if self.tools.tool != Tool::Clone
            || !self.has_document()
            || self.pending
            || self.modal.is_some()
            || self.space_pan
            || self.keyboard_modifiers.contains(Modifiers::ALT)
            || !matches!(self.gesture, None | Some(Gesture::BrushTip(_)))
        {
            return None;
        }
        let center = match self.gesture {
            Some(Gesture::BrushTip(drag)) => drag.anchor,
            _ => self.canvas_pointer?,
        };
        let point = [
            (center[0] - offset[0]) / zoom,
            (center[1] - offset[1]) / zoom,
        ];
        let delta = self.clone_stroke_offset(point)?;
        let diameter = self.tools.brush.diameter;
        Some(Key {
            session: self.session().id,
            revision: self.revision,
            layer: self.session().document.active,
            center: [point[0] + delta[0], point[1] + delta[1]],
            diameter,
            hardness: self.tools.brush.hardness,
            side: (diameter * zoom * self.session().backing_scale)
                .ceil()
                .clamp(1., 1024.) as u32,
            all_layers: self.tools.clone_sample_all,
        })
    }

    pub(super) fn request_clone_preview(
        &mut self,
        cx: &ViewContext<'_, Self>,
        zoom: f64,
        offset: Point,
    ) {
        let desired = self.clone_preview_key(zoom, offset);
        self.clone_preview.desired = desired;
        let Some(key) = desired else {
            self.clone_preview.ready = None;
            return;
        };
        if self
            .clone_preview
            .ready
            .as_ref()
            .is_some_and(|(ready, _)| *ready == key)
            || matches!(self.clone_preview.work, Work::Running(_))
            || matches!(self.clone_preview.work, Work::Failed(failed) if failed == key)
        {
            return;
        }
        let document = self.preview_document();
        let mut cache = std::mem::take(&mut self.clone_preview.cache);
        self.clone_preview.work = Work::Running(key);
        let launched = cx.spawn_background(
            move || {
                let pixels = calculate(document, key, &mut cache);
                (pixels, cache)
            },
            move |this, result, cx| {
                let result = result
                    .map_err(|error| invalid(format!("Clone preview worker failed: {error}.")))
                    .and_then(|(pixels, cache)| {
                        this.clone_preview.cache = cache;
                        pixels
                    });
                this.receive_clone_preview(key, result);
                cx.invalidate();
            },
        );
        if let Err(error) = launched {
            self.receive_clone_preview(
                key,
                Err(invalid(format!(
                    "Could not start the clone preview: {error}."
                ))),
            );
        }
    }

    fn receive_clone_preview(&mut self, key: Key, pixels: Result<RgbaImage>) {
        if !matches!(self.clone_preview.work, Work::Running(running) if running == key) {
            return;
        }
        self.clone_preview.work = Work::Idle;
        if self.clone_preview.desired != Some(key)
            || self.session().id != key.session
            || self.revision != key.revision
        {
            return;
        }
        let image = pixels.and_then(|pixels| {
            Image::from_rgba(key.side, key.side, pixels.into_raw())
                .map_err(|error| invalid(format!("Could not display the clone preview: {error}.")))
        });
        match image {
            Ok(image) => self.clone_preview.ready = Some((key, image)),
            Err(error) => {
                self.clone_preview.work = Work::Failed(key);
                self.clone_preview.ready = None;
                self.status = format!(
                    "{error} Your project is preserved. Move the pointer or change the brush size to retry."
                );
            }
        }
    }

    pub(super) fn clone_preview_image(&self, zoom: f64, offset: Point) -> Option<Image> {
        let key = self.clone_preview_key(zoom, offset)?;
        self.clone_preview
            .ready
            .as_ref()
            .filter(|(ready, _)| *ready == key)
            .map(|(_, image)| image.clone())
    }
}

fn calculate(mut document: Document, key: Key, cache: &mut Cache) -> Result<RgbaImage> {
    // Swift draws the active raster with its transform, independent of compositing properties.
    if !key.all_layers {
        document
            .layers
            .retain(|layer| Some(layer.id) == key.layer && layer.raster().is_some());
        for layer in &mut document.layers {
            layer.parent = None;
            layer.visible = true;
            layer.opacity = 1.;
            layer.blend = Blend::Normal;
            layer.mask = None;
            layer.clip_source = None;
        }
    }
    let origin = key.center.map(|v| v - key.diameter / 2.);
    let step = key.diameter / f64::from(key.side);
    let mut pixels = render::region_accelerated(
        &document,
        key.side,
        key.side,
        origin,
        [step; 2],
        &mut cache.render,
    )?;
    if cache
        .tip
        .as_ref()
        .is_none_or(|tip| tip.diameter != key.diameter || tip.hardness != key.hardness)
    {
        cache.tip = Some(make_tip(key.diameter, key.hardness)?);
    }
    let tip = cache
        .tip
        .as_ref()
        .ok_or_else(|| invalid("The clone preview brush tip is missing."))?;
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let unit = [
            (f64::from(x) + 0.5) / f64::from(key.side),
            (f64::from(y) + 0.5) / f64::from(key.side),
        ];
        let alpha = render::pixel(&tip.pixels, unit, Sampling::Smooth)[3];
        pixel[3] = (f64::from(pixel[3]) * alpha).round() as u8;
    }
    Ok(pixels)
}

fn make_tip(diameter: f64, hardness: f64) -> Result<Tip> {
    let side = diameter.ceil() as u32;
    let mut document = Document::new(side, side)?;
    let brush = Brush {
        diameter,
        hardness,
        opacity: 1.,
        color: [255; 4],
    };
    let mut stroke = Stroke::start(
        &mut document,
        [f64::from(side) / 2.; 2],
        brush,
        PaintMode::Paint,
        false,
        false,
    )?;
    stroke.finish(&mut document)?;
    // The centered tip stays within this identity-transformed layer. Reuse its pixels
    // directly instead of recompositing up to four million samples for a large brush.
    let pixels = document
        .active_layer()
        .and_then(|layer| layer.raster())
        .cloned()
        .ok_or_else(|| invalid("The clone preview brush tip did not produce pixels."))?;
    Ok(Tip {
        diameter,
        hardness,
        pixels,
    })
}

#[cfg(test)]
#[path = "clone_preview_tests.rs"]
mod tests;
