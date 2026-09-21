use super::LayerEffects;
use crate::{
    Result,
    document::{Document, Layer, LayerContent},
    geometry::Transform,
};
use image::{GrayImage, RgbaImage};
use std::sync::{Arc, Mutex, OnceLock, Weak};

struct Entry {
    source: Weak<RgbaImage>,
    mask: Option<Weak<GrayImage>>,
    mask_enabled: bool,
    mask_placement: Option<Transform>,
    transform: Transform,
    effects: LayerEffects,
    rendered: Arc<RgbaImage>,
}
static CACHE: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
fn cache() -> &'static Mutex<Vec<Entry>> {
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}
fn matches(entry: &Entry, layer: &Layer, source: &Arc<RgbaImage>, effects: &LayerEffects) -> bool {
    entry.source.ptr_eq(&Arc::downgrade(source))
        && entry.effects == *effects
        && entry.transform == layer.transform
        && entry.mask_enabled == layer.mask.as_ref().is_some_and(|m| m.enabled)
        && entry.mask_placement == layer.mask.as_ref().and_then(|m| m.placement)
        && match (&entry.mask, &layer.mask) {
            (None, None) => true,
            (Some(a), Some(b)) => a.ptr_eq(&Arc::downgrade(&b.pixels)),
            _ => false,
        }
}
fn surface(
    layer: &Layer,
    source: &Arc<RgbaImage>,
    effects: &LayerEffects,
    accelerated: bool,
) -> Result<Arc<RgbaImage>> {
    if let Ok(entries) = cache().lock()
        && let Some(entry) = entries.iter().find(|e| matches(e, layer, source, effects))
    {
        return Ok(entry.rendered.clone());
    }
    let inset = effects.margin();
    let mut padded = RgbaImage::new(source.width() + inset * 2, source.height() + inset * 2);
    let backgrounds = layer.mask.as_ref().map_or(0., |m| m.background());
    for (x, y, p) in source.enumerate_pixels() {
        let mut pixel = *p;
        if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled) {
            let unit = [
                (x as f64 + 0.5) / source.width() as f64,
                (y as f64 + 0.5) / source.height() as f64,
            ];
            let t = mask.placement.unwrap_or(layer.transform);
            let unit = if mask.placement.is_some() {
                t.unit(layer.transform.point(unit))
            } else {
                unit
            };
            let alpha = if unit.iter().any(|v| !(0. ..1.).contains(v)) {
                if mask.placement.is_some() {
                    backgrounds
                } else {
                    0.
                }
            } else {
                crate::render::mask_pixel(&mask.pixels, unit, t.sampling)
            };
            pixel[3] = (pixel[3] as f64 * alpha).round() as u8;
        }
        padded.put_pixel(x + inset, y + inset, pixel);
    }
    let gpu = if accelerated {
        crate::render::gpu_effects(&padded, effects)?
    } else {
        None
    };
    let rendered = Arc::new(gpu.unwrap_or_else(|| super::cpu::render(&padded, effects)));
    if rendered.len() <= 64 * 1024 * 1024
        && let Ok(mut entries) = cache().lock()
    {
        entries.retain(|e| e.source.strong_count() > 0);
        while entries.len() >= 8
            || entries.iter().map(|e| e.rendered.len()).sum::<usize>() + rendered.len()
                > 64 * 1024 * 1024
        {
            if entries.is_empty() {
                break;
            }
            entries.remove(0);
        }
        entries.push(Entry {
            source: Arc::downgrade(source),
            mask: layer.mask.as_ref().map(|m| Arc::downgrade(&m.pixels)),
            mask_enabled: layer.mask.as_ref().is_some_and(|m| m.enabled),
            mask_placement: layer.mask.as_ref().and_then(|m| m.placement),
            transform: layer.transform,
            effects: effects.clone(),
            rendered: rendered.clone(),
        });
    }
    Ok(rendered)
}
/// Temporary surfaces include the enabled raster mask, then follow the original
/// transform. Clipping, folder masks, opacity and blend stay in the compositor.
pub(crate) fn prepare(doc: &Document, accelerated: bool) -> Result<Document> {
    let mut prepared = doc.clone();
    for layer in &mut prepared.layers {
        if layer
            .effects
            .as_ref()
            .is_some_and(|effects| !effects.validate())
        {
            return Err(crate::invalid(
                "The layer effects contain invalid settings. Restore valid stroke, shadow, glow, color, and opacity values; source pixels are preserved.",
            ));
        }
        let Some(effects) = layer
            .effects
            .as_ref()
            .map(LayerEffects::visible)
            .filter(|e| !e.is_empty())
        else {
            continue;
        };
        let source = layer.raster().cloned().ok_or_else(|| crate::invalid("Layer effects require a layer with pixels. Folder and adjustment effects cannot be rendered."))?;
        if !effects.validate_size(source.width(), source.height()) {
            return Err(crate::invalid(
                "The layer effects need more than 100 million pixels. Reduce the stroke, glow size, shadow distance, blur, or layer size; source pixels are preserved.",
            ));
        }
        let rendered = surface(layer, &source, &effects, accelerated)?;
        let center = layer.transform.point([0.5, 0.5]);
        layer.transform.size[0] *= rendered.width() as f64 / source.width() as f64;
        layer.transform.size[1] *= rendered.height() as f64 / source.height() as f64;
        layer.transform.origin = [
            center[0] - layer.transform.size[0] / 2.,
            center[1] - layer.transform.size[1] / 2.,
        ];
        layer.content = LayerContent::Raster(Some(rendered));
        layer.mask = None;
        layer.effects = None;
    }
    Ok(prepared)
}
