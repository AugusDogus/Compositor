use super::ConversionReport;
use crate::{
    Result,
    blend::Blend,
    document::{Document, Layer, LayerContent},
    invalid, render,
};
use ag_psd::psd::{self as photoshop, BlendMode, PixelData};
use std::collections::HashMap;
use uuid::Uuid;

pub fn export_report(doc: &Document) -> ConversionReport {
    let mut report = ConversionReport::default();
    if !doc.paths.is_empty() {
        report.note("Saved working paths are omitted from PSD output. Save a .comp project to retain editable paths.");
    }
    if requires_rendered_copy(doc) {
        report.note("Artboards, Photo Filter or fractional Channel Mixer/Selective Color adjustment layers require a rendered PSD copy. All layers are flattened in this export; save a .comp project to retain editable layers and filter settings.");
        return report;
    }
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
        if matches!(&layer.content, LayerContent::ExtendedAdjustment(a) if matches!(**a, crate::adjustment::ExtendedAdjustment::ChannelMixer(s) if s.monochrome))
        {
            report.note(format!("{}: PSD stores the active monochrome mix; inactive RGB mixes remain only in the .comp project.", layer.name));
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
            match super::effects::encode(doc, layer) {
                Ok(_) => report.note(format!("{}: supported layer effects remain editable. Photoshop's stroke and blur rendering may differ; the flattened composite retains the rendered appearance.", layer.name)),
                Err(reason) => report.note(format!("{}: layer effects and their mask are baked into pixels because {reason}.", layer.name)),
            }
        }
        if layer.shape.is_some() || layer.is_path_shape() {
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
    if requires_rendered_copy(doc) {
        let mut flattened = Document::new(doc.width, doc.height)?;
        flattened.guides = doc.guides.clone();
        flattened.resolution = doc.resolution;
        flattened.layers[0].name = "Rendered composite".into();
        flattened.layers[0].content = LayerContent::Raster(Some(std::sync::Arc::new(
            render::render(doc, doc.width, doc.height)?,
        )));
        return encode(&flattened);
    }
    let mut budget = u64::from(doc.width) * u64::from(doc.height);
    let mut source = doc.clone();
    let mut effects = HashMap::new();
    for layer in &mut source.layers {
        if let Ok(styles) = super::effects::encode(doc, layer) {
            effects.insert(layer.id, styles);
            layer.effects = None;
        }
    }
    let prepared = crate::effects::prepare(&source, false)?;
    let children = export_layers(&prepared, None, &effects, &mut budget, 0)?;
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
    crate::document::validate_pixel_budget(*budget)?;
    Ok(())
}
fn export_layers(
    doc: &Document,
    parent: Option<Uuid>,
    effects: &HashMap<Uuid, photoshop::LayerEffectsInfo>,
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
        } else if let LayerContent::ExtendedAdjustment(adjustment) = &layer.content {
            Some(
                super::extended::export(adjustment)
                    .ok_or_else(|| invalid("This Linux adjustment needs a rendered PSD copy."))?,
            )
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
        output.additional_info.effects = effects.get(&layer.id).cloned();
        if let Some(adjustment) = adjustment {
            output.additional_info.adjustment = Some(adjustment);
        } else if layer.is_group() {
            output.children = Some(export_layers(
                doc,
                Some(layer.id),
                effects,
                budget,
                depth + 1,
            )?);
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

fn requires_rendered_copy(doc: &Document) -> bool {
    doc.layers.iter().any(|layer| {
        matches!(layer.content, LayerContent::Artboard(_))
            || matches!(&layer.content, LayerContent::ExtendedAdjustment(adjustment) if super::extended::export(adjustment).is_none())
    })
}

#[cfg(test)]
mod extended_tests {
    use super::*;
    use crate::adjustment::{ExtendedAdjustment, PhotoFilter};
    #[test]
    fn extended_adjustment_psd_preserves_visible_composite_with_explicit_notice() {
        let mut doc = Document::new(2, 2).unwrap();
        doc.layers[0].content = LayerContent::Raster(Some(std::sync::Arc::new(
            image::RgbaImage::from_pixel(2, 2, image::Rgba([180, 120, 80, 128])),
        )));
        let mut layer = Layer::blank("Warm", 2, 2);
        layer.content = LayerContent::ExtendedAdjustment(Box::new(
            ExtendedAdjustment::PhotoFilter(PhotoFilter::default()),
        ));
        doc.add(layer).unwrap();
        doc.resolution = 300.;
        let original = doc.clone();
        assert!(format!("{:?}", export_report(&doc)).contains("All layers are flattened"));
        let bytes = encode(&doc).unwrap();
        let psd = ag_psd::read_psd(
            &bytes,
            &photoshop::ReadOptions {
                use_image_data: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        let composite = render::render(&doc, 2, 2).unwrap();
        assert_eq!(
            psd.image_resources
                .unwrap()
                .resolution_info
                .unwrap()
                .horizontal_resolution,
            300.
        );
        let layers = psd.children.unwrap();
        assert_eq!(layers.len(), 1);
        assert_eq!(
            layers[0].image_data.as_ref().unwrap().data,
            *composite.as_raw()
        );
        let reopened = super::super::decode(&bytes).unwrap().document;
        assert_eq!(render::render(&reopened, 2, 2).unwrap(), composite);
        // PSD's 8-bit merged preview stores white-matted RGB; alpha removal
        // quantizes semi-transparent colors. The exported layer remains exact.
        for (a, b) in psd.image_data.unwrap().data.iter().zip(composite.as_raw()) {
            assert!(a.abs_diff(*b) <= 2);
        }
        assert_eq!(doc, original);
    }
}

#[cfg(test)]
mod artboard_tests;
