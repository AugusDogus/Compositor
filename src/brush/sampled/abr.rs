//! Deferred sampled-tip import. Photoshop brush dynamics are not a paint engine here.
mod envelope;
use super::{MAX_TIP_PIXELS, Tip};
use crate::{Result, invalid};
use brushkit_abr::parse_abr_all_deferred_without_patterns as parse;
use std::{collections::HashSet, io::Read, path::Path, sync::Arc};

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_IMPORT_TIPS: usize = 32;
pub const MAX_IMPORT_PIXELS: usize = 16 * 1024 * 1024;
pub const LIMITATIONS: &str = "Only sampled tip shapes and spacing are imported. Photoshop dynamics, tip transforms, dual brushes and texture patterns are not imported. Painting uses the current foreground color and brush size.";

#[derive(Debug)]
pub struct TipInfo {
    pub name: String,
    pub size: [u32; 2],
    spacing: f64,
    unavailable: Option<String>,
}
impl TipInfo {
    pub fn unavailable(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }
    pub fn pixels(&self) -> usize {
        self.size[0] as usize * self.size[1] as usize
    }
}
#[derive(Debug)]
pub struct Pack {
    bytes: Arc<Vec<u8>>,
    tips: Vec<TipInfo>,
    report: String,
}
impl Pack {
    pub fn tips(&self) -> &[TipInfo] {
        &self.tips
    }
    pub fn report(&self) -> &str {
        &self.report
    }
    pub fn read(path: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path)?;
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            return Err(invalid(
                "Choose a regular ABR file no larger than 64 MiB. No tips were loaded.",
            ));
        }
        let file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            return Err(invalid(
                "The ABR file changed or exceeds 64 MiB. Choose a regular ABR file; no tips were loaded.",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        Self::from_bytes(bytes)
    }
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(invalid(
                "The ABR pack exceeds 64 MiB. Export a smaller pack; no tips were loaded.",
            ));
        }
        let framing = envelope::inspect(&bytes)?;
        let deferred = parse(&bytes).map_err(|e| {
            invalid(format!(
                "Could not read the ABR pack: {e}. Export the pack again; no tips were loaded."
            ))
        })?;
        let pack = &deferred.pack;
        if pack.brushes.len() > 2048 || pack.preset_count > 2048 {
            return Err(invalid(
                "The ABR pack contains more than 2048 presets. Export a smaller pack; no tips were loaded.",
            ));
        }
        if framing
            .legacy_spacing
            .as_ref()
            .is_some_and(|s| s.len() != pack.brushes.len())
        {
            return Err(invalid(
                "The ABR pack has inconsistent sampled records. Export the pack again; no tips were loaded.",
            ));
        }
        let mut default_spacing = 0;
        let mut tips = Vec::with_capacity(pack.brushes.len());
        for (index, brush) in pack.brushes.iter().enumerate() {
            let spacing = if let Some(spacing) = &framing.legacy_spacing {
                spacing[index]
            } else if let Some(percent) = brush.descriptor.spacing_pct {
                percent / 100.
            } else {
                default_spacing += 1;
                0.25
            };
            let size = [brush.tip.width, brush.tip.height];
            let pixels = u64::from(size[0]) * u64::from(size[1]);
            let unavailable = if size.contains(&0)
                || size.iter().any(|v| *v > 4096)
                || pixels > MAX_TIP_PIXELS as u64
            {
                Some("Tip exceeds 4096 px per side or 4 million pixels, or is empty".into())
            } else if !(0.01..=10.).contains(&spacing) {
                Some("Tip spacing must be 1 to 1000%".into())
            } else if !matches!(brush.tip.depth, 8 | 16)
                || deferred.tip_decoded_len(index)
                    != pixels as usize * usize::from(brush.tip.depth / 8)
            {
                Some("Unsupported or inconsistent tip bitmap".into())
            } else {
                None
            };
            tips.push(TipInfo {
                name: if brush.name.trim().is_empty() {
                    format!("ABR tip {}", index + 1)
                } else {
                    brush.name.chars().take(256).collect()
                },
                size,
                spacing,
                unavailable,
            });
        }
        let skipped = if framing.legacy_spacing.is_some() {
            pack.preset_count.saturating_sub(pack.brushes.len())
        } else {
            pack.computed_presets.len() + pack.unsupported_tip_count + pack.skipped_preset_count
        };
        let mut report = format!(
            "{} sampled tips. {skipped} computed, unsupported or unresolved presets omitted. {} auxiliary tips omitted.",
            tips.len(),
            pack.dropped_samp_count
        );
        if framing.patterns {
            report.push_str(" Embedded texture patterns omitted.");
        }
        if default_spacing > 0 {
            report.push_str(&format!(
                " {default_spacing} tips have no spacing metadata and use 25%."
            ));
        }
        if pack.desc_parse_error.is_some() {
            report.push_str(" Preset descriptions could not be read; available bitmap names and spacing may be incomplete.");
        }
        drop(deferred);
        Ok(Self {
            bytes: Arc::new(bytes),
            tips,
            report,
        })
    }
    pub fn decode(&self, selected: &[usize]) -> Result<Vec<Tip>> {
        if selected.is_empty() || selected.len() > MAX_IMPORT_TIPS {
            return Err(invalid(
                "Select 1 to 32 sampled tips to import. No tips were loaded.",
            ));
        }
        let mut seen = HashSet::new();
        let mut pixels = 0;
        for index in selected {
            let info = self
                .tips
                .get(*index)
                .filter(|_| seen.insert(*index))
                .ok_or_else(|| {
                    invalid("The ABR selection is invalid. Reopen the pack; no tips were loaded.")
                })?;
            if let Some(reason) = &info.unavailable {
                return Err(invalid(format!(
                    "Cannot import '{}': {reason}. No tips were loaded.",
                    info.name
                )));
            }
            pixels += info.pixels();
            if pixels > MAX_IMPORT_PIXELS {
                return Err(invalid(
                    "Selected tips exceed 16 million pixels. Select fewer tips; no tips were loaded.",
                ));
            }
        }
        // Every individual and aggregate allocation was checked before any bitmap decode.
        let deferred = parse(&self.bytes).map_err(|e| {
            invalid(format!(
                "Could not reopen the ABR pack: {e}. No tips were loaded."
            ))
        })?;
        let mut tips = Vec::with_capacity(selected.len());
        for index in selected {
            let info = &self.tips[*index];
            let bitmap = deferred.decode_tip(*index).map_err(|e| {
                invalid(format!(
                    "Could not decode '{}': {e}. No tips were loaded. Deselect this tip and retry.",
                    info.name
                ))
            })?;
            let bytes = if bitmap.depth == 16 {
                bitmap
                    .data
                    .chunks_exact(2)
                    .map(|v| ((u32::from(u16::from_be_bytes([v[0], v[1]])) + 128) / 257) as u8)
                    .collect()
            } else {
                bitmap.data
            };
            let pixels =
                image::GrayImage::from_raw(info.size[0], info.size[1], bytes).ok_or_else(|| {
                    invalid("An ABR tip decoded to the wrong size. No tips were loaded.")
                })?;
            tips.push(Tip {
                name: info.name.clone(),
                pixels,
                spacing: info.spacing,
                colored: false,
            });
        }
        Ok(tips)
    }
}

#[cfg(test)]
mod tests;
