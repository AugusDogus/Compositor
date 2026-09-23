use super::ConversionReport;
use crate::{
    Result,
    blend::Blend,
    document::{Document, Layer, LayerContent},
    invalid, render,
};
use ag_psd::psd::{self as photoshop, BlendMode, PixelData};
use uuid::Uuid;

pub fn export_report(doc: &Document) -> ConversionReport {
    let mut report = ConversionReport::default();
    if doc.selection.is_some() {
        report.note("The active selection is not stored in PSD output.");
    }
    if doc.guides.iter().any(|guide| guide.position < 0.) {
        report.note("Guides before the canvas origin are omitted from PSD output.");
    }
    for layer in &doc.layers {
        if let LayerContent::Adjustment(adjustment) = &layer.content
            && super::adjustments::export(adjustment).is_none()
        {
            report.note(format!("{}: adjustment layer is omitted from the editable PSD stack; it remains in the flattened composite.",layer.name));
        }
        if layer.mask.as_ref().is_some_and(|mask| !mask.linked) {
            report.note(format!(
                "{}: mask linking is not stored; the mask will reopen linked.",
                layer.name
            ));
        }
        if layer.text.is_some() {
            report.note(format!("{}: editable text is rasterized.", layer.name));
        }
        if layer.raw.is_some() {
            report.note(format!("{}: RAW development is exported as cached pixels; the camera source and settings remain in the native project.", layer.name));
        }
        if layer
            .effects
            .as_ref()
            .is_some_and(|effects| !effects.is_empty())
        {
            report.note(format!(
                "{}: layer effects and their mask are baked into pixels.",
                layer.name
            ));
        }
        if layer.shape.is_some() {
            report.note(format!("{}: editable shape is rasterized.", layer.name));
        }
        if layer.transform.rotation != 0. || layer.transform.flip_x || layer.transform.flip_y {
            report.note(format!(
                "{}: rotation and flips are baked into pixels.",
                layer.name
            ));
        }
    }
    report
}
pub fn encode(doc: &Document) -> Result<Vec<u8>> {
    doc.validate()?;
    crate::document::validate_size(doc.width, doc.height)?;
    if doc.layers.len() > 10_000 {
        return Err(invalid("PSD export supports at most 10,000 layers."));
    }
    let mut budget = u64::from(doc.width) * u64::from(doc.height);
    let prepared = crate::effects::prepare(doc, false)?;
    let children = export_layers(&prepared, None, &mut budget, 0)?;
    let composite = render::render(doc, doc.width, doc.height)?;
    let psd = photoshop::Psd {
        width: f64::from(doc.width),
        height: f64::from(doc.height),
        bits_per_channel: Some(8.),
        color_mode: Some(photoshop::ColorMode::Rgb),
        children: Some(children),
        image_resources: Some(super::resources::export(doc)),
        image_data: Some(PixelData {
            width: doc.width,
            height: doc.height,
            data: composite.into_raw(),
        }),
        ..Default::default()
    };
    Ok(ag_psd::write_psd(
        &psd,
        &photoshop::WriteOptions {
            no_background: Some(true),
            ..Default::default()
        },
    ))
}
fn charge(width: u32, height: u32, budget: &mut u64) -> Result<()> {
    crate::document::validate_size(width, height)?;
    *budget += u64::from(width) * u64::from(height);
    if *budget > 100_000_000 {
        return Err(invalid(
            "PSD layers and masks exceed the combined 100 million pixel export budget.",
        ));
    }
    Ok(())
}
fn export_layers(
    doc: &Document,
    parent: Option<Uuid>,
    budget: &mut u64,
    depth: usize,
) -> Result<Vec<photoshop::Layer>> {
    if depth > 128 {
        return Err(invalid("PSD folders exceed the 128-level nesting limit."));
    }
    let mut result = Vec::new();
    let mut base = None;
    for layer in doc.layers.iter().filter(|layer| layer.parent == parent) {
        let adjustment = if let LayerContent::Adjustment(adjustment) = &layer.content {
            let Some(adjustment) = super::adjustments::export(adjustment) else {
                continue;
            };
            Some(adjustment)
        } else {
            None
        };
        if layer.clip_source.is_some() && layer.clip_source != base {
            return Err(invalid(format!(
                "{} clips to a non-adjacent base. Reorder or merge this clipping stack before exporting PSD; the project is unchanged.",
                layer.name
            )));
        }
        if layer.clip_source.is_none() {
            base = Some(layer.id);
        }
        let mut output = photoshop::Layer {
            hidden: Some(!layer.visible),
            opacity: Some(layer.opacity),
            blend_mode: Some(to_blend(layer.blend)),
            clipping: Some(layer.clip_source.is_some()),
            ..Default::default()
        };
        output.additional_info.name = Some(layer.name.clone());
        if let Some(adjustment) = adjustment {
            output.additional_info.adjustment = Some(adjustment);
        } else if layer.is_group() {
            output.children = Some(export_layers(doc, Some(layer.id), budget, depth + 1)?);
        } else {
            bake_layer(doc, layer, &mut output, budget)?;
        }
        if let Some(mask) = &layer.mask {
            let t = mask.placement.unwrap_or(layer.transform);
            let bounds = t.bounds();
            let left = bounds[0].floor();
            let top = bounds[1].floor();
            let width = (bounds[2].ceil() - left).max(1.) as u32;
            let height = (bounds[3].ceil() - top).max(1.) as u32;
            charge(width, height, budget)?;
            let mut data = Vec::with_capacity(width as usize * height as usize * 4);
            for y in 0..height {
                for x in 0..width {
                    let unit = t.unit([left + f64::from(x) + 0.5, top + f64::from(y) + 0.5]);
                    let value = if unit.iter().all(|v| (0. ..1.).contains(v)) {
                        (render::mask_pixel(&mask.pixels, unit, t.sampling) * 255.).round() as u8
                    } else if mask.placement.is_some() {
                        (mask.background() * 255.).round() as u8
                    } else {
                        0
                    };
                    data.extend_from_slice(&[value, value, value, 255]);
                }
            }
            output.additional_info.mask = Some(photoshop::LayerMaskData {
                left: Some(left),
                top: Some(top),
                right: Some(left + f64::from(width)),
                bottom: Some(top + f64::from(height)),
                default_color: Some(if mask.placement.is_some() {
                    mask.background() * 255.
                } else {
                    0.
                }),
                disabled: Some(!mask.enabled),
                position_relative_to_layer: Some(false),
                image_data: Some(PixelData {
                    width,
                    height,
                    data,
                }),
                ..Default::default()
            });
        }
        result.push(output);
    }
    Ok(result)
}
fn bake_layer(
    doc: &Document,
    layer: &Layer,
    output: &mut photoshop::Layer,
    budget: &mut u64,
) -> Result<()> {
    let bounds = layer.transform.bounds();
    let left = bounds[0].floor();
    let top = bounds[1].floor();
    let width = (bounds[2].ceil() - left).max(1.) as u32;
    let height = (bounds[3].ceil() - top).max(1.) as u32;
    charge(width, height, budget)?;
    let mut isolated = doc.clone();
    let mut raster = layer.clone();
    raster.parent = None;
    raster.mask = None;
    raster.clip_source = None;
    raster.opacity = 1.;
    raster.blend = Blend::Normal;
    raster.visible = true;
    isolated.layers = vec![raster];
    let pixels = render::region(&isolated, width, height, [left, top], [1., 1.])?;
    output.left = Some(left);
    output.top = Some(top);
    output.right = Some(left + f64::from(width));
    output.bottom = Some(top + f64::from(height));
    output.image_data = Some(PixelData {
        width,
        height,
        data: pixels.into_raw(),
    });
    Ok(())
}
fn to_blend(mode: Blend) -> BlendMode {
    match mode {
        Blend::Normal => BlendMode::Normal,
        Blend::LinearBurn => BlendMode::LinearBurn,
        Blend::LinearDodge => BlendMode::LinearDodge,
        Blend::HardLight => BlendMode::HardLight,
        Blend::VividLight => BlendMode::VividLight,
        Blend::LinearLight => BlendMode::LinearLight,
        Blend::PinLight => BlendMode::PinLight,
        Blend::HardMix => BlendMode::HardMix,
        Blend::Exclusion => BlendMode::Exclusion,
        Blend::Subtract => BlendMode::Subtract,
        Blend::Divide => BlendMode::Divide,
        Blend::Multiply => BlendMode::Multiply,
        Blend::Screen => BlendMode::Screen,
        Blend::Overlay => BlendMode::Overlay,
        Blend::SoftLight => BlendMode::SoftLight,
        Blend::Darken => BlendMode::Darken,
        Blend::Lighten => BlendMode::Lighten,
        Blend::Difference => BlendMode::Difference,
        Blend::ColorDodge => BlendMode::ColorDodge,
        Blend::ColorBurn => BlendMode::ColorBurn,
        Blend::Hue => BlendMode::Hue,
        Blend::Saturation => BlendMode::Saturation,
        Blend::Color => BlendMode::Color,
        Blend::Luminosity => BlendMode::Luminosity,
    }
}
