//! Decode oversized Photoshop layers through bounded canvas intersections.
//! ag-psd retains compressed channels first; no source-sized bitmap is allocated.
mod channel;

use super::ConversionReport;
use crate::{Result, document, invalid};
use ag_psd::psd::{self as ps, ChannelId, Compression};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}
impl Rect {
    fn new(left: Option<f64>, top: Option<f64>, right: Option<f64>, bottom: Option<f64>) -> Self {
        Self {
            left: left.unwrap_or(0.) as i64,
            top: top.unwrap_or(0.) as i64,
            right: right.unwrap_or(0.) as i64,
            bottom: bottom.unwrap_or(0.) as i64,
        }
    }
    fn width(self) -> usize {
        (self.right - self.left) as usize
    }
    fn height(self) -> usize {
        (self.bottom - self.top) as usize
    }
    fn intersection(self, canvas: [i64; 2]) -> Self {
        let left = self.left.clamp(0, canvas[0]);
        let top = self.top.clamp(0, canvas[1]);
        Self {
            left,
            top,
            right: self.right.clamp(left, canvas[0]),
            bottom: self.bottom.clamp(top, canvas[1]),
        }
    }
    fn translated(self, origin: [i64; 2]) -> Self {
        Self {
            left: self.left + origin[0],
            right: self.right + origin[0],
            top: self.top + origin[1],
            bottom: self.bottom + origin[1],
        }
    }
    fn pixels(self) -> u64 {
        self.width() as u64 * self.height() as u64
    }
}

#[derive(Clone, Copy)]
struct Plan {
    source: Rect,
    target: Rect,
    fill: Option<u8>,
}
impl Plan {
    fn layer(layer: &ps::Layer, canvas: [i64; 2]) -> Self {
        let source = Rect::new(layer.left, layer.top, layer.right, layer.bottom);
        Self {
            source,
            target: source.intersection(canvas),
            fill: None,
        }
    }
    fn mask(mask: &ps::LayerMaskData, origin: Rect, canvas: [i64; 2]) -> Self {
        let mut source = Rect::new(mask.left, mask.top, mask.right, mask.bottom);
        if mask.position_relative_to_layer.unwrap_or(false) {
            source = source.translated([origin.left, origin.top]);
        }
        let target = source.intersection(canvas);
        if target.pixels() == 0 {
            Self {
                source,
                target: Rect {
                    left: 0,
                    top: 0,
                    right: 1,
                    bottom: 1,
                },
                fill: Some(mask.default_color.unwrap_or(0.) as u8),
            }
        } else {
            Self {
                source,
                target,
                fill: None,
            }
        }
    }
    fn changed(self) -> bool {
        self.source != self.target
    }
    fn mask_bounds(self, mask: &mut ps::LayerMaskData) {
        mask.left = Some(self.target.left as f64);
        mask.top = Some(self.target.top as f64);
        mask.right = Some(self.target.right as f64);
        mask.bottom = Some(self.target.bottom as f64);
        mask.position_relative_to_layer = Some(false);
    }
}

pub(super) fn decode(psd: &mut ps::Psd, report: &mut ConversionReport) -> Result<()> {
    let canvas = [psd.width as i64, psd.height as i64];
    let layers = psd.children.as_deref_mut().unwrap_or_default();
    let mut budget = psd.width as u64 * psd.height as u64;
    retained_budget(layers, canvas, &mut budget)?;
    document::validate_pixel_budget(budget)?;
    decode_layers(layers, canvas, report)
}

fn retained_budget(layers: &[ps::Layer], canvas: [i64; 2], budget: &mut u64) -> Result<()> {
    for layer in layers {
        let plan = Plan::layer(layer, canvas);
        if plan.target.pixels() > 0 {
            document::validate_size(plan.target.width() as u32, plan.target.height() as u32)?;
            *budget += plan.target.pixels();
        }
        for mask in [
            &layer.additional_info.mask,
            &layer.additional_info.real_mask,
        ]
        .into_iter()
        .flatten()
        {
            let plan = Plan::mask(mask, plan.source, canvas);
            // Import adds a default-tone border around each decoded mask.
            let width = plan.target.width() as u32 + 2;
            let height = plan.target.height() as u32 + 2;
            document::validate_size(width, height)?;
            *budget += u64::from(width) * u64::from(height);
        }
        retained_budget(
            layer.children.as_deref().unwrap_or_default(),
            canvas,
            budget,
        )?;
    }
    Ok(())
}

fn decode_layers(
    layers: &mut [ps::Layer],
    canvas: [i64; 2],
    report: &mut ConversionReport,
) -> Result<()> {
    for layer in layers {
        let image = Plan::layer(layer, canvas);
        let mask = layer
            .additional_info
            .mask
            .as_ref()
            .map(|m| Plan::mask(m, image.source, canvas));
        let real = layer
            .additional_info
            .real_mask
            .as_ref()
            .map(|m| Plan::mask(m, image.source, canvas));
        let cropped =
            image.changed() || mask.is_some_and(Plan::changed) || real.is_some_and(Plan::changed);
        if let Some(raw) = &mut layer.raw_data {
            for channel in &mut raw.channels {
                let plan = match channel.id {
                    ChannelId::UserMask => mask,
                    ChannelId::RealUserMask => real,
                    _ => Some(image),
                };
                let Some(plan) = plan else {
                    continue;
                };
                if plan.changed() || plan.fill.is_some() {
                    channel.data = Some(channel::crop(
                        channel.data.as_deref().unwrap_or_default(),
                        channel.compression,
                        raw.large,
                        plan,
                    )?);
                    channel.compression = Compression::RawData;
                }
            }
        }
        layer.left = Some(image.target.left as f64);
        layer.top = Some(image.target.top as f64);
        layer.right = Some(image.target.right as f64);
        layer.bottom = Some(image.target.bottom as f64);
        if let (Some(plan), Some(mask)) = (mask, &mut layer.additional_info.mask) {
            plan.mask_bounds(mask);
        }
        if let (Some(plan), Some(mask)) = (real, &mut layer.additional_info.real_mask) {
            plan.mask_bounds(mask);
        }
        ag_psd::reader::decode_layer_pixels(layer, true).map_err(|e| {
            invalid(format!(
                "Cropped PSD pixels could not be decoded: {e}. The current document is unchanged."
            ))
        })?;
        if cropped {
            report.note(format!("{}: cropped to the canvas to fit memory limits. Pixels outside the canvas were not imported.", layer.additional_info.name.as_deref().unwrap_or("Layer")));
        }
        decode_layers(
            layer.children.as_deref_mut().unwrap_or_default(),
            canvas,
            report,
        )?;
    }
    Ok(())
}
