use super::ConversionReport;
use crate::{
    document::Document,
    guides::{Axis, Guide},
};
use ag_psd::psd::*;
use uuid::Uuid;

pub(super) fn import(
    document: &mut Document,
    resources: Option<ImageResources>,
    report: &mut ConversionReport,
) {
    let Some(resources) = resources else {
        return;
    };
    if let Some(resolution) = resources.resolution_info {
        // PSD stores both resolutions in pixels per inch; the unit is display metadata.
        if (1. ..=9600.).contains(&resolution.horizontal_resolution) {
            document.resolution = resolution.horizontal_resolution;
            if (resolution.horizontal_resolution - resolution.vertical_resolution).abs() > 0.001 {
                report.note("PSD uses unequal horizontal and vertical resolution; horizontal resolution is used for both axes.");
            }
        } else {
            report.note("PSD print resolution is unsupported and is reset to 72 DPI.");
        }
    }
    if let Some(info) = resources.grid_and_guides_information {
        document.guides = info
            .guides
            .unwrap_or_default()
            .into_iter()
            .map(|guide| Guide {
                id: Uuid::new_v4(),
                position: guide.location,
                axis: match guide.direction {
                    GuideDirection::Horizontal => Axis::Horizontal,
                    GuideDirection::Vertical => Axis::Vertical,
                },
            })
            .collect();
    }
}
pub(super) fn export(document: &Document) -> ImageResources {
    ImageResources {
        resolution_info: Some(ResolutionInfo {
            horizontal_resolution: document.resolution,
            vertical_resolution: document.resolution,
            horizontal_resolution_unit: ResolutionUnit::Ppi,
            vertical_resolution_unit: ResolutionUnit::Ppi,
            width_unit: DimensionUnit::Inches,
            height_unit: DimensionUnit::Inches,
        }),
        grid_and_guides_information: (!document.guides.is_empty()).then(|| {
            GridAndGuidesInformation {
                grid: None,
                guides: Some(
                    document
                        .guides
                        .iter()
                        .filter(|guide| guide.position >= 0.)
                        .map(|guide| GuideInfo {
                            location: guide.position,
                            direction: match guide.axis {
                                Axis::Horizontal => GuideDirection::Horizontal,
                                Axis::Vertical => GuideDirection::Vertical,
                            },
                        })
                        .collect(),
                ),
            }
        }),
        ..Default::default()
    }
}

// ag-psd 0.3.0 leaves unknown image-resource payloads unread, making its strict
// section reader reject otherwise valid Photoshop files. Omit only those opaque
// resources from the decoder view after preflight has validated their framing.
// Recognized resources and all layer/channel data still use the strict reader.
pub(super) fn decoder_bytes<'a>(
    bytes: &'a [u8],
    length_offset: usize,
    end: usize,
    retained: &[std::ops::Range<usize>],
) -> std::borrow::Cow<'a, [u8]> {
    let start = length_offset + 4;
    let length: usize = retained.iter().map(std::ops::Range::len).sum();
    if length == end - start {
        return std::borrow::Cow::Borrowed(bytes);
    }
    let mut output = Vec::with_capacity(bytes.len() - (end - start) + length);
    output.extend_from_slice(&bytes[..length_offset]);
    // Preflight caps the entire file at 512 MiB.
    output.extend_from_slice(&(length as u32).to_be_bytes());
    for range in retained {
        output.extend_from_slice(&bytes[start + range.start..start + range.end]);
    }
    output.extend_from_slice(&bytes[end..]);
    std::borrow::Cow::Owned(output)
}
