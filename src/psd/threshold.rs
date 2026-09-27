//! PSD threshold levels use unsigned integer values in the 0..255 range.
use crate::{Result, adjustment::ExtendedAdjustment, invalid, threshold::Threshold};
use ag_psd::psd as ps;

pub(super) fn import(source: &ps::ThresholdAdjustment) -> Result<ExtendedAdjustment> {
    let level = source.level.filter(|level| {
        (0. ..=255.).contains(level) && level.fract() == 0.
    }).ok_or_else(|| invalid(
        "PSD Threshold requires an integer level between 0 and 255. The source file is unchanged.",
    ))?;
    Ok(ExtendedAdjustment::Threshold(Threshold {
        level: level as u8,
    }))
}

pub(super) fn export(settings: Threshold) -> ps::AdjustmentLayer {
    ps::AdjustmentLayer::Threshold(ps::ThresholdAdjustment {
        level: Some(f64::from(settings.level)),
    })
}

#[cfg(test)]
mod tests;
