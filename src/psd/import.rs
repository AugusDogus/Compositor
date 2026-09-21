use super::{ConversionReport, Imported, preflight};
use crate::{
    Result,
    blend::Blend,
    document::{Document, Layer, LayerContent, Mask},
    geometry::Transform,
    invalid,
};
use ag_psd::psd::{self as photoshop, BlendMode};
use image::{GrayImage, Luma, RgbaImage};
use std::sync::Arc;
use uuid::Uuid;

pub fn decode(bytes: &[u8]) -> Result<Imported> {
    let preflight::Prepared {
        mut report,
        metadata,
        bytes,
    } = preflight::validate(bytes)?;
    let mut psd = ag_psd::read_psd(
        &bytes,
        &photoshop::ReadOptions {
            use_image_data: Some(true),
            skip_thumbnail: Some(true),
            skip_linked_files_data: Some(true),
            total_memory_limit: Some(800_000_000),
            strict: Some(true),
            ..Default::default()
        },
    )
    .map_err(|e| {
        invalid(format!(
            "PSD could not be read: {e}. The current document is unchanged."
        ))
    })?;
    super::vector_metadata::apply(&mut psd, metadata)?;
    let mut budget = decoded_budget(&psd);
    if budget > 100_000_000 {
        return Err(invalid(
            "PSD layers and padded masks exceed the combined 100 million pixel import budget.",
        ));
    }
    let mut document = Document::new(psd.width as u32, psd.height as u32)?;
    super::resources::import(&mut document, psd.image_resources, &mut report);
    document.layers.clear();
    document.active = None;
    document.selected.clear();
    if let Some(children) = psd.children.filter(|children| !children.is_empty()) {
        layers(children, None, &mut document, &mut report, 0, &mut budget)?;
    } else if let Some(pixels) = psd.image_data.or(psd.canvas) {
        let mut layer = Layer::blank("Background", pixels.width, pixels.height);
        layer.content = LayerContent::Raster(Some(Arc::new(rgba(pixels)?)));
        document.add(layer)?;
    }
    document.validate()?;
    Ok(Imported { document, report })
}
fn rgba(pixels: photoshop::PixelData) -> Result<RgbaImage> {
    RgbaImage::from_raw(pixels.width, pixels.height, pixels.data)
        .ok_or_else(|| invalid("PSD pixel data does not match its layer dimensions."))
}
fn layers(
    children: Vec<photoshop::Layer>,
    parent: Option<Uuid>,
    doc: &mut Document,
    report: &mut ConversionReport,
    depth: usize,
    budget: &mut u64,
) -> Result<()> {
    if depth > 128 {
        return Err(invalid("PSD folders exceed the 128-level nesting limit."));
    }
    let mut base = None;
    for source in children {
        let info = &source.additional_info;
        let name = info
            .name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Layer".into());
        let adjustment = if let Some(adjustment) = &info.adjustment {
            match super::adjustments::import(adjustment)? {
                Some(adjustment) => {
                    report.note(format!("{name}: editable adjustment parameters are preserved; rendering may differ from Photoshop."));
                    Some(adjustment)
                }
                None => {
                    if !source.clipping.unwrap_or(false) {
                        base = None;
                    }
                    report.note(format!("{name}: unsupported Photoshop adjustment is omitted from the editable stack."));
                    continue;
                }
            }
        } else {
            None
        };
        let vector = if adjustment.is_none() && source.children.is_none() {
            super::vector::import(info, budget, report, &name)?
        } else {
            None
        };
        if info.text.is_some()
            || info.placed_layer.is_some()
            || (vector.is_none() && (info.vector_mask.is_some() || info.vector_fill.is_some()))
        {
            report.note(format!("{name}: text, unsupported vector, or smart-object content is converted to saved pixels."));
        }
        if info.effects.is_some() {
            report.note(format!("{name}: Photoshop layer effects are omitted."));
        }
        let mut layer = Layer::blank(name, 1, 1);
        layer.parent = parent;
        layer.visible = !source.hidden.unwrap_or(false);
        layer.opacity = source.opacity.unwrap_or(1.) * info.fill_opacity.unwrap_or(1.);
        layer.blend = from_blend(source.blend_mode.unwrap_or(BlendMode::Normal), report);
        let group = source.children.is_some();
        if group && layer.blend != Blend::Normal {
            report.note(format!(
                "{}: folder blend is converted to Normal.",
                layer.name
            ));
            layer.blend = Blend::Normal;
        }
        if info.real_mask.is_some() {
            report.note(format!(
                "{}: additional Photoshop mask channels are omitted.",
                layer.name
            ));
        }
        if group {
            layer.content = LayerContent::Group;
            layer.transform = Transform::new(doc.width, doc.height);
        } else if let Some(adjustment) = adjustment {
            layer.content = LayerContent::Adjustment(Box::new(adjustment));
            layer.transform = Transform::new(doc.width, doc.height);
        } else if let Some(vector) = vector {
            layer.shape = vector.shape;
            layer.transform = vector.transform;
            layer.content = LayerContent::Raster(Some(Arc::new(vector.pixels)));
        } else if let Some(pixels) = source.image_data.or(source.canvas) {
            layer.transform = Transform::new(pixels.width, pixels.height);
            layer.transform.origin = [source.left.unwrap_or(0.), source.top.unwrap_or(0.)];
            layer.content = LayerContent::Raster(Some(Arc::new(rgba(pixels)?)));
        } else if info.vector_fill.is_some() || info.text.is_some() || info.placed_layer.is_some() {
            report.note(format!(
                "{}: no saved pixels are available; this layer is empty.",
                layer.name
            ));
        }
        if let Some(mask) = &info.mask {
            layer.mask = import_mask(mask, report, &layer.name, layer.transform.origin)?;
        }
        if source.clipping.unwrap_or(false) {
            layer.clip_source = base;
            if base.is_none() {
                report.note(format!(
                    "{}: clipping has no base layer and is removed.",
                    layer.name
                ));
            }
        } else {
            base = matches!(layer.content, LayerContent::Raster(_)).then_some(layer.id);
        }
        let id = layer.id;
        doc.add(layer)?;
        if let Some(children) = source.children {
            layers(children, Some(id), doc, report, depth + 1, budget)?;
        }
    }
    Ok(())
}
fn import_mask(
    mask: &photoshop::LayerMaskData,
    report: &mut ConversionReport,
    name: &str,
    origin: [f64; 2],
) -> Result<Option<Mask>> {
    let Some(data) = mask.image_data.as_ref().or(mask.canvas.as_ref()) else {
        return Ok(None);
    };
    // A one-pixel default-color border makes the existing outside-mask sampler
    // reproduce Photoshop's explicit default tone instead of guessing from art.
    let width = data
        .width
        .checked_add(2)
        .ok_or_else(|| invalid("PSD mask width overflow."))?;
    let height = data
        .height
        .checked_add(2)
        .ok_or_else(|| invalid("PSD mask height overflow."))?;
    crate::document::validate_size(width, height)?;
    let mut pixels = GrayImage::from_pixel(
        width,
        height,
        Luma([mask.default_color.unwrap_or(0.) as u8]),
    );
    let channels = if data.data.len() == data.width as usize * data.height as usize {
        1
    } else {
        4
    };
    if data.data.len() != data.width as usize * data.height as usize * channels {
        return Err(invalid("PSD mask data length is invalid."));
    }
    for y in 0..data.height {
        for x in 0..data.width {
            pixels[(x + 1, y + 1)] =
                Luma([data.data[(y as usize * data.width as usize + x as usize) * channels]]);
        }
    }
    let mut placement = Transform::new(width, height);
    placement.origin = [mask.left.unwrap_or(0.) - 1., mask.top.unwrap_or(0.) - 1.];
    if mask.position_relative_to_layer.unwrap_or(false) {
        placement.origin[0] += origin[0];
        placement.origin[1] += origin[1];
    }
    if mask.user_mask_feather.unwrap_or(0.) != 0. || mask.user_mask_density.unwrap_or(1.) != 1. {
        report.note(format!(
            "{name}: Photoshop mask feather/density settings are omitted."
        ));
    }
    Ok(Some(Mask {
        pixels: Arc::new(pixels),
        enabled: !mask.disabled.unwrap_or(false),
        linked: true,
        placement: Some(placement),
    }))
}
fn from_blend(mode: BlendMode, report: &mut ConversionReport) -> Blend {
    match mode {
        BlendMode::Normal => Blend::Normal,
        BlendMode::Multiply => Blend::Multiply,
        BlendMode::Screen => Blend::Screen,
        BlendMode::Overlay => Blend::Overlay,
        BlendMode::SoftLight => Blend::SoftLight,
        BlendMode::Darken => Blend::Darken,
        BlendMode::Lighten => Blend::Lighten,
        BlendMode::Difference => Blend::Difference,
        BlendMode::ColorDodge => Blend::ColorDodge,
        BlendMode::ColorBurn => Blend::ColorBurn,
        BlendMode::Hue => Blend::Hue,
        BlendMode::Saturation => Blend::Saturation,
        BlendMode::Color => Blend::Color,
        BlendMode::Luminosity => Blend::Luminosity,
        _ => {
            report.note(format!("Photoshop {mode:?} blend is converted to Normal."));
            Blend::Normal
        }
    }
}

// Include decoded storage before charging new vector surfaces. This deliberately
// counts cached vector pixels too, since they remain allocated during conversion.
fn decoded_budget(psd: &photoshop::Psd) -> u64 {
    fn children(layers: &[photoshop::Layer]) -> u64 {
        layers
            .iter()
            .map(|l| {
                let raster = l
                    .image_data
                    .as_ref()
                    .or(l.canvas.as_ref())
                    .map_or(0, |p| u64::from(p.width) * u64::from(p.height));
                let mask = l
                    .additional_info
                    .mask
                    .as_ref()
                    .and_then(|m| m.image_data.as_ref().or(m.canvas.as_ref()))
                    .map_or(0, |p| (u64::from(p.width) + 2) * (u64::from(p.height) + 2));
                raster + mask + children(l.children.as_deref().unwrap_or_default())
            })
            .sum()
    }
    psd.width as u64 * psd.height as u64 + children(psd.children.as_deref().unwrap_or_default())
}
