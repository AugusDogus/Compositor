//! PSD posterize levels are integers from 2 through 256.
use crate::{Result, adjustment::ExtendedAdjustment, invalid, posterize::Posterize};
use ag_psd::psd as ps;

pub(super) fn import(source: &ps::PosterizeAdjustment) -> Result<ExtendedAdjustment> {
    let levels = source.levels.filter(|levels| {
        (2. ..=256.).contains(levels) && levels.fract() == 0.
    }).ok_or_else(|| invalid(
        "PSD Posterize requires an integer level count between 2 and 256. The source file is unchanged.",
    ))?;
    Ok(ExtendedAdjustment::Posterize(Posterize::new(
        levels as u16,
    )?))
}

pub(super) fn export(settings: Posterize) -> ps::AdjustmentLayer {
    ps::AdjustmentLayer::Posterize(ps::PosterizeAdjustment {
        levels: Some(f64::from(settings.levels())),
    })
}

#[cfg(test)]
mod tests;
