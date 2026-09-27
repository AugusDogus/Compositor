//! PSD composite-gray ranges map directly to independent byte split handles.
use super::{ConversionReport, invalid};
use crate::{
    Result,
    blend_if::{Range, Settings},
};
use ag_psd::psd::BlendingRanges;

pub(super) fn preflight(bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    if bytes.len() < 8 || !bytes.len().is_multiple_of(8) {
        return Err(invalid(
            "Photoshop Blend If data is truncated. Re-export a complete PSD; the source file is unchanged.",
        ));
    }
    for points in bytes.chunks_exact(4) {
        Range::from_endpoints([points[0], points[1], points[2], points[3]])?;
    }
    Ok(())
}

fn range(values: &[f64]) -> Result<Range> {
    if values.len() != 4
        || values
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=255.).contains(v) || v.fract() != 0.)
    {
        return Err(invalid(
            "Photoshop Blend If ranges must contain four byte endpoints. Re-export the source as an 8-bit RGB PSD; the file is unchanged.",
        ));
    }
    Range::from_endpoints([
        values[0] as u8,
        values[1] as u8,
        values[2] as u8,
        values[3] as u8,
    ])
}

pub(super) fn import(
    ranges: Option<&BlendingRanges>,
    report: &mut ConversionReport,
    name: &str,
) -> Result<Option<Settings>> {
    let Some(ranges) = ranges else {
        return Ok(None);
    };
    for channel in &ranges.ranges {
        if !range(&channel.source_range)?.is_identity()
            || !range(&channel.dest_range)?.is_identity()
        {
            return Err(invalid(format!(
                "{name}: this PSD uses channel-specific Blend If ranges. Only Gray ranges are supported. Rasterize that layer in the source editor or switch its Blend If channel to Gray before exporting; the source file is unchanged."
            )));
        }
    }
    let settings = Settings {
        enabled: true,
        source: range(&ranges.composite_gray_blend_source)?,
        underlying: range(&ranges.composite_graph_blend_destination_range)?,
    };
    if settings.is_identity() {
        return Ok(None);
    }
    report.note(format!("{name}: Gray Blend If split handles remain editable. Gray tone, transparent backdrops and layer-effect interactions can render differently from Photoshop."));
    Ok(Some(settings))
}

pub(super) fn export(settings: Option<Settings>) -> Option<BlendingRanges> {
    let settings = settings.filter(|s| !s.is_identity())?;
    Some(BlendingRanges {
        composite_gray_blend_source: settings.source.endpoints().map(f64::from).to_vec(),
        composite_graph_blend_destination_range: settings
            .underlying
            .endpoints()
            .map(f64::from)
            .to_vec(),
        ranges: Vec::new(),
    })
}

pub(super) fn notice(settings: Settings, report: &mut ConversionReport, name: &str) {
    if !settings.enabled {
        report.note(format!("{name}: disabled Blend If handles are omitted from PSD output. Save a .comp project to retain the inactive settings."));
    } else if !settings.is_identity() {
        report.note(format!("{name}: Gray Blend If split handles remain editable. Photoshop may render gray tones, transparent backdrops and layer-effect interactions differently; the saved composite retains the Linux appearance."));
    }
}
