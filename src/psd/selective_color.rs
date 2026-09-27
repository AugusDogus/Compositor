//! PSD selc stores nine CMYK rows as signed integer percentages.
use crate::{
    Result,
    adjustment::ExtendedAdjustment,
    invalid,
    selective_color::{Mode, SelectiveColor},
};
use ag_psd::psd as ps;

pub(super) fn import(source: &ps::SelectiveColorAdjustment) -> Result<ExtendedAdjustment> {
    let mut settings = SelectiveColor {
        mode: match source.mode.unwrap_or(ps::SelectiveColorMode::Relative) {
            ps::SelectiveColorMode::Relative => Mode::Relative,
            ps::SelectiveColorMode::Absolute => Mode::Absolute,
        },
        ..Default::default()
    };
    for (row, source) in settings.adjustments.iter_mut().zip([
        &source.reds,
        &source.yellows,
        &source.greens,
        &source.cyans,
        &source.blues,
        &source.magentas,
        &source.whites,
        &source.neutrals,
        &source.blacks,
    ]) {
        let source = source.as_ref().ok_or_else(|| {
            invalid("PSD Selective Color is missing a color range. The source file is unchanged.")
        })?;
        let values = [source.c, source.m, source.y, source.k];
        if values.iter().any(|value| !(-100. ..=100.).contains(value)) {
            return Err(invalid(
                "PSD Selective Color CMYK values must be between -100% and 100%. The source file is unchanged.",
            ));
        }
        *row = values.map(|value| value as f32);
    }
    settings.validate()?;
    Ok(ExtendedAdjustment::SelectiveColor(settings))
}

pub(super) fn export(settings: &SelectiveColor) -> Option<ps::AdjustmentLayer> {
    // PSD integer coefficients cannot retain fractional Linux settings.
    if settings.validate().is_err()
        || settings
            .adjustments
            .iter()
            .flatten()
            .any(|value| value.fract() != 0.)
    {
        return None;
    }
    let [
        reds,
        yellows,
        greens,
        cyans,
        blues,
        magentas,
        whites,
        neutrals,
        blacks,
    ] = settings.adjustments.map(|[c, m, y, k]| {
        Some(ps::Cmyk {
            c: f64::from(c),
            m: f64::from(m),
            y: f64::from(y),
            k: f64::from(k),
        })
    });
    Some(ps::AdjustmentLayer::SelectiveColor(
        ps::SelectiveColorAdjustment {
            mode: Some(match settings.mode {
                Mode::Relative => ps::SelectiveColorMode::Relative,
                Mode::Absolute => ps::SelectiveColorMode::Absolute,
            }),
            reds,
            yellows,
            greens,
            cyans,
            blues,
            magentas,
            whites,
            neutrals,
            blacks,
        },
    ))
}

#[cfg(test)]
mod tests;
