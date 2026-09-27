//! PSD writes signed integer slider values; fractional settings render instead.
use crate::{Result, adjustment::ExtendedAdjustment, invalid, vibrance::Vibrance};
use ag_psd::psd as ps;

pub(super) fn import(source: &ps::VibranceAdjustment) -> Result<ExtendedAdjustment> {
    let values = [
        source.vibrance.unwrap_or(0.),
        source.saturation.unwrap_or(0.),
    ];
    if values.iter().any(|value| !(-100. ..=100.).contains(value)) {
        return Err(invalid(
            "PSD Vibrance and Saturation must be between -100% and 100%. The source file is unchanged.",
        ));
    }
    Ok(ExtendedAdjustment::Vibrance(Vibrance::new(
        values[0] as f32,
        values[1] as f32,
    )?))
}

pub(super) fn export(settings: Vibrance) -> Option<ps::AdjustmentLayer> {
    if settings.vibrance().fract() != 0. || settings.saturation().fract() != 0. {
        return None;
    }
    Some(ps::AdjustmentLayer::Vibrance(ps::VibranceAdjustment {
        vibrance: Some(f64::from(settings.vibrance())),
        saturation: Some(f64::from(settings.saturation())),
    }))
}

#[cfg(test)]
mod tests;
