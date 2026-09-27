//! Two file layouts, selected by the header's major version (all big-endian):
//!
//! **Legacy v1/v2** (Photoshop 6 and older, Photoshop 7 to CS): the header's
//! `subversion` is the brush count, followed by that many length-prefixed
//! entries, each a spacing plus a sampled bitmap (v2 adds a Unicode name).
//! Read by `parse_legacy`.
//!
//! **8BIM blocks v6/v7/v9/v10** (Photoshop CS and later):
//!   - 2 bytes: major version (6, 7, 9 or 10)
//!   - 2 bytes: subversion (1 or 2)
//!   - Sequence of 8BIM blocks:
//!     - 4 bytes: signature "8BIM"
//!     - 4 bytes: block type (e.g. "samp", "desc", "patt", "phry")
//!     - 4 bytes: block data length
//!     - N bytes: block data
//!
//! In both cases, the bitmap header consists of:
//!   4×i32: top, left, bottom, right (signed rectangle)
//!   u16: depth (8 or 16)
//!   u8: compression (0=raw, 1=RLE)
//!   followed by pixel data.

use byteorder::{BigEndian, ReadBytesExt};
use flate2::read::ZlibDecoder;
use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::ops::Range;

use super::descriptor::{extract_all_brush_info_inner, BrushDescInfo, ExtractError};
use super::pattern::parse_patt_block;
use super::{
    AbrBrush, AbrError, AbrPack, AbrVersion, BrushDescriptor, ComputedPreset, DroppedTipDetail,
    SkippedPresetDetail, TipBitmap, UnsupportedTipPresetDetail,
};
use crate::limits::{MAX_DIMENSION, MAX_NAME_CODE_UNITS, MAX_RECORDS};

const MAX_TIP_DECODED_BYTES: usize = 256 * 1024 * 1024;

pub fn parse_abr(bytes: &[u8]) -> Result<AbrPack, AbrError> {
    parse_abr_with(bytes, Tips::Eager, PatternMode::Read).map(|parsed| parsed.pack)
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum PatternMode {
    Read,
    Skip,
}

/// Everything `parse_abr_with` produces: the pack itself plus the bookkeeping
/// only the deferred mode uses. In `Tips::Eager` every tip vector holds
/// `None`.
struct ParsedAbr {
    pack: AbrPack,
    /// Aligned with `pack.brushes`.
    tips: Vec<Option<DeferredTip>>,
    /// Aligned with `pack.dropped_tip_details`.
    dropped_tips: Vec<Option<DeferredTip>>,
    /// Every `dual_brush_uuid` any preset names, sampled or computed. Filled
    /// only in `Tips::Deferred(Defer::Paired)`, the one mode that reads it.
    dual_uuids: HashSet<String>,
}

/// Whether a parse decodes tips as it reads them or defers them.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Tips {
    Eager,
    Deferred(Defer),
}

fn parse_abr_with(bytes: &[u8], tips: Tips, patterns: PatternMode) -> Result<ParsedAbr, AbrError> {
    let mut cursor = Cursor::new(bytes);
    let (version, subversion) = read_header(&mut cursor)?;

    match version {
        AbrVersion::V1 | AbrVersion::V2 => {
            let defer = tips == Tips::Deferred(Defer::All);
            return parse_legacy(&mut cursor, subversion, version, defer);
        }
        AbrVersion::V6 | AbrVersion::V7 | AbrVersion::V9 | AbrVersion::V10 => {}
    }

    let blocks = read_blocks(&mut cursor, patterns)?;

    let mut bitmaps: Vec<Option<SampEntry>> = Vec::new();
    for block in &blocks {
        if block.block_type == "samp" {
            let block_mode = match tips {
                Tips::Eager => TipMode::Eager,
                Tips::Deferred(_) => TipMode::Deferred {
                    block_start: block.start,
                },
            };
            let entries = parse_samp_block(
                block.data,
                version,
                subversion,
                block_mode,
                MAX_RECORDS - bitmaps.len(),
            )?;
            bitmaps.extend(entries);
        }
    }

    let mut desc_infos: Vec<BrushDescInfo> = Vec::new();
    let mut raw_desc_block: Option<Vec<u8>> = None;
    let mut desc_parse_error: Option<String> = None;
    for block in &blocks {
        if block.block_type == "desc" {
            match extract_all_brush_info_inner(block.data, MAX_RECORDS - desc_infos.len()) {
                Ok(infos) => {
                    if !infos.is_empty() {
                        desc_infos.extend(infos);
                    }
                }
                Err(ExtractError::RecordLimit) => return Err(AbrError::RecordLimit("descriptors")),
                Err(e) => {
                    if desc_parse_error.is_none() {
                        desc_parse_error = Some(e.to_string());
                    }
                }
            }
            if raw_desc_block.is_none() {
                raw_desc_block = Some(block.data.to_vec());
            }
        }
    }
    let preset_count = desc_infos.len();

    let mut patterns = Vec::new();
    let mut dropped_pattern_details = Vec::new();
    let mut unreadable_patt_chunks = Vec::new();
    let mut records_seen = 0usize;
    let mut patt_block_index = 0usize;
    for block in &blocks {
        if block.block_type == "patt" && !block.data.is_empty() {
            let outcome = parse_patt_block(block.data, patt_block_index, records_seen);
            records_seen +=
                outcome.patterns.len() + outcome.dropped.len() + outcome.unreadable.len();
            dropped_pattern_details.extend(outcome.dropped);
            unreadable_patt_chunks.extend(outcome.unreadable);
            patt_block_index += 1;
            patterns.extend(outcome.patterns);
        }
    }

    let dual_uuids: HashSet<String> = match tips {
        Tips::Deferred(Defer::Paired) => desc_infos
            .iter()
            .filter_map(|info| info.dual_brush_uuid.clone())
            .collect(),
        Tips::Eager | Tips::Deferred(Defer::All) => HashSet::new(),
    };

    let valid_bitmaps: Vec<SampEntry> = bitmaps.into_iter().flatten().collect();
    let PairedBrushes {
        brushes,
        dropped_samp_count,
        skipped_preset_count,
        dropped_tip_details,
        skipped_preset_details,
        tips,
        dropped_tips,
    } = pair_brushes(valid_bitmaps, &desc_infos);

    let computed_presets = desc_infos
        .iter()
        .enumerate()
        .filter(|(_, info)| info.sampled_data_uuid.is_none() && info.descriptor.computed.is_some())
        .map(|(pi, info)| ComputedPreset {
            name: info.name.clone(),
            descriptor: info.descriptor.clone(),
            preset_index: Some(pi),
        })
        .collect();

    let unsupported_tip_presets = collect_unsupported_tip_presets(&desc_infos);

    Ok(ParsedAbr {
        pack: AbrPack {
            version,
            brushes,
            preset_count,
            raw_desc_block,
            computed_presets,
            patterns,
            dropped_pattern_count: dropped_pattern_details.len(),
            dropped_pattern_details,
            unreadable_patt_chunk_count: unreadable_patt_chunks.len(),
            unreadable_patt_chunks,
            dropped_samp_count,
            skipped_preset_count,
            dropped_tip_details,
            skipped_preset_details,
            unsupported_tip_count: unsupported_tip_presets.len(),
            unsupported_tip_presets,
            desc_parse_error,
        },
        tips,
        dropped_tips,
        dual_uuids,
    })
}

/// Whether `parse_samp_block` decodes a tip's pixels now or records where to
/// find them. `block_start` is the input offset of the samp block's payload.
#[derive(Copy, Clone, PartialEq, Eq)]
enum TipMode {
    Eager,
    Deferred { block_start: usize },
}

/// One sampled tip whose pixels are still compressed inside a samp block, or
/// inside the input of a v1/v2 pack.
///
/// Produced by [`parse_abr_deferred`], [`parse_abr_deferred_without_patterns`]
/// and [`parse_abr_all_deferred_without_patterns`], and only useful through
/// [`DeferredPack::decode_tip`], which replays the exact slice and header the
/// eager parse would have decoded.
#[derive(Debug, Clone)]
pub struct DeferredTip {
    /// The samp entry's bytes in the input. For a v1/v2 tip, its rect through
    /// its last pixel byte.
    entry: Range<usize>,
    header: BitmapHeader,
    /// `width * height * bytes_per_pixel` — what `decode_tip` will allocate.
    decoded_len: usize,
}

/// A pack whose sampled tips are still compressed. It borrows the input it
/// was parsed from, where the compressed tips stay until decoded.
///
/// The invariant: whenever [`is_deferred`](DeferredPack::is_deferred) is true
/// for brush `i`, `pack.brushes[i].tip.data` is EMPTY — the width, height and
/// depth are correct, the pixels are not there. Pixels come only from
/// [`decode_tip`](DeferredPack::decode_tip), which decodes one tip per call and
/// hands the caller the sole copy. A deferred pack is the only place in the
/// workspace where a sampled `TipBitmap` may carry no data; a consumer that
/// reads `brush.tip.data` directly on a deferred pack silently sees a blank
/// tip. From [`parse_abr_all_deferred_without_patterns`] the same holds for
/// every `pack.dropped_tip_details[*].bitmap`, which nothing decodes.
///
/// [`parse_abr_deferred`] and [`parse_abr_deferred_without_patterns`] decode
/// two kinds of tip eagerly, so the converter's dual-brush lookups keep
/// working unchanged: every tip in `pack.dropped_tip_details`, and every brush
/// a preset names as its dual brush. They also decode every tip of a v1 or v2
/// pack. `is_deferred` is false for those.
/// [`parse_abr_all_deferred_without_patterns`] decodes none of them.
pub struct DeferredPack<'a> {
    pub pack: AbrPack,
    tips: Vec<Option<DeferredTip>>,
    input: &'a [u8],
}

impl DeferredPack<'_> {
    /// True when brush `i`'s pixels still have to come from `decode_tip`.
    pub fn is_deferred(&self, i: usize) -> bool {
        self.tips.get(i).is_some_and(Option::is_some)
    }

    /// What brush `i`'s pixels will occupy once decoded, without decoding them.
    pub fn tip_decoded_len(&self, i: usize) -> usize {
        match self.tips.get(i).and_then(Option::as_ref) {
            Some(tip) => tip.decoded_len,
            None => self.pack.brushes[i].tip.data.len(),
        }
    }

    /// Decode brush `i`'s tip. Equal, byte for byte, to what `parse_abr` puts
    /// in `brushes[i].tip`; clones the already-decoded tip for an eager brush.
    pub fn decode_tip(&self, i: usize) -> Result<TipBitmap, AbrError> {
        match self.tips.get(i).and_then(Option::as_ref) {
            Some(tip) => decode_deferred_tip(self.input, tip),
            None => Ok(self.pack.brushes[i].tip.clone()),
        }
    }
}

fn decode_deferred_tip(input: &[u8], tip: &DeferredTip) -> Result<TipBitmap, AbrError> {
    decode_bitmap(&input[tip.entry.clone()], &tip.header)
}

/// Parse a pack without decoding its sampled tips, so a caller can hold the
/// input plus one decoded tip at a time instead of all of them.
///
/// See [`DeferredPack`] for the empty-`tip.data` invariant and for the tips
/// this still decodes eagerly.
pub fn parse_abr_deferred(bytes: &[u8]) -> Result<DeferredPack<'_>, AbrError> {
    parse_abr_deferred_with(bytes, PatternMode::Read, Defer::Paired)
}

/// Like [`parse_abr_deferred`], but embedded pattern payloads are neither
/// copied nor decoded.
///
/// `patterns`, `dropped_pattern_details`, and `unreadable_patt_chunks` are
/// empty, and both pattern diagnostic counts are zero because patterns were
/// not inspected. All other fields and decoded tips are unchanged.
/// Declared block lengths are still checked against the input length.
pub fn parse_abr_deferred_without_patterns(bytes: &[u8]) -> Result<DeferredPack<'_>, AbrError> {
    parse_abr_deferred_with(bytes, PatternMode::Skip, Defer::Paired)
}

/// Like [`parse_abr_deferred_without_patterns`], but no tip is decoded up
/// front. Every brush whose tip has pixels is deferred, including the tips of
/// a v1 or v2 pack and the brushes a preset names as its dual brush, and every
/// `pack.dropped_tip_details[*].bitmap` has correct dimensions and empty data.
/// A tip whose pixels fail to decode fails its [`DeferredPack::decode_tip`]
/// call, not the parse. The exception is a raw v1/v2 tip whose pixels run past
/// the end of the input, which fails the parse as in [`parse_abr`]. See
/// [`DeferredPack`].
pub fn parse_abr_all_deferred_without_patterns(bytes: &[u8]) -> Result<DeferredPack<'_>, AbrError> {
    parse_abr_deferred_with(bytes, PatternMode::Skip, Defer::All)
}

/// Which tips a deferred parse leaves to `DeferredPack::decode_tip`.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Defer {
    /// All but the tips [`DeferredPack`] lists as decoded eagerly.
    Paired,
    /// Every tip.
    All,
}

fn parse_abr_deferred_with(
    bytes: &[u8],
    patterns: PatternMode,
    defer: Defer,
) -> Result<DeferredPack<'_>, AbrError> {
    let ParsedAbr {
        mut pack,
        mut tips,
        dropped_tips,
        dual_uuids,
    } = parse_abr_with(bytes, Tips::Deferred(defer), patterns)?;

    if defer == Defer::Paired {
        for (detail, deferred) in pack.dropped_tip_details.iter_mut().zip(&dropped_tips) {
            if let Some(tip) = deferred {
                detail.bitmap = decode_deferred_tip(bytes, tip)?;
            }
        }

        for (i, brush) in pack.brushes.iter_mut().enumerate() {
            if !dual_uuids.contains(&brush.id) {
                continue;
            }
            if let Some(tip) = tips[i].take() {
                brush.tip = decode_deferred_tip(bytes, &tip)?;
            }
        }
    }

    Ok(DeferredPack {
        pack,
        tips,
        input: bytes,
    })
}

fn collect_unsupported_tip_presets(
    desc_infos: &[BrushDescInfo],
) -> Vec<UnsupportedTipPresetDetail> {
    desc_infos
        .iter()
        .enumerate()
        .filter(|(_, info)| info.sampled_data_uuid.is_none() && info.descriptor.computed.is_none())
        .map(|(pi, info)| UnsupportedTipPresetDetail {
            name: info.name.clone(),
            preset_index: pi,
            has_shape_tip: info.has_shape_tip,
            shape_tip_family: info.shape_tip_family,
            tip_shape: info.tip_shape,
        })
        .collect()
}

struct PairedBrushes {
    brushes: Vec<AbrBrush>,
    dropped_samp_count: usize,
    skipped_preset_count: usize,
    dropped_tip_details: Vec<DroppedTipDetail>,
    skipped_preset_details: Vec<SkippedPresetDetail>,
    /// Aligned with `brushes`; all `None` in `TipMode::Eager`.
    tips: Vec<Option<DeferredTip>>,
    /// Aligned with `dropped_tip_details`; all `None` in `TipMode::Eager`.
    dropped_tips: Vec<Option<DeferredTip>>,
}

fn pair_brushes(valid_bitmaps: Vec<SampEntry>, desc_infos: &[BrushDescInfo]) -> PairedBrushes {
    let mut uuid_to_samp: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (i, samp) in valid_bitmaps.iter().enumerate() {
        if let Some(ref uuid) = samp.uuid {
            uuid_to_samp.insert(uuid.as_str(), i);
        }
    }

    let presets_first = desc_infos.iter().any(|info| {
        info.sampled_data_uuid
            .as_deref()
            .is_some_and(|u| uuid_to_samp.contains_key(u))
    });

    if presets_first {
        let mut brushes = Vec::new();
        let mut tips: Vec<Option<DeferredTip>> = Vec::new();
        let mut seen: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
        let mut used: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let mut skipped_preset_details: Vec<SkippedPresetDetail> = Vec::new();
        for (pi, info) in desc_infos.iter().enumerate() {
            let Some(uuid) = info.sampled_data_uuid.as_deref() else {
                continue;
            };
            let Some(&idx) = uuid_to_samp.get(uuid) else {
                skipped_preset_details.push(SkippedPresetDetail {
                    name: info.name.clone(),
                    uuid: uuid.to_string(),
                });
                continue;
            };
            used.insert(idx);
            let count = seen.entry(uuid).or_insert(0);
            *count += 1;
            let id = if *count == 1 {
                uuid.to_string()
            } else {
                format!("{uuid}-{count}")
            };
            brushes.push(AbrBrush {
                id,
                name: info.name.clone(),
                tip: valid_bitmaps[idx].bitmap.clone(),
                descriptor: info.descriptor.clone(),
                synthesized: false,
                preset_index: Some(pi),
            });
            tips.push(valid_bitmaps[idx].deferred.clone());
        }
        let mut owners: std::collections::HashMap<&str, Vec<String>> =
            std::collections::HashMap::new();
        for info in desc_infos {
            if let Some(dual) = info.dual_brush_uuid.as_deref() {
                let names = owners.entry(dual).or_default();
                if !names.contains(&info.name) {
                    names.push(info.name.clone());
                }
            }
        }

        // Duplicate unused sample UUIDs share one diagnostic list. Cloning
        // all owner strings per bitmap would multiply descriptor memory by
        // the number of samples, even when every bitmap remains deferred.
        let owners: std::collections::HashMap<&str, std::sync::Arc<[String]>> = owners
            .into_iter()
            .map(|(uuid, names)| (uuid, names.into()))
            .collect();

        let dropped_tips: Vec<Option<DeferredTip>> = valid_bitmaps
            .iter()
            .enumerate()
            .filter(|(idx, _)| !used.contains(idx))
            .map(|(_, samp)| samp.deferred.clone())
            .collect();

        let dropped_tip_details: Vec<DroppedTipDetail> = valid_bitmaps
            .iter()
            .enumerate()
            .filter(|(idx, _)| !used.contains(idx))
            .map(|(_, samp)| DroppedTipDetail {
                uuid: samp.uuid.clone(),
                width: samp.bitmap.width,
                height: samp.bitmap.height,
                owner_preset_names: samp
                    .uuid
                    .as_deref()
                    .and_then(|u| owners.get(u).cloned())
                    .unwrap_or_default(),
                bitmap: samp.bitmap.clone(),
            })
            .collect();

        return PairedBrushes {
            dropped_samp_count: dropped_tip_details.len(),
            skipped_preset_count: skipped_preset_details.len(),
            brushes,
            dropped_tip_details,
            skipped_preset_details,
            tips,
            dropped_tips,
        };
    }

    let mut brushes = Vec::new();
    let mut tips: Vec<Option<DeferredTip>> = Vec::new();
    for (i, samp) in valid_bitmaps.into_iter().enumerate() {
        let id = samp.uuid.clone().unwrap_or_else(|| format!("brush_{i}"));

        let matched = desc_infos.get(i);
        let name = matched.map(|info| info.name.clone()).unwrap_or_default();
        let descriptor = matched
            .map(|info| info.descriptor.clone())
            .unwrap_or_default();

        brushes.push(AbrBrush {
            id,
            name,
            tip: samp.bitmap,
            descriptor,
            synthesized: false,
            preset_index: matched.map(|_| i),
        });
        tips.push(samp.deferred);
    }

    brushes.reverse();
    tips.reverse();
    PairedBrushes {
        brushes,
        dropped_samp_count: 0,
        skipped_preset_count: 0,
        dropped_tip_details: Vec::new(),
        skipped_preset_details: Vec::new(),
        tips,
        dropped_tips: Vec::new(),
    }
}

/// Parse the legacy v1/v2 entry stream (no 8BIM blocks).
///
/// v1 and v2 share one entry layout; the only difference is that v2 carries
/// a Unicode name between `spacing` and `anti_aliasing` and v1 has no name
/// field at all.
///
/// With `defer`, every tip with pixels is deferred as a range of the input.
/// An entry's rect, depth and compression have the samp bitmap header layout,
/// so a `BitmapHeader` with `rect_offset` 0 decodes it.
fn parse_legacy(
    cursor: &mut Cursor<&[u8]>,
    brush_count: u16,
    version: AbrVersion,
    defer: bool,
) -> Result<ParsedAbr, AbrError> {
    if usize::from(brush_count) > MAX_RECORDS {
        return Err(AbrError::RecordLimit("legacy brushes"));
    }
    let mut brushes = Vec::new();
    let mut tips = Vec::new();

    for _ in 0..brush_count {
        let brush_type = cursor
            .read_u16::<BigEndian>()
            .map_err(|_| AbrError::InvalidHeader)?;

        if brush_type != 2 {
            let entry_len = cursor
                .read_u32::<BigEndian>()
                .map_err(|_| AbrError::InvalidHeader)? as u64;
            cursor.set_position(cursor.position() + entry_len);
            continue;
        }

        let entry_len = cursor
            .read_u32::<BigEndian>()
            .map_err(|_| AbrError::InvalidHeader)? as u64;
        let entry_end = cursor.position() + entry_len;

        cursor
            .read_u32::<BigEndian>()
            .map_err(|_| AbrError::InvalidHeader)?;
        cursor
            .read_u16::<BigEndian>()
            .map_err(|_| AbrError::InvalidHeader)?;

        let name = if version == AbrVersion::V2 {
            read_v2_unicode_name(cursor)?
        } else {
            String::new()
        };

        cursor.read_u8().map_err(|_| AbrError::InvalidHeader)?;

        for _ in 0..4 {
            cursor
                .read_i16::<BigEndian>()
                .map_err(|_| AbrError::InvalidHeader)?;
        }

        let rect_start = cursor.position() as usize;
        let top = cursor
            .read_i32::<BigEndian>()
            .map_err(|_| AbrError::InvalidHeader)?;
        let left = cursor
            .read_i32::<BigEndian>()
            .map_err(|_| AbrError::InvalidHeader)?;
        let bottom = cursor
            .read_i32::<BigEndian>()
            .map_err(|_| AbrError::InvalidHeader)?;
        let right = cursor
            .read_i32::<BigEndian>()
            .map_err(|_| AbrError::InvalidHeader)?;

        let width = i64::from(right) - i64::from(left);
        let height = i64::from(bottom) - i64::from(top);
        if width < 0
            || height < 0
            || width > i64::from(MAX_DIMENSION)
            || height > i64::from(MAX_DIMENSION)
        {
            return Err(entry_err(
                cursor.position(),
                "bitmap dimensions out of range",
            ));
        }
        let width = width as u32;
        let height = height as u32;

        let depth = cursor
            .read_u16::<BigEndian>()
            .map_err(|_| AbrError::InvalidHeader)?;
        if depth != 8 && depth != 16 {
            return Err(entry_err(cursor.position(), "unsupported bitmap depth"));
        }
        let depth = depth as u8;
        let compression = cursor.read_u8().map_err(|_| AbrError::InvalidHeader)?;

        let bpp = (depth as usize) / 8;
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|wh| wh.checked_mul(bpp))
            .filter(|&n| n <= MAX_TIP_DECODED_BYTES)
            .ok_or_else(|| entry_err(cursor.position(), "bitmap byte size out of range"))?;

        if entry_end > cursor.get_ref().len() as u64 {
            return Err(entry_err(
                cursor.position(),
                "samp entry length exceeds buffer",
            ));
        }
        if cursor.position() > entry_end {
            return Err(entry_err(
                cursor.position(),
                "samp entry length shorter than header",
            ));
        }

        let input: &[u8] = cursor.get_ref();
        let pixel_start = cursor.position();
        let pixel_end = match compression {
            0 => {
                let end = pixel_start + expected as u64;
                if end > input.len() as u64 {
                    return Err(AbrError::Decompression("truncated raw data".into()));
                }
                end
            }
            1 => entry_end,
            _ => {
                cursor.set_position(entry_end);
                continue;
            }
        };
        // A raw entry may declare more or fewer bytes than its pixels. GIMP
        // ignores the declared length of a sampled entry, so a short one still
        // reads its pixels and the next entry starts after them.
        cursor.set_position(entry_end.max(pixel_end));
        let pixels = &input[pixel_start as usize..pixel_end as usize];

        let (pixel_data, deferred) = if !defer {
            #[cfg(test)]
            tests::record_decode();
            let data = if compression == 0 {
                pixels.to_vec()
            } else {
                decode_rle(pixels, height as usize, width as usize, bpp, expected)?
            };
            (data, None)
        } else if expected == 0 {
            // Nothing to decode, and `bitmap_geometry` rejects an empty pixel
            // region.
            (Vec::new(), None)
        } else {
            let header = BitmapHeader {
                rect_offset: 0,
                top,
                left,
                bottom,
                right,
                depth,
                compression,
            };
            let tip = DeferredTip {
                entry: rect_start..pixel_end as usize,
                header,
                decoded_len: expected,
            };
            (Vec::new(), Some(tip))
        };

        brushes.push(AbrBrush {
            id: format!("brush_{}", brushes.len()),
            name,
            tip: TipBitmap {
                width,
                height,
                depth,
                data: pixel_data,
            },
            descriptor: BrushDescriptor::default(),
            synthesized: false,
            preset_index: None,
        });
        tips.push(deferred);
    }

    brushes.reverse();
    tips.reverse();
    let pack = AbrPack {
        version,
        brushes,
        preset_count: brush_count as usize,
        raw_desc_block: None,
        computed_presets: Vec::new(),
        patterns: Vec::new(),
        dropped_pattern_count: 0,
        dropped_pattern_details: Vec::new(),
        unreadable_patt_chunk_count: 0,
        unreadable_patt_chunks: Vec::new(),
        dropped_samp_count: 0,
        skipped_preset_count: 0,
        dropped_tip_details: Vec::new(),
        skipped_preset_details: Vec::new(),
        unsupported_tip_count: 0,
        unsupported_tip_presets: Vec::new(),
        desc_parse_error: None,
    };
    Ok(ParsedAbr {
        pack,
        tips,
        dropped_tips: Vec::new(),
        dual_uuids: HashSet::new(),
    })
}

/// Read v2 Unicode brush name: u32 char count (including null) + UTF-16BE + null
fn read_v2_unicode_name(cursor: &mut Cursor<&[u8]>) -> Result<String, AbrError> {
    let char_count = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| AbrError::InvalidHeader)? as usize;
    if char_count == 0 {
        return Ok(String::new());
    }
    if char_count > MAX_NAME_CODE_UNITS {
        return Err(entry_err(cursor.position(), "v2 brush name too long"));
    }
    let mut buf = vec![0u16; char_count];
    for val in buf.iter_mut() {
        *val = cursor
            .read_u16::<BigEndian>()
            .map_err(|_| AbrError::InvalidHeader)?;
    }
    if buf.last() == Some(&0) {
        buf.pop();
    }
    Ok(String::from_utf16_lossy(&buf))
}

const MAX_PLAUSIBLE_MAJOR: u16 = 16;

fn read_header(cursor: &mut Cursor<&[u8]>) -> Result<(AbrVersion, u16), AbrError> {
    let major = cursor
        .read_u16::<BigEndian>()
        .map_err(|_| AbrError::InvalidHeader)?;
    let minor = cursor
        .read_u16::<BigEndian>()
        .map_err(|_| AbrError::InvalidHeader)?;

    let version = match major {
        1 => AbrVersion::V1,
        2 => AbrVersion::V2,
        6 => AbrVersion::V6,
        7 => AbrVersion::V7,
        9 => AbrVersion::V9,
        10 => AbrVersion::V10,
        other if (1..=MAX_PLAUSIBLE_MAJOR).contains(&other) => {
            return Err(AbrError::UnsupportedVersion(other));
        }
        _ => return Err(AbrError::InvalidHeader),
    };

    Ok((version, minor))
}

#[derive(Debug)]
struct Block<'a> {
    block_type: String,
    /// Input offset of `data`.
    start: usize,
    data: &'a [u8],
}

fn read_blocks<'a>(
    cursor: &mut Cursor<&'a [u8]>,
    patterns: PatternMode,
) -> Result<Vec<Block<'a>>, AbrError> {
    let input: &'a [u8] = cursor.get_ref();
    let file_len = input.len() as u64;
    let mut blocks = Vec::new();
    let mut block_count = 0;

    loop {
        let pos = cursor.position();
        if pos + 12 > file_len {
            break;
        }

        let mut sig = [0u8; 4];
        if cursor.read_exact(&mut sig).is_err() {
            break;
        }
        if &sig != b"8BIM" {
            break;
        }

        if block_count >= MAX_RECORDS {
            return Err(AbrError::RecordLimit("blocks"));
        }
        block_count += 1;
        let mut type_buf = [0u8; 4];
        cursor
            .read_exact(&mut type_buf)
            .map_err(|_| AbrError::MalformedBlock {
                offset: pos,
                reason: "truncated block type".into(),
            })?;
        let block_type = String::from_utf8_lossy(&type_buf).to_string();

        let data_len = cursor
            .read_u32::<BigEndian>()
            .map_err(|_| AbrError::MalformedBlock {
                offset: pos,
                reason: "truncated block length".into(),
            })? as u64;

        let data_start = cursor.position();
        if data_start + data_len > file_len {
            return Err(AbrError::MalformedBlock {
                offset: pos,
                reason: format!(
                    "block claims {data_len} bytes but only {} remain",
                    file_len - data_start
                ),
            });
        }

        let omit = patterns == PatternMode::Skip && block_type == "patt";
        if !omit {
            let start = data_start as usize;
            let data = &input[start..start + data_len as usize];
            blocks.push(Block {
                block_type,
                start,
                data,
            });
        }

        // 8BIM blocks are 4-byte aligned: Photoshop writes 0-3 zero padding
        // bytes after an unaligned payload. The clamp only matters for a file
        // that ends on an
        // unaligned block, where the trailing padding is optional.
        let aligned = (data_start + data_len).next_multiple_of(4).min(file_len);
        cursor.set_position(aligned);
    }

    Ok(blocks)
}

struct SampEntry {
    uuid: Option<String>,
    bitmap: TipBitmap,
    /// `Some` only in `TipMode::Deferred`, where `bitmap.data` is empty and
    /// this says where the pixels are.
    deferred: Option<DeferredTip>,
}

#[derive(Debug, Clone)]
struct BitmapHeader {
    rect_offset: usize,
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    depth: u8,
    compression: u8,
}

/// Uuid-relative offset of the bitmap `rect` for the v6/subversion-1 layout.
///
/// The measured v6/subversion-1 entry layout counts offsets from the
/// `u32 entry_length` prefix and puts `rect` at +51; both call sites
/// below hand out a slice that starts at the `$` of the uuid, 4 bytes later.
const V6_SUB1_RECT_OFFSET: usize = 47;

/// Uuid-relative offset of the bitmap `rect2` for the subversion-2 layout.
///
/// The entry layout is selected by `subversion`, not by the major version
/// so this one offset covers v6/sub-2,
/// v7/sub-2, v9/sub-2 and v10/sub-2 alike.
///
/// Same subtraction: the document puts `rect2` at +305.
const SUB2_RECT_OFFSET: usize = 301;

/// Frame a samp block by its `u32 entry_length` prefixes, each body padded to
/// the next 4-byte boundary. Returns `(body_start, body_end)` per entry and
/// whether the chain
/// framed the whole block. `clean == false` means the walk stopped early on a
/// zero length or a body running past the block end; the frames before that
/// point are still returned.
fn frame_samp_entries_by_length(
    data: &[u8],
    remaining: usize,
) -> Result<(Vec<(usize, usize)>, bool), AbrError> {
    let mut entries = Vec::new();
    let mut at = 0usize;
    while at + 4 <= data.len() {
        let len = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        if len == 0 {
            return Ok((entries, false));
        }
        let start = at + 4;
        let Some(end) = start.checked_add(len).filter(|&end| end <= data.len()) else {
            return Ok((entries, false));
        };
        if entries.len() >= remaining {
            return Err(AbrError::RecordLimit("sampled records"));
        }
        entries.push((start, end));
        at = end.next_multiple_of(4);
    }
    Ok((entries, true))
}

/// Split a samp block into entries. The declared lengths frame it whenever the
/// chain walks the whole block and — for a block that contains `$uuid\0`
/// anchors at all — every framed body starts with one. Otherwise the uuid
/// anchor scan recovers what it can.
fn parse_samp_block(
    data: &[u8],
    version: AbrVersion,
    subversion: u16,
    mode: TipMode,
    remaining: usize,
) -> Result<Vec<Option<SampEntry>>, AbrError> {
    let documented = documented_rect_offset(version, subversion);
    let (frames, clean) = frame_samp_entries_by_length(data, remaining)?;
    let every_body_is_anchored = frames
        .iter()
        .all(|&(start, end)| extract_entry_uuid(&data[start..end]).is_some());
    // Valid framing wins over UUID-shaped bytes inside pixels. Only scan for
    // recovery anchors when the length chain cannot identify anchored records.
    if clean && every_body_is_anchored {
        return Ok(parse_samp_block_by_lengths(data, &frames, documented, mode));
    }
    let uuids = find_all_uuid_offsets(data, remaining)?;
    if uuids.is_empty() {
        if !clean {
            eprintln!(
                "warning: samp entry lengths stop framing the block after {} entries",
                frames.len()
            );
        }
        return Ok(parse_samp_block_by_lengths(data, &frames, documented, mode));
    }

    eprintln!(
        "warning: samp entry lengths do not frame the block; falling back to the uuid anchor scan"
    );
    Ok(parse_samp_block_by_uuids(data, &uuids, documented, mode))
}

fn documented_rect_offset(version: AbrVersion, subversion: u16) -> Option<usize> {
    match (version, subversion) {
        (AbrVersion::V6, 1) => Some(V6_SUB1_RECT_OFFSET),
        (AbrVersion::V6, 2) | (AbrVersion::V7, 2) | (AbrVersion::V9, 2) | (AbrVersion::V10, 2) => {
            Some(SUB2_RECT_OFFSET)
        }
        _ => None,
    }
}

fn bitmap_header_at(data: &[u8], rect_offset: usize) -> Option<BitmapHeader> {
    if rect_offset.checked_add(19)? >= data.len() {
        return None;
    }

    let rs = rect_offset;
    let top = i32::from_be_bytes(data[rs..rs + 4].try_into().ok()?);
    let left = i32::from_be_bytes(data[rs + 4..rs + 8].try_into().ok()?);
    let bottom = i32::from_be_bytes(data[rs + 8..rs + 12].try_into().ok()?);
    let right = i32::from_be_bytes(data[rs + 12..rs + 16].try_into().ok()?);

    let depth_val = u16::from_be_bytes([data[rs + 16], data[rs + 17]]);
    if depth_val != 8 && depth_val != 16 {
        return None;
    }
    let compression = data[rs + 18];
    if compression > 2 {
        return None;
    }

    let w = i64::from(right) - i64::from(left);
    let h = i64::from(bottom) - i64::from(top);
    let max = i64::from(MAX_DIMENSION);
    if w <= 0 || h <= 0 || w > max || h > max {
        return None;
    }

    Some(BitmapHeader {
        rect_offset,
        top,
        left,
        bottom,
        right,
        depth: depth_val as u8,
        compression,
    })
}

fn locate_bitmap_header(data: &[u8], documented: Option<usize>) -> Option<BitmapHeader> {
    documented
        .and_then(|offset| bitmap_header_at(data, offset))
        .or_else(|| find_bitmap_header(data))
}

fn parse_samp_block_by_uuids(
    data: &[u8],
    uuids: &[(usize, String)],
    documented: Option<usize>,
    mode: TipMode,
) -> Vec<Option<SampEntry>> {
    let mut entries = Vec::new();

    for (i, (uuid_offset, uuid_str)) in uuids.iter().enumerate() {
        let entry_end = uuids
            .get(i + 1)
            .map_or(data.len(), |(next_offset, _)| *next_offset);
        let search_data = &data[*uuid_offset..entry_end];
        match locate_bitmap_header(search_data, documented) {
            Some(header) => {
                match read_entry_bitmap(search_data, &header, mode, *uuid_offset..entry_end) {
                    Ok((bitmap, deferred)) => {
                        entries.push(Some(SampEntry {
                            uuid: Some(uuid_str.clone()),
                            bitmap,
                            deferred,
                        }));
                    }
                    Err(e) => {
                        eprintln!("warning: bitmap decode failed for {uuid_str}: {e}");
                        entries.push(None);
                    }
                }
            }
            None => {
                eprintln!("warning: no bitmap header found for UUID {uuid_str}");
                entries.push(None);
            }
        }
    }

    entries
}

fn parse_samp_block_by_lengths(
    data: &[u8],
    frames: &[(usize, usize)],
    documented: Option<usize>,
    mode: TipMode,
) -> Vec<Option<SampEntry>> {
    let mut entries = Vec::new();

    for &(start, end) in frames {
        let entry_data = &data[start..end];
        let uuid = extract_entry_uuid(entry_data);

        match locate_bitmap_header(entry_data, documented) {
            Some(header) => match read_entry_bitmap(entry_data, &header, mode, start..end) {
                Ok((bitmap, deferred)) => entries.push(Some(SampEntry {
                    uuid,
                    bitmap,
                    deferred,
                })),
                Err(e) => {
                    eprintln!("warning: bitmap decode failed at offset {start}: {e}");
                    entries.push(None);
                }
            },
            None => {
                eprintln!("warning: no bitmap header found in samp entry at offset {start}");
                entries.push(None);
            }
        }
    }

    entries
}

fn find_bitmap_header(data: &[u8]) -> Option<BitmapHeader> {
    let scan_limit = data.len().min(2048);
    if scan_limit < 19 {
        return None;
    }

    for i in 16..scan_limit.saturating_sub(2) {
        let depth_val = u16::from_be_bytes([data[i], data[i + 1]]);
        let comp = data[i + 2];

        if (depth_val == 8 || depth_val == 16) && comp <= 1 {
            let rs = i - 16;
            let top = i32::from_be_bytes(data[rs..rs + 4].try_into().ok()?);
            let left = i32::from_be_bytes(data[rs + 4..rs + 8].try_into().ok()?);
            let bottom = i32::from_be_bytes(data[rs + 8..rs + 12].try_into().ok()?);
            let right = i32::from_be_bytes(data[rs + 12..rs + 16].try_into().ok()?);

            let w = i64::from(right) - i64::from(left);
            let h = i64::from(bottom) - i64::from(top);

            let max = i64::from(MAX_DIMENSION);
            if w > 0 && h > 0 && w <= max && h <= max {
                return Some(BitmapHeader {
                    rect_offset: rs,
                    top,
                    left,
                    bottom,
                    right,
                    depth: depth_val as u8,
                    compression: comp,
                });
            }
        }
    }

    None
}

struct BitmapGeometry {
    width: u32,
    height: u32,
    pixel_start: usize,
    /// `width * height * bytes_per_pixel`.
    expected: usize,
}

fn bitmap_geometry(data: &[u8], header: &BitmapHeader) -> Result<BitmapGeometry, AbrError> {
    let width = (header.right - header.left) as u32;
    let height = (header.bottom - header.top) as u32;

    let pixel_start = header.rect_offset + 19;
    if pixel_start >= data.len() {
        return Err(entry_err(0, "bitmap header extends past data end"));
    }
    let available = data.len() - pixel_start;

    let bpp = (header.depth as usize) / 8;
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|wh| wh.checked_mul(bpp))
        .filter(|&n| n <= MAX_TIP_DECODED_BYTES)
        .ok_or_else(|| entry_err(0, "bitmap byte size out of range"))?;

    match header.compression {
        0 => {
            if available < expected {
                return Err(entry_err(
                    0,
                    &format!(
                        "pixel data too short: {available} bytes, expected {expected} ({width}x{height}x{}bit)",
                        header.depth
                    ),
                ));
            }
        }
        1 | 2 => {}
        _ => {
            return Err(entry_err(
                0,
                &format!("unknown compression {}", header.compression),
            ))
        }
    }

    Ok(BitmapGeometry {
        width,
        height,
        pixel_start,
        expected,
    })
}

/// Record where a tip's pixels are instead of decoding them. The returned
/// `TipBitmap` carries the real dimensions and NO data — see [`DeferredPack`].
fn defer_bitmap(
    data: &[u8],
    header: &BitmapHeader,
    entry: Range<usize>,
) -> Result<(TipBitmap, DeferredTip), AbrError> {
    let geom = bitmap_geometry(data, header)?;
    Ok((
        TipBitmap {
            width: geom.width,
            height: geom.height,
            depth: header.depth,
            data: Vec::new(),
        },
        DeferredTip {
            entry,
            header: header.clone(),
            decoded_len: geom.expected,
        },
    ))
}

/// `entry` is the samp entry's range inside its block's payload; `data` is that
/// same range, already sliced. A deferred tip shifts it by the block's input
/// offset, which is what `DeferredPack::decode_tip` re-slices.
fn read_entry_bitmap(
    data: &[u8],
    header: &BitmapHeader,
    mode: TipMode,
    entry: Range<usize>,
) -> Result<(TipBitmap, Option<DeferredTip>), AbrError> {
    match mode {
        TipMode::Eager => Ok((decode_bitmap(data, header)?, None)),
        TipMode::Deferred { block_start } => {
            let entry = block_start + entry.start..block_start + entry.end;
            let (bitmap, deferred) = defer_bitmap(data, header, entry)?;
            Ok((bitmap, Some(deferred)))
        }
    }
}

fn decode_bitmap(data: &[u8], header: &BitmapHeader) -> Result<TipBitmap, AbrError> {
    #[cfg(test)]
    tests::record_decode();
    let BitmapGeometry {
        width,
        height,
        pixel_start,
        expected,
    } = bitmap_geometry(data, header)?;

    let pixel_raw = &data[pixel_start..];
    let bpp = (header.depth as usize) / 8;

    let mut pixel_data = match header.compression {
        0 => pixel_raw[..expected].to_vec(),
        1 => decode_rle(pixel_raw, height as usize, width as usize, bpp, expected)?,
        2 => {
            let mut dec = ZlibDecoder::new(pixel_raw).take(expected as u64 + 1);
            let mut buf = Vec::new();
            dec.read_to_end(&mut buf)
                .map_err(|e| AbrError::Decompression(format!("zlib: {e}")))?;
            if buf.len() != expected {
                return Err(AbrError::Decompression(format!(
                    "zlib output has {} bytes, expected {expected}",
                    buf.len()
                )));
            }
            buf
        }
        _ => {
            return Err(entry_err(
                0,
                &format!("unknown compression {}", header.compression),
            ))
        }
    };

    if pixel_data.len() < expected {
        return Err(entry_err(
            0,
            &format!(
                "pixel data too short: {} bytes, expected {} ({width}x{height}x{}bit)",
                pixel_data.len(),
                expected,
                header.depth
            ),
        ));
    }

    pixel_data.truncate(expected);
    Ok(TipBitmap {
        width,
        height,
        depth: header.depth,
        data: pixel_data,
    })
}

fn find_all_uuid_offsets(data: &[u8], remaining: usize) -> Result<Vec<(usize, String)>, AbrError> {
    let mut results = Vec::new();

    if data.len() < 38 {
        return Ok(results);
    }

    for i in 0..data.len().saturating_sub(37) {
        if data[i] == b'$' && is_uuid_at(data, i + 1) && data[i + 37] == 0 {
            if results.len() >= remaining {
                return Err(AbrError::RecordLimit("UUID recovery anchors"));
            }
            let uuid = String::from_utf8_lossy(&data[i + 1..i + 37]).to_string();
            results.push((i, uuid));
        }
    }

    Ok(results)
}

fn is_uuid_at(data: &[u8], start: usize) -> bool {
    if start + 36 > data.len() {
        return false;
    }

    let segment_lengths = [8, 4, 4, 4, 12];
    let mut pos = start;

    for (seg_idx, &seg_len) in segment_lengths.iter().enumerate() {
        for _ in 0..seg_len {
            if pos >= data.len() || !data[pos].is_ascii_hexdigit() {
                return false;
            }
            pos += 1;
        }
        if seg_idx < 4 {
            if pos >= data.len() || data[pos] != b'-' {
                return false;
            }
            pos += 1;
        }
    }

    true
}

fn extract_entry_uuid(data: &[u8]) -> Option<String> {
    if data.len() >= 38 && data[0] == b'$' && is_uuid_at(data, 1) && data[37] == 0 {
        Some(String::from_utf8_lossy(&data[1..37]).to_string())
    } else {
        None
    }
}

pub(crate) fn decode_rle(
    data: &[u8],
    height: usize,
    width: usize,
    bytes_per_pixel: usize,
    expected_size: usize,
) -> Result<Vec<u8>, AbrError> {
    let counts_len = height
        .checked_mul(2)
        .ok_or_else(|| AbrError::Decompression("RLE row count table too large".into()))?;
    let counts = data
        .get(..counts_len)
        .ok_or_else(|| AbrError::Decompression("truncated RLE row byte counts".into()))?;

    let row_width = width * bytes_per_pixel;
    let remaining = data.len() - counts_len;
    let mut result = Vec::with_capacity(expected_size.min(remaining.saturating_mul(128)));

    let mut pos = counts_len;
    for row in 0..height {
        let count = u16::from_be_bytes([counts[2 * row], counts[2 * row + 1]]) as usize;
        let slice = data.get(pos..pos + count).ok_or_else(|| {
            AbrError::Decompression(format!(
                "truncated RLE row {row}: declares {count} bytes, {} remain",
                data.len() - pos
            ))
        })?;
        decode_rle_row(slice, row, row_width, &mut result)?;
        pos += count;
    }

    debug_assert_eq!(result.len(), expected_size);
    Ok(result)
}

/// Decode one PackBits row from exactly `slice`, appending `row_width` bytes.
/// Errors when the slice ends before the row is complete (underrun) or a run
/// would push the row past `row_width` (overrun). Bytes left in the slice
/// after the row is complete are ignored: the declared count, not the
/// content, decides where the next row starts.
fn decode_rle_row(
    slice: &[u8],
    row: usize,
    row_width: usize,
    out: &mut Vec<u8>,
) -> Result<(), AbrError> {
    let mut i = 0usize;
    let mut produced = 0usize;

    while produced < row_width {
        let n = *slice.get(i).ok_or_else(|| {
            AbrError::Decompression(format!(
                "truncated RLE row {row}: slice ended after {produced} of {row_width} bytes"
            ))
        })? as i8;
        i += 1;

        match n {
            0..=127 => {
                let count = n as usize + 1;
                let bytes = slice.get(i..i + count).ok_or_else(|| {
                    AbrError::Decompression(format!(
                        "truncated RLE row {row}: literal run of {count} exceeds the row's declared bytes"
                    ))
                })?;
                if produced + count > row_width {
                    return Err(AbrError::Decompression(format!(
                        "RLE row {row} overruns its width: {} > {row_width}",
                        produced + count
                    )));
                }
                out.extend_from_slice(bytes);
                produced += count;
                i += count;
            }
            -128 => {}
            _ => {
                let count = (1 - n as isize) as usize;
                let val = *slice.get(i).ok_or_else(|| {
                    AbrError::Decompression(format!(
                        "truncated RLE row {row}: repeat run has no value byte"
                    ))
                })?;
                if produced + count > row_width {
                    return Err(AbrError::Decompression(format!(
                        "RLE row {row} overruns its width: {} > {row_width}",
                        produced + count
                    )));
                }
                out.extend(std::iter::repeat_n(val, count));
                produced += count;
                i += 1;
            }
        }
    }

    Ok(())
}

fn entry_err(offset: u64, reason: &str) -> AbrError {
    AbrError::MalformedBlock {
        offset,
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn repeated_dropped_uuids_share_owner_diagnostics() {
        let name = "N".repeat(256 * 1024);
        let mut samples = vec![samp(Some("main"))];
        samples.extend((0..127).map(|_| samp(Some("dual"))));
        let infos = [info_dual(&name, Some("main"), "dual")];
        let paired = pair_brushes(samples, &infos);
        assert_eq!(paired.brushes[0].name, name);
        assert_eq!(paired.dropped_tip_details.len(), 127);
        let shared = &paired.dropped_tip_details[0].owner_preset_names;
        assert_eq!(shared[0], name);
        for tip in &paired.dropped_tip_details[1..] {
            assert_eq!(tip.owner_preset_names.as_ptr(), shared.as_ptr());
        }
    }

    #[test]
    fn record_limits_fail_before_collecting_or_reading_record_payloads() {
        let mut framed = Vec::new();
        for _ in 0..=MAX_RECORDS {
            framed.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0]);
        }
        assert!(matches!(
            frame_samp_entries_by_length(&framed, MAX_RECORDS),
            Err(AbrError::RecordLimit("sampled records"))
        ));
        let mut legacy = Cursor::new(&[][..]);
        assert!(matches!(
            parse_legacy(&mut legacy, (MAX_RECORDS + 1) as u16, AbrVersion::V1, true),
            Err(AbrError::RecordLimit("legacy brushes"))
        ));
        let mut blocks = Vec::new();
        for _ in 0..=MAX_RECORDS {
            blocks.extend_from_slice(b"8BIMpatt\0\0\0\0");
        }
        let mut cursor = Cursor::new(blocks.as_slice());
        assert!(matches!(
            read_blocks(&mut cursor, PatternMode::Skip),
            Err(AbrError::RecordLimit("blocks"))
        ));
    }
    use super::*;
    use brushkit_fixture::{write_bool, write_key};
    use byteorder::WriteBytesExt;
    use std::cell::Cell;

    use crate::pattern::tests::{channel_slot, record_block, record_body, unreadable_chunk};

    thread_local! {
        // Thread-local because cargo runs unit tests on parallel threads.
        static DECODES: Cell<usize> = const { Cell::new(0) };
    }

    /// Called once per tip whose pixels are decoded or copied out.
    pub(super) fn record_decode() {
        DECODES.with(|count| count.set(count.get() + 1));
    }

    /// `f`'s result and the tips it decoded on this thread.
    fn decodes<T>(f: impl FnOnce() -> T) -> (T, usize) {
        DECODES.with(|count| count.set(0));
        let out = f();
        (out, DECODES.with(Cell::get))
    }

    fn tiny_bitmap() -> TipBitmap {
        TipBitmap {
            width: 1,
            height: 1,
            depth: 8,
            data: vec![255u8],
        }
    }

    fn samp(uuid: Option<&str>) -> SampEntry {
        SampEntry {
            uuid: uuid.map(str::to_string),
            bitmap: tiny_bitmap(),
            deferred: None,
        }
    }

    fn info(name: &str, uuid: Option<&str>, scale: Option<f64>) -> BrushDescInfo {
        BrushDescInfo {
            name: name.to_string(),
            sampled_data_uuid: uuid.map(str::to_string),
            dual_brush_uuid: None,
            has_shape_tip: false,
            shape_tip_family: None,
            tip_shape: None,
            descriptor: BrushDescriptor {
                texture_scale_pct: scale,
                ..Default::default()
            },
        }
    }

    fn info_dual(name: &str, uuid: Option<&str>, dual_uuid: &str) -> BrushDescInfo {
        BrushDescInfo {
            name: name.to_string(),
            sampled_data_uuid: uuid.map(str::to_string),
            dual_brush_uuid: Some(dual_uuid.to_string()),
            has_shape_tip: false,
            shape_tip_family: None,
            tip_shape: None,
            descriptor: BrushDescriptor::default(),
        }
    }

    #[test]
    fn unsupported_tip_presets_collect_only_presets_with_neither_tip_nor_geometry() {
        let mut computed = info("Computed", None, None);
        computed.descriptor.computed = Some(crate::ComputedGeometry {
            diameter_px: Some(30.0),
            ..Default::default()
        });
        let mut bristle = info("Bristle", None, None);
        bristle.has_shape_tip = true;
        bristle.shape_tip_family = Some(crate::ShapeTipFamily::Bristle);
        let infos = vec![
            info("Resolvable", Some("uuid-x"), None),
            info("Dangling", Some("uuid-missing"), None),
            computed,
            bristle,
        ];

        let collected = collect_unsupported_tip_presets(&infos);
        assert_eq!(
            collected.len(),
            1,
            "only the tip-less, geometry-less preset"
        );
        assert_eq!(collected[0].name, "Bristle");
        assert_eq!(collected[0].preset_index, 3, "ordinal in desc order");
        assert!(collected[0].has_shape_tip);
        assert_eq!(
            collected[0].shape_tip_family,
            Some(crate::ShapeTipFamily::Bristle)
        );
    }

    #[test]
    fn unsupported_tip_preset_without_shape_tip_is_still_collected() {
        let infos = vec![info("Empty", None, None)];
        let collected = collect_unsupported_tip_presets(&infos);
        assert_eq!(collected.len(), 1);
        assert!(!collected[0].has_shape_tip);
        assert!(collected[0].shape_tip_family.is_none());
    }

    #[test]
    fn duplicate_main_uuid_emits_both_presets() {
        let bitmaps = vec![samp(Some("uuid-x"))];
        let infos = vec![
            info("P1", Some("uuid-x"), Some(43.0)),
            info("P2", Some("uuid-x"), Some(47.0)),
        ];
        let PairedBrushes {
            brushes,
            dropped_samp_count,
            skipped_preset_count,
            ..
        } = pair_brushes(bitmaps, &infos);
        assert_eq!(brushes.len(), 2);
        assert_eq!(brushes[0].name, "P1");
        assert_eq!(brushes[0].descriptor.texture_scale_pct, Some(43.0));
        assert_eq!(brushes[0].id, "uuid-x");
        assert_eq!(brushes[1].name, "P2");
        assert_eq!(brushes[1].descriptor.texture_scale_pct, Some(47.0));
        assert_eq!(brushes[1].id, "uuid-x-2");
        assert_eq!(dropped_samp_count, 0);
        assert_eq!(skipped_preset_count, 0);
    }

    #[test]
    fn dual_only_samp_not_emitted() {
        let bitmaps = vec![samp(Some("uuid-a")), samp(Some("uuid-b"))];
        let infos = vec![info("A", Some("uuid-a"), Some(40.0))];
        let PairedBrushes { brushes, .. } = pair_brushes(bitmaps, &infos);
        assert_eq!(brushes.len(), 1);
        assert_eq!(brushes[0].id, "uuid-a");
        assert_eq!(brushes[0].descriptor.texture_scale_pct, Some(40.0));
    }

    #[test]
    fn no_resolvable_uuids_falls_back_to_index_pairing() {
        let bitmaps = vec![samp(None), samp(None)];
        let infos = vec![info("First", None, None), info("Second", None, None)];
        let PairedBrushes { brushes, .. } = pair_brushes(bitmaps, &infos);
        assert_eq!(brushes.len(), 2);
        assert_eq!(brushes[0].name, "Second");
        assert_eq!(brushes[1].name, "First");
    }

    #[test]
    fn presets_first_preset_index_has_gap_for_skipped() {
        let bitmaps = vec![samp(Some("uuid-a")), samp(Some("uuid-c"))];
        let infos = vec![
            info("A", Some("uuid-a"), None),
            info("B", Some("uuid-y-dangling"), None),
            info("C", Some("uuid-c"), None),
        ];
        let PairedBrushes { brushes, .. } = pair_brushes(bitmaps, &infos);
        assert_eq!(brushes.len(), 2);
        assert_eq!(brushes[0].name, "A");
        assert_eq!(brushes[0].preset_index, Some(0));
        assert_eq!(brushes[1].name, "C");
        assert_eq!(brushes[1].preset_index, Some(2));
    }

    #[test]
    fn fallback_preset_index_is_positional_post_reverse() {
        let bitmaps = vec![samp(None), samp(None)];
        let infos = vec![info("First", None, None), info("Second", None, None)];
        let PairedBrushes { brushes, .. } = pair_brushes(bitmaps, &infos);
        assert_eq!(brushes[0].name, "Second");
        assert_eq!(brushes[0].preset_index, Some(1));
        assert_eq!(brushes[1].name, "First");
        assert_eq!(brushes[1].preset_index, Some(0));
    }

    #[test]
    fn preset_index_aligns_with_dump_ordinals() {
        use crate::dump::{dump_descriptors, DumpValue};
        let samp = build_v10_entry(8, 8, 0xDD);
        let desc = build_mixed_desc_block();
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(10).unwrap();
        d.write_u16::<BigEndian>(2).unwrap();
        d.extend_from_slice(b"8BIM");
        d.extend_from_slice(b"samp");
        d.write_u32::<BigEndian>(samp.len() as u32).unwrap();
        d.extend_from_slice(&samp);
        d.resize(d.len().next_multiple_of(4), 0);
        d.extend_from_slice(b"8BIM");
        d.extend_from_slice(b"desc");
        d.write_u32::<BigEndian>(desc.len() as u32).unwrap();
        d.extend_from_slice(&desc);

        let pack = parse_abr(&d).unwrap();
        let dump = dump_descriptors(pack.raw_desc_block.as_ref().unwrap());

        let name_at = |pi: usize| -> String {
            let preset = dump.presets.iter().find(|p| p.index == pi).unwrap();
            preset
                .items
                .iter()
                .find_map(|it| match &it.value {
                    DumpValue::Text(s) if it.key == "Nm  " => Some(s.clone()),
                    _ => None,
                })
                .unwrap()
        };

        let b = &pack.brushes[0];
        let pi = b.preset_index.expect("sampled brush has preset_index");
        assert_eq!(name_at(pi), b.name);
        let c = &pack.computed_presets[0];
        let cpi = c.preset_index.expect("computed preset has preset_index");
        assert_eq!(name_at(cpi), c.name);
    }

    #[test]
    fn unresolvable_preset_skipped() {
        let bitmaps = vec![samp(Some("uuid-x"))];
        let infos = vec![
            info("X", Some("uuid-x"), Some(50.0)),
            info("Y", Some("uuid-y-dangling"), Some(99.0)),
        ];
        let PairedBrushes { brushes, .. } = pair_brushes(bitmaps, &infos);
        assert_eq!(brushes.len(), 1);
        assert_eq!(brushes[0].name, "X");
        assert_eq!(brushes[0].id, "uuid-x");
    }

    #[test]
    fn counts_dropped_dual_only_samps() {
        let bitmaps = vec![samp(Some("uuid-a")), samp(Some("uuid-b"))];
        let infos = vec![info("A", Some("uuid-a"), None)];
        let PairedBrushes {
            dropped_samp_count,
            skipped_preset_count,
            ..
        } = pair_brushes(bitmaps, &infos);
        assert_eq!(dropped_samp_count, 1);
        assert_eq!(skipped_preset_count, 0);
    }

    #[test]
    fn counts_dangling_preset() {
        let bitmaps = vec![samp(Some("uuid-x"))];
        let infos = vec![
            info("X", Some("uuid-x"), None),
            info("Y", Some("uuid-y-dangling"), None),
        ];
        let PairedBrushes {
            dropped_samp_count,
            skipped_preset_count,
            ..
        } = pair_brushes(bitmaps, &infos);
        assert_eq!(dropped_samp_count, 0);
        assert_eq!(skipped_preset_count, 1);
    }

    #[test]
    fn counts_zero_on_fallback() {
        let bitmaps = vec![samp(None), samp(None)];
        let infos = vec![info("First", None, None), info("Second", None, None)];
        let PairedBrushes {
            dropped_samp_count,
            skipped_preset_count,
            ..
        } = pair_brushes(bitmaps, &infos);
        assert_eq!(dropped_samp_count, 0);
        assert_eq!(skipped_preset_count, 0);
    }

    #[test]
    fn attributes_dropped_tip_to_dual_owner() {
        let bitmaps = vec![samp(Some("uuid-a")), samp(Some("uuid-b"))];
        let infos = vec![info_dual("P1", Some("uuid-a"), "uuid-b")];
        let PairedBrushes {
            dropped_tip_details,
            dropped_samp_count,
            skipped_preset_count,
            ..
        } = pair_brushes(bitmaps, &infos);
        assert_eq!(dropped_samp_count, 1);
        assert_eq!(skipped_preset_count, 0);
        assert_eq!(dropped_tip_details.len(), 1);
        assert_eq!(dropped_tip_details[0].uuid.as_deref(), Some("uuid-b"));
        assert_eq!(dropped_tip_details[0].owner_preset_names.as_ref(), ["P1"]);
    }

    #[test]
    fn dual_ref_to_emitted_main_tip_not_listed() {
        let bitmaps = vec![samp(Some("uuid-a")), samp(Some("uuid-b"))];
        let infos = vec![
            info_dual("P1", Some("uuid-a"), "uuid-b"),
            info("P2", Some("uuid-b"), None),
        ];
        let PairedBrushes {
            dropped_tip_details,
            dropped_samp_count,
            skipped_preset_count,
            ..
        } = pair_brushes(bitmaps, &infos);
        assert_eq!(dropped_samp_count, 0);
        assert_eq!(skipped_preset_count, 0);
        assert!(dropped_tip_details.is_empty());
    }

    #[test]
    fn dangling_preset_detail_carries_name_and_uuid() {
        let bitmaps = vec![samp(Some("uuid-x"))];
        let infos = vec![
            info("X", Some("uuid-x"), None),
            info("Y", Some("uuid-y-dangling"), None),
        ];
        let PairedBrushes {
            skipped_preset_details,
            ..
        } = pair_brushes(bitmaps, &infos);
        assert_eq!(skipped_preset_details.len(), 1);
        assert_eq!(skipped_preset_details[0].name, "Y");
        assert_eq!(skipped_preset_details[0].uuid, "uuid-y-dangling");
    }

    #[test]
    fn unreferenced_tip_without_owner_has_empty_owners() {
        let bitmaps = vec![samp(Some("uuid-a")), samp(Some("uuid-b"))];
        let infos = vec![info("P1", Some("uuid-a"), None)];
        let PairedBrushes {
            dropped_tip_details,
            ..
        } = pair_brushes(bitmaps, &infos);
        assert_eq!(dropped_tip_details.len(), 1);
        assert_eq!(dropped_tip_details[0].uuid.as_deref(), Some("uuid-b"));
        assert!(dropped_tip_details[0].owner_preset_names.is_empty());
    }

    #[test]
    fn corrupt_desc_block_sets_diagnostic_and_falls_back() {
        let samp = build_v10_entry(8, 8, 0xDD);
        let mut desc = Vec::new();
        desc.write_u32::<BigEndian>(16).unwrap();

        let mut d = Vec::new();
        d.write_u16::<BigEndian>(10).unwrap();
        d.write_u16::<BigEndian>(2).unwrap();
        d.extend_from_slice(b"8BIM");
        d.extend_from_slice(b"samp");
        d.write_u32::<BigEndian>(samp.len() as u32).unwrap();
        d.extend_from_slice(&samp);
        d.resize(d.len().next_multiple_of(4), 0);
        d.extend_from_slice(b"8BIM");
        d.extend_from_slice(b"desc");
        d.write_u32::<BigEndian>(desc.len() as u32).unwrap();
        d.extend_from_slice(&desc);

        let pack = parse_abr(&d).expect("parse_abr returns Ok despite corrupt desc");
        assert!(
            pack.desc_parse_error.is_some(),
            "corrupt desc block sets the diagnostic"
        );
        assert!(
            !pack.brushes.is_empty(),
            "fallback pairing still emits the samp bitmap"
        );
    }

    #[test]
    fn descriptor_defaults_when_no_desc_block() {
        let entry = build_simple_entry(4, 4, 8, 0xAB);
        let mut block = Vec::new();
        block.write_u32::<BigEndian>(entry.len() as u32).unwrap();
        block.extend_from_slice(&entry);

        let mut d = Vec::new();
        d.write_u16::<BigEndian>(6).unwrap();
        d.write_u16::<BigEndian>(1).unwrap();
        d.extend_from_slice(b"8BIM");
        d.extend_from_slice(b"samp");
        d.write_u32::<BigEndian>(block.len() as u32).unwrap();
        d.extend_from_slice(&block);

        let pack = parse_abr(&d).unwrap();
        assert_eq!(pack.brushes.len(), 1);
        assert_eq!(pack.brushes[0].descriptor.spacing_pct, None);
    }

    #[test]
    fn preset_count_v2_counts_skipped_computed_entry() {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(2).unwrap();
        d.write_u16::<BigEndian>(1).unwrap();
        d.write_u16::<BigEndian>(1).unwrap();
        d.write_u32::<BigEndian>(4).unwrap();
        d.extend_from_slice(&[0u8; 4]);
        let pack = parse_abr(&d).unwrap();
        assert!(pack.brushes.is_empty());
        assert_eq!(pack.preset_count, 1);
        assert!(
            pack.desc_parse_error.is_none(),
            "v2 files carry no desc block"
        );
    }

    const MIXED_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";

    fn desc_unit(buf: &mut Vec<u8>, key: &[u8; 4], unit: &[u8; 4], val: f64) {
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(key);
        buf.extend_from_slice(b"UntF");
        buf.extend_from_slice(unit);
        buf.write_f64::<BigEndian>(val).unwrap();
    }

    fn desc_name(buf: &mut Vec<u8>, name: &str) {
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Nm  ");
        buf.extend_from_slice(b"TEXT");
        let u: Vec<u16> = name.encode_utf16().collect();
        buf.write_u32::<BigEndian>(u.len() as u32 + 1).unwrap();
        for c in &u {
            buf.write_u16::<BigEndian>(*c).unwrap();
        }
        buf.write_u16::<BigEndian>(0).unwrap();
    }

    fn desc_preset_header(buf: &mut Vec<u8>, item_count: u32) {
        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(11).unwrap();
        buf.extend_from_slice(b"brushPreset");
        buf.write_u32::<BigEndian>(item_count).unwrap();
    }

    fn build_mixed_desc_block() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.write_u32::<BigEndian>(16).unwrap();
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"null");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(2).unwrap();

        desc_preset_header(&mut buf, 2);
        desc_name(&mut buf, "Sampled");
        buf.write_u32::<BigEndian>(11).unwrap();
        buf.extend_from_slice(b"sampledData");
        buf.extend_from_slice(b"TEXT");
        let u: Vec<u16> = MIXED_UUID.encode_utf16().collect();
        buf.write_u32::<BigEndian>(u.len() as u32 + 1).unwrap();
        for c in &u {
            buf.write_u16::<BigEndian>(*c).unwrap();
        }
        buf.write_u16::<BigEndian>(0).unwrap();

        desc_preset_header(&mut buf, 2);
        desc_name(&mut buf, "Computed");
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(13).unwrap();
        buf.extend_from_slice(b"computedBrush");
        buf.write_u32::<BigEndian>(5).unwrap();
        desc_unit(&mut buf, b"Dmtr", b"#Pxl", 30.0);
        desc_unit(&mut buf, b"Hrdn", b"#Prc", 80.0);
        desc_unit(&mut buf, b"Angl", b"#Ang", 45.0);
        desc_unit(&mut buf, b"Rndn", b"#Prc", 60.0);
        desc_unit(&mut buf, b"Spcn", b"#Prc", 25.0);

        buf
    }

    #[test]
    fn mixed_pack_splits_sampled_and_computed() {
        let samp = build_v10_entry(8, 8, 0xDD);
        let desc = build_mixed_desc_block();

        let mut d = Vec::new();
        d.write_u16::<BigEndian>(10).unwrap();
        d.write_u16::<BigEndian>(2).unwrap();
        d.extend_from_slice(b"8BIM");
        d.extend_from_slice(b"samp");
        d.write_u32::<BigEndian>(samp.len() as u32).unwrap();
        d.extend_from_slice(&samp);
        d.resize(d.len().next_multiple_of(4), 0);
        d.extend_from_slice(b"8BIM");
        d.extend_from_slice(b"desc");
        d.write_u32::<BigEndian>(desc.len() as u32).unwrap();
        d.extend_from_slice(&desc);

        let pack = parse_abr(&d).unwrap();

        assert_eq!(pack.preset_count, 2);
        assert_eq!(pack.brushes.len(), 1);
        assert_eq!(pack.brushes[0].name, "Sampled");
        assert!(!pack.brushes[0].synthesized);
        assert!(pack.brushes[0].descriptor.computed.is_none());

        assert_eq!(pack.computed_presets.len(), 1);
        let cp = &pack.computed_presets[0];
        assert_eq!(cp.name, "Computed");
        assert_eq!(cp.descriptor.spacing_pct, Some(25.0));
        assert_eq!(
            cp.descriptor.computed,
            Some(crate::ComputedGeometry {
                diameter_px: Some(30.0),
                hardness_pct: Some(80.0),
                angle_deg: Some(45.0),
                roundness_pct: Some(60.0),
            })
        );
    }

    #[test]
    fn test_read_header_v6() {
        let data: [u8; 4] = [0, 6, 0, 1];
        let mut c = Cursor::new(data.as_slice());
        let (v, s) = read_header(&mut c).unwrap();
        assert_eq!(v, AbrVersion::V6);
        assert_eq!(s, 1);
    }

    #[test]
    fn test_read_header_v10() {
        let data: [u8; 4] = [0, 10, 0, 2];
        let mut c = Cursor::new(data.as_slice());
        let (v, _) = read_header(&mut c).unwrap();
        assert_eq!(v, AbrVersion::V10);
    }

    #[test]
    fn test_read_header_invalid() {
        let data: [u8; 2] = [0, 6];
        let mut c = Cursor::new(data.as_slice());
        assert!(read_header(&mut c).is_err());
    }

    #[test]
    fn test_parse_abr_v1_empty() {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(1).unwrap();
        d.write_u16::<BigEndian>(0).unwrap();
        let pack = parse_abr(&d).unwrap();
        assert_eq!(pack.version, AbrVersion::V1);
        assert!(pack.brushes.is_empty());
        assert_eq!(pack.preset_count, 0);
    }

    #[test]
    fn test_parse_abr_v1_sampled_entry_has_no_name_field() {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(1).unwrap();
        d.write_u16::<BigEndian>(2).unwrap();

        d.write_u16::<BigEndian>(1).unwrap();
        d.write_u32::<BigEndian>(3).unwrap();
        d.extend_from_slice(&[0xAA, 0xBB, 0xCC]);

        d.write_u16::<BigEndian>(2).unwrap();
        d.write_u32::<BigEndian>(36).unwrap();
        d.write_u32::<BigEndian>(16).unwrap();
        d.write_u16::<BigEndian>(25).unwrap();
        d.push(1);
        for v in [0i16, 0, 1, 2] {
            d.write_i16::<BigEndian>(v).unwrap();
        }
        for v in [0i32, 0, 1, 2] {
            d.write_i32::<BigEndian>(v).unwrap();
        }
        d.write_u16::<BigEndian>(8).unwrap();
        d.push(0);
        d.extend_from_slice(&[0x10, 0x20]);

        let pack = parse_abr(&d).unwrap();
        assert_eq!(pack.version, AbrVersion::V1);
        assert_eq!(pack.preset_count, 2);
        assert_eq!(pack.brushes.len(), 1);
        let b = &pack.brushes[0];
        assert_eq!(b.id, "brush_0");
        assert_eq!(b.name, "");
        assert_eq!((b.tip.width, b.tip.height, b.tip.depth), (2, 1, 8));
        assert_eq!(b.tip.data, vec![0x10, 0x20]);
        assert_eq!(b.preset_index, None);
        assert!(!b.synthesized);
    }

    #[test]
    fn test_parse_abr_v9_reaches_block_path() {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(9).unwrap();
        d.write_u16::<BigEndian>(2).unwrap();
        let pack = parse_abr(&d).unwrap();
        assert_eq!(pack.version, AbrVersion::V9);
        assert!(pack.brushes.is_empty());
    }

    #[test]
    fn test_parse_abr_v7_reaches_block_path() {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(7).unwrap();
        d.write_u16::<BigEndian>(2).unwrap();
        let pack = parse_abr(&d).unwrap();
        assert_eq!(pack.version, AbrVersion::V7);
        assert!(pack.brushes.is_empty());
    }

    #[test]
    fn test_documented_rect_offset_v7_follows_sub2() {
        assert_eq!(
            documented_rect_offset(AbrVersion::V7, 2),
            Some(SUB2_RECT_OFFSET)
        );
        assert_eq!(documented_rect_offset(AbrVersion::V7, 1), None);
    }

    #[test]
    fn test_parse_abr_unknown_major_is_unsupported() {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(8).unwrap();
        d.write_u16::<BigEndian>(2).unwrap();
        let err = parse_abr(&d).unwrap_err();
        assert!(
            matches!(err, AbrError::UnsupportedVersion(8)),
            "expected UnsupportedVersion(8), got {err:?}"
        );
        let msg = err.to_string();
        assert!(msg.contains('8'), "message must name the version: {msg}");
        assert!(
            !msg.contains("invalid"),
            "a version issue must not read as a damaged file: {msg}"
        );
    }

    #[test]
    fn test_parse_abr_truncated_is_invalid_header() {
        let err = parse_abr(&[0, 9]).unwrap_err();
        assert!(
            matches!(err, AbrError::InvalidHeader),
            "expected InvalidHeader, got {err:?}"
        );
        assert!(
            err.to_string().contains("header"),
            "message must point at the header: {err}"
        );
    }

    #[test]
    fn test_parse_abr_foreign_signature_is_invalid_header() {
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR";
        let err = parse_abr(png).unwrap_err();
        assert!(
            matches!(err, AbrError::InvalidHeader),
            "expected InvalidHeader, got {err:?}"
        );
        let msg = err.to_string();
        assert!(
            !msg.contains("35152"),
            "a foreign signature must not be reported as a version: {msg}"
        );
    }

    #[test]
    fn test_parse_abr_v2_empty() {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(2).unwrap();
        d.write_u16::<BigEndian>(0).unwrap();
        let pack = parse_abr(&d).unwrap();
        assert_eq!(pack.version, AbrVersion::V2);
        assert!(pack.brushes.is_empty());
    }

    #[test]
    fn test_parse_abr_empty_v6() {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(6).unwrap();
        d.write_u16::<BigEndian>(1).unwrap();
        let pack = parse_abr(&d).unwrap();
        assert_eq!(pack.version, AbrVersion::V6);
        assert!(pack.brushes.is_empty());
    }

    #[test]
    fn test_read_blocks_basic() {
        let mut d = Vec::new();
        d.extend_from_slice(b"8BIM");
        d.extend_from_slice(b"test");
        d.write_u32::<BigEndian>(5).unwrap();
        d.extend_from_slice(b"hello");
        let mut c = Cursor::new(d.as_slice());
        let blocks = read_blocks(&mut c, PatternMode::Read).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block_type, "test");
    }

    #[test]
    fn read_blocks_skips_padding_to_four_bytes_between_blocks() {
        let mut d = Vec::new();
        d.extend_from_slice(b"8BIM");
        d.extend_from_slice(b"desc");
        d.write_u32::<BigEndian>(5).unwrap();
        d.extend_from_slice(b"hello");
        d.extend_from_slice(&[0u8; 3]); // pad 17 -> 20
        d.extend_from_slice(b"8BIM");
        d.extend_from_slice(b"samp");
        d.write_u32::<BigEndian>(4).unwrap();
        d.extend_from_slice(b"data");
        let mut c = Cursor::new(d.as_slice());
        let blocks = read_blocks(&mut c, PatternMode::Read).unwrap();
        let types: Vec<&str> = blocks.iter().map(|b| b.block_type.as_str()).collect();
        assert_eq!(types, ["desc", "samp"]);
        assert_eq!(blocks[1].data, b"data");
    }

    #[test]
    fn read_blocks_accepts_an_unaligned_final_block_without_padding() {
        let mut d = Vec::new();
        d.extend_from_slice(b"8BIM");
        d.extend_from_slice(b"desc");
        d.write_u32::<BigEndian>(5).unwrap();
        d.extend_from_slice(b"hello");
        let mut c = Cursor::new(d.as_slice());
        let blocks = read_blocks(&mut c, PatternMode::Read).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block_type, "desc");
        assert_eq!(blocks[0].data, b"hello");
    }

    fn push_block(file: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
        file.extend_from_slice(b"8BIM");
        file.extend_from_slice(kind);
        file.write_u32::<BigEndian>(payload.len() as u32).unwrap();
        file.extend_from_slice(payload);
        file.resize(file.len().next_multiple_of(4), 0);
    }

    #[test]
    fn read_blocks_in_skip_mode_omits_patt_and_keeps_the_walk() {
        let mut d = Vec::new();
        push_block(&mut d, b"patt", b"hello");
        push_block(&mut d, b"samp", b"data");
        d.extend_from_slice(b"8BIM");
        d.extend_from_slice(b"patt");
        d.write_u32::<BigEndian>(3).unwrap();
        d.extend_from_slice(b"end");

        let mut c = Cursor::new(d.as_slice());
        let read = read_blocks(&mut c, PatternMode::Read).unwrap();
        let types: Vec<&str> = read.iter().map(|b| b.block_type.as_str()).collect();
        assert_eq!(types, ["patt", "samp", "patt"]);

        let mut c = Cursor::new(d.as_slice());
        let skipped = read_blocks(&mut c, PatternMode::Skip).unwrap();
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].block_type, "samp");
        assert_eq!(skipped[0].data, b"data");
    }

    fn gray_record(uuid: &str, pixels: [u8; 4]) -> Vec<u8> {
        let channel = channel_slot(2, 2, 0, &pixels);
        record_body(1, (2, 2), "gray", uuid, None, &[channel], 23)
    }

    fn v10_header() -> Vec<u8> {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(10).unwrap();
        d.write_u16::<BigEndian>(2).unwrap();
        d
    }

    fn pack_with_every_pattern_outcome() -> Vec<u8> {
        let bad_ch = channel_slot(2, 2, 0, &[10, 20, 30, 40]);
        let bad = record_body(4, (2, 2), "unsupported", "uuid-mode4", None, &[bad_ch], 23);
        let patt = record_block(&[
            gray_record("uuid-good", [1, 2, 3, 4]),
            unreadable_chunk(),
            bad,
        ]);

        let mut d = v10_header();
        push_block(&mut d, b"samp", &build_v10_entry(8, 8, 0xDD));
        push_block(&mut d, b"patt", &patt);
        push_block(&mut d, b"desc", &build_mixed_desc_block());
        d
    }

    #[test]
    fn without_patterns_leaves_every_pattern_field_empty_and_the_rest_intact() {
        let d = pack_with_every_pattern_outcome();

        let full = parse_abr(&d).unwrap();
        assert_eq!(full.patterns.len(), 1);
        assert_eq!(full.patterns[0].id, "uuid-good");
        assert_eq!(full.dropped_pattern_count, 1);
        assert_eq!(full.dropped_pattern_details.len(), 1);
        assert_eq!(full.unreadable_patt_chunk_count, 1);
        assert_eq!(full.unreadable_patt_chunks.len(), 1);

        let deferred = parse_abr_deferred(&d).unwrap();
        assert_eq!(deferred.pack.patterns.len(), 1);
        assert_eq!(deferred.pack.dropped_pattern_count, 1);
        assert_eq!(deferred.pack.unreadable_patt_chunk_count, 1);

        let skipped = parse_abr_deferred_without_patterns(&d).unwrap();
        let pack = &skipped.pack;
        assert!(pack.patterns.is_empty());
        assert_eq!(pack.dropped_pattern_count, 0);
        assert!(pack.dropped_pattern_details.is_empty());
        assert_eq!(pack.unreadable_patt_chunk_count, 0);
        assert!(pack.unreadable_patt_chunks.is_empty());

        assert_eq!(pack.version, full.version);
        assert_eq!(pack.preset_count, full.preset_count);
        assert_eq!(pack.raw_desc_block, full.raw_desc_block);
        assert_eq!(pack.computed_presets.len(), full.computed_presets.len());
        assert_eq!(pack.desc_parse_error, full.desc_parse_error);
        assert_eq!(pack.brushes.len(), 1);
        assert_eq!(pack.brushes.len(), full.brushes.len());
        for (i, brush) in full.brushes.iter().enumerate() {
            assert_eq!(pack.brushes[i].id, brush.id);
            assert_eq!(pack.brushes[i].name, brush.name);
            assert!(skipped.is_deferred(i));
            let tip = skipped.decode_tip(i).unwrap();
            assert_eq!(
                (tip.width, tip.height, tip.depth),
                (brush.tip.width, brush.tip.height, brush.tip.depth)
            );
            assert_eq!(tip.data, brush.tip.data);
            assert_eq!(tip.data, deferred.decode_tip(i).unwrap().data);
        }
    }

    #[test]
    fn without_patterns_still_rejects_a_patt_block_that_overruns_the_file() {
        for declared in [u32::MAX, 5] {
            let mut d = v10_header();
            d.extend_from_slice(b"8BIMpatt");
            d.write_u32::<BigEndian>(declared).unwrap();
            d.extend_from_slice(b"1234");

            assert!(
                matches!(parse_abr(&d), Err(AbrError::MalformedBlock { .. })),
                "full parse must reject a patt block claiming {declared} bytes"
            );
            assert!(
                matches!(
                    parse_abr_deferred_without_patterns(&d),
                    Err(AbrError::MalformedBlock { .. })
                ),
                "skipping parse must reject a patt block claiming {declared} bytes"
            );
        }
    }

    #[test]
    fn without_patterns_walks_unaligned_patt_blocks_to_the_samp_and_desc_behind_them() {
        let mut d = v10_header();
        let mut first = record_block(&[gray_record("uuid-first", [1, 2, 3, 4])]);
        first.push(0);
        assert_eq!(first.len() % 4, 1);
        push_block(&mut d, b"patt", &first);
        let mut second = record_block(&[gray_record("uuid-second", [5, 6, 7, 8])]);
        second.extend_from_slice(&[0, 0, 0]);
        assert_eq!(second.len() % 4, 3);
        push_block(&mut d, b"patt", &second);
        push_block(&mut d, b"samp", &build_v10_entry(8, 8, 0xDD));
        push_block(&mut d, b"desc", &build_mixed_desc_block());
        let mut last = record_block(&[gray_record("uuid-last", [9, 10, 11, 12])]);
        last.extend_from_slice(&[0, 0]);
        d.extend_from_slice(b"8BIMpatt");
        d.write_u32::<BigEndian>(last.len() as u32).unwrap();
        d.extend_from_slice(&last);
        assert_eq!(
            d.len() % 4,
            2,
            "the final block ends unaligned and unpadded"
        );

        let full = parse_abr(&d).unwrap();
        let ids: Vec<&str> = full.patterns.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["uuid-first", "uuid-second", "uuid-last"]);
        assert_eq!(full.brushes.len(), 1);

        let skipped = parse_abr_deferred_without_patterns(&d).unwrap();
        assert!(skipped.pack.patterns.is_empty());
        assert_eq!(skipped.pack.preset_count, 2);
        assert_eq!(skipped.pack.computed_presets.len(), 1);
        assert_eq!(skipped.pack.brushes.len(), 1);
        assert_eq!(skipped.pack.brushes[0].name, "Sampled");
        assert_eq!(
            skipped.decode_tip(0).unwrap().data,
            full.brushes[0].tip.data
        );
    }

    #[test]
    fn deferred_tips_decode_from_their_own_samp_block_after_an_unaligned_one() {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(6).unwrap();
        d.write_u16::<BigEndian>(1).unwrap();
        for (entry, misalignment) in [
            (build_simple_entry(3, 2, 8, 0x11), 1),
            (build_simple_entry(3, 3, 8, 0x22), 0),
        ] {
            let mut payload = Vec::new();
            payload.write_u32::<BigEndian>(entry.len() as u32).unwrap();
            payload.extend_from_slice(&entry);
            assert_eq!(payload.len() % 4, misalignment);
            push_block(&mut d, b"samp", &payload);
        }

        let eager = parse_abr(&d).unwrap();
        assert_eq!(eager.brushes.len(), 2);
        for deferred in [
            parse_abr_deferred(&d).unwrap(),
            parse_abr_all_deferred_without_patterns(&d).unwrap(),
        ] {
            assert!(deferred.is_deferred(0) && deferred.is_deferred(1));
            assert_tips_match_eager(&deferred, &eager);
        }
    }

    #[test]
    fn raw_desc_block_holds_the_desc_payload() {
        let desc = build_mixed_desc_block();
        let mut d = v10_header();
        push_block(&mut d, b"samp", &build_v10_entry(8, 8, 0xDD));
        push_block(&mut d, b"desc", &desc);
        let pack = parse_abr_all_deferred_without_patterns(&d).unwrap().pack;
        assert_eq!(pack.raw_desc_block, Some(desc));
    }

    fn build_simple_entry(w: u32, h: u32, depth: u8, val: u8) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.write_i32::<BigEndian>(0).unwrap();
        buf.write_i32::<BigEndian>(0).unwrap();
        buf.write_i32::<BigEndian>(h as i32).unwrap();
        buf.write_i32::<BigEndian>(w as i32).unwrap();
        buf.write_u16::<BigEndian>(depth as u16).unwrap();
        buf.push(0);
        let bpp = (depth as usize) / 8;
        buf.extend(std::iter::repeat_n(val, (w as usize) * (h as usize) * bpp));
        buf
    }

    fn build_v10_entry(w: u32, h: u32, val: u8) -> Vec<u8> {
        build_v10_entry_with_uuid("a1b2c3d4-e5f6-7890-abcd-ef1234567890", w, h, val)
    }

    fn build_v10_entry_with_uuid(uuid: &str, w: u32, h: u32, val: u8) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(b'$');
        buf.extend_from_slice(uuid.as_bytes());
        buf.push(0);
        buf.extend(std::iter::repeat_n(0u8, 200));
        buf.write_i32::<BigEndian>(0).unwrap();
        buf.write_i32::<BigEndian>(0).unwrap();
        buf.write_i32::<BigEndian>(h as i32).unwrap();
        buf.write_i32::<BigEndian>(w as i32).unwrap();
        buf.write_u16::<BigEndian>(8).unwrap();
        buf.push(0);
        buf.extend(std::iter::repeat_n(val, (w as usize) * (h as usize)));
        buf
    }

    fn corpus_file(relative: &str) -> Option<std::path::PathBuf> {
        let root = std::path::PathBuf::from(std::env::var_os("BRUSHKIT_CORPUS_DIR")?);
        let path = root.join(relative);
        path.is_file().then_some(path)
    }

    #[test]
    fn corpus_e07_block_walk_reaches_the_trailing_phry() {
        let relative = "ps-27.8.0/e07-scatter.abr";
        let Some(path) = corpus_file(relative) else {
            println!("skip: {relative} absent (set BRUSHKIT_CORPUS_DIR)");
            return;
        };
        let bytes = std::fs::read(&path).unwrap();
        let mut cursor = Cursor::new(&bytes[4..]);
        let blocks = read_blocks(&mut cursor, PatternMode::Read).unwrap();

        let types: Vec<&str> = blocks.iter().map(|b| b.block_type.as_str()).collect();
        assert_eq!(types, ["samp", "patt", "desc", "phry"]);

        let desc = blocks.iter().find(|b| b.block_type == "desc").unwrap();
        assert_ne!(
            desc.data.len() % 4,
            0,
            "the desc payload must be unaligned or this witness proves nothing"
        );
    }

    #[test]
    fn corpus_hero_samp_length_chain_frames_every_anchor() {
        let relative = "third-party/Hero-Artistic-Brushes-V3.abr";
        let Some(path) = corpus_file(relative) else {
            println!("skip: {relative} absent (set BRUSHKIT_CORPUS_DIR)");
            return;
        };
        let bytes = std::fs::read(&path).unwrap();
        let mut cursor = Cursor::new(&bytes[4..]);
        let blocks = read_blocks(&mut cursor, PatternMode::Read).unwrap();
        let samp = blocks.iter().find(|b| b.block_type == "samp").unwrap();

        let (frames, clean) = frame_samp_entries_by_length(samp.data, MAX_RECORDS).unwrap();
        assert!(clean, "the length chain frames the whole block");
        assert_eq!(frames.len(), 17);
        assert_eq!(
            frames.iter().filter(|&&(s, e)| (e - s) % 4 != 0).count(),
            11,
            "11 of the 17 bodies are unaligned"
        );

        let anchors = find_all_uuid_offsets(samp.data, MAX_RECORDS).unwrap();
        assert_eq!(anchors.len(), 17);
        let anchor_offsets: Vec<usize> = anchors.iter().map(|&(off, _)| off).collect();
        let frame_starts: Vec<usize> = frames.iter().map(|&(start, _)| start).collect();
        assert_eq!(frame_starts, anchor_offsets);

        let pack = parse_abr(&bytes).unwrap();
        assert_eq!(pack.brushes.len(), 24);
        assert_eq!(pack.preset_count, 32);
    }

    #[test]
    fn test_find_bitmap_header_simple() {
        let entry = build_simple_entry(4, 4, 8, 0xAB);
        let h = find_bitmap_header(&entry).unwrap();
        assert_eq!((h.right - h.left) as u32, 4);
        assert_eq!((h.bottom - h.top) as u32, 4);
        assert_eq!(h.depth, 8);
    }

    #[test]
    fn test_find_bitmap_header_v10() {
        let entry = build_v10_entry(64, 64, 0xCC);
        let h = find_bitmap_header(&entry).unwrap();
        assert_eq!((h.right - h.left) as u32, 64);
        assert_eq!((h.bottom - h.top) as u32, 64);
    }

    #[test]
    fn test_parse_samp_simple_v6() {
        let entry = build_simple_entry(4, 4, 8, 0xAB);
        let mut block = Vec::new();
        block.write_u32::<BigEndian>(entry.len() as u32).unwrap();
        block.extend_from_slice(&entry);
        let entries =
            parse_samp_block(&block, AbrVersion::V6, 1, TipMode::Eager, MAX_RECORDS).unwrap();
        assert_eq!(entries.len(), 1);
        let b = entries[0].as_ref().unwrap();
        assert_eq!(b.bitmap.width, 4);
        assert_eq!(b.bitmap.height, 4);
        assert!(b.uuid.is_none());
    }

    #[test]
    fn test_parse_samp_v10_uuid() {
        let e1 = build_v10_entry(8, 8, 0xDD);
        let e2 = build_v10_entry(16, 16, 0xEE);

        let mut block = Vec::new();
        block.extend_from_slice(&e1);
        block.extend_from_slice(&[0u8; 6]);
        let mut e2_mod = e2.clone();
        e2_mod[1] = b'b';
        block.extend_from_slice(&e2_mod);

        let entries =
            parse_samp_block(&block, AbrVersion::V10, 2, TipMode::Eager, MAX_RECORDS).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_some());
        assert!(entries[1].is_some());
        assert_eq!(entries[0].as_ref().unwrap().bitmap.width, 8);
        assert_eq!(entries[1].as_ref().unwrap().bitmap.width, 16);
    }

    fn push_framed(block: &mut Vec<u8>, body: &[u8]) {
        block.write_u32::<BigEndian>(body.len() as u32).unwrap();
        block.extend_from_slice(body);
        block.resize(block.len().next_multiple_of(4), 0);
    }

    #[test]
    fn samp_uuid_bytes_inside_pixels_do_not_move_the_record_boundary() {
        let fake = b"$11111111-1111-1111-1111-111111111111\0";
        let mut e1 = build_v10_entry(8, 8, 0xDD);
        e1[257..257 + fake.len()].copy_from_slice(fake);
        let mut e2 = build_v10_entry(4, 4, 0xEE);
        e2[1] = b'b';

        let mut block = Vec::new();
        push_framed(&mut block, &e1);
        push_framed(&mut block, &e2);

        assert_eq!(
            find_all_uuid_offsets(&block, MAX_RECORDS).unwrap().len(),
            3,
            "three anchors"
        );

        let entries =
            parse_samp_block(&block, AbrVersion::V10, 2, TipMode::Eager, MAX_RECORDS).unwrap();
        assert_eq!(entries.len(), 2);

        let first = entries[0].as_ref().unwrap();
        assert_eq!(
            first.uuid.as_deref(),
            Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        );
        assert_eq!(first.bitmap.width, 8);
        assert_eq!(first.bitmap.data[0], b'$');

        let second = entries[1].as_ref().unwrap();
        assert_eq!(
            second.uuid.as_deref(),
            Some("b1b2c3d4-e5f6-7890-abcd-ef1234567890")
        );
        assert_eq!(second.bitmap.width, 4);
        assert!(second.bitmap.data.iter().all(|&b| b == 0xEE));
    }

    #[test]
    fn samp_broken_length_chain_falls_back_to_anchor_scan() {
        let mut block = Vec::new();
        push_framed(&mut block, &build_v10_entry(8, 8, 0xDD));
        block.write_u32::<BigEndian>(0xFFFF_FFFF).unwrap();
        let mut e2 = build_v10_entry(4, 4, 0xEE);
        e2[1] = b'b';
        block.extend_from_slice(&e2);

        let (frames, clean) = frame_samp_entries_by_length(&block, MAX_RECORDS).unwrap();
        assert!(!clean);
        assert_eq!(frames.len(), 1);

        let entries =
            parse_samp_block(&block, AbrVersion::V10, 2, TipMode::Eager, MAX_RECORDS).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].as_ref().unwrap().bitmap.width, 8);
        assert_eq!(entries[1].as_ref().unwrap().bitmap.width, 4);
    }

    #[test]
    fn test_is_uuid_at() {
        let data = b"a1b2c3d4-e5f6-7890-abcd-ef1234567890";
        assert!(is_uuid_at(data, 0));
        assert!(!is_uuid_at(b"not-a-uuid-at-all-nope-no", 0));
    }

    #[test]
    fn test_find_all_uuid_offsets() {
        let mut data = Vec::new();
        data.extend_from_slice(b"prefix$a1b2c3d4-e5f6-7890-abcd-ef1234567890\0more");
        let uuids = find_all_uuid_offsets(&data, MAX_RECORDS).unwrap();
        assert_eq!(uuids.len(), 1);
        assert_eq!(uuids[0].1, "a1b2c3d4-e5f6-7890-abcd-ef1234567890");
    }

    #[test]
    fn test_extract_entry_uuid() {
        let data = b"$a1b2c3d4-e5f6-7890-abcd-ef1234567890\0rest";
        let uuid = extract_entry_uuid(data);
        assert_eq!(
            uuid.as_deref(),
            Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        );
    }

    #[test]
    fn test_extract_entry_uuid_absent() {
        assert!(extract_entry_uuid(&[0u8; 40]).is_none());
    }

    #[test]
    fn test_decode_rle_simple() {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(3).unwrap();
        d.push(1);
        d.push(0xAA);
        d.push(0xBB);
        assert_eq!(decode_rle(&d, 1, 2, 1, 2).unwrap(), vec![0xAA, 0xBB]);
    }

    #[test]
    fn test_decode_rle_repeat() {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(2).unwrap();
        d.push((-2i8) as u8);
        d.push(0xFF);
        assert_eq!(decode_rle(&d, 1, 3, 1, 3).unwrap(), vec![0xFF, 0xFF, 0xFF]);
    }

    fn rle(counts: &[u16], rows: &[u8]) -> Vec<u8> {
        let mut d = Vec::new();
        for c in counts {
            d.write_u16::<BigEndian>(*c).unwrap();
        }
        d.extend_from_slice(rows);
        d
    }

    #[test]
    fn rle_rows_decode_from_their_declared_slices() {
        let d = rle(&[3, 2], &[0x00, 0xAA, 0x99, 0x00, 0xBB]);
        assert_eq!(decode_rle(&d, 2, 1, 1, 2).unwrap(), vec![0xAA, 0xBB]);
    }

    #[test]
    fn rle_overlong_run_is_an_error_not_a_spill() {
        let d = rle(&[2, 2], &[0xFE, 0xAA, 0xFE, 0xBB]);
        let err = decode_rle(&d, 2, 2, 1, 4).unwrap_err();
        assert!(format!("{err}").contains("overruns"), "got: {err}");
    }

    #[test]
    fn rle_row_shorter_than_its_width_is_truncated() {
        let d = rle(&[1], &[0x00]);
        let err = decode_rle(&d, 1, 2, 1, 2).unwrap_err();
        assert!(format!("{err}").contains("truncated"), "got: {err}");
    }

    #[test]
    fn rle_declared_count_past_data_end_is_truncated() {
        let d = rle(&[5], &[0x00, 0xAA]);
        let err = decode_rle(&d, 1, 1, 1, 1).unwrap_err();
        assert!(format!("{err}").contains("truncated"), "got: {err}");
    }

    #[test]
    fn rle_sixteen_bit_row_returns_exactly_expected_bytes() {
        let d = rle(&[5], &[0x03, 0x12, 0x34, 0xAB, 0xCD]);
        let out = decode_rle(&d, 1, 2, 2, 4).unwrap();
        assert_eq!(out, vec![0x12, 0x34, 0xAB, 0xCD]);
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn rle_noop_byte_is_consumed_inside_the_row() {
        let d = rle(&[3], &[0x80, 0xFF, 0xAA]);
        assert_eq!(decode_rle(&d, 1, 2, 1, 2).unwrap(), vec![0xAA, 0xAA]);
    }

    #[test]
    fn samp_anchorless_entries_with_unaligned_first_length_decode_all() {
        let e1 = build_simple_entry(4, 4, 8, 0xAA);
        let e2 = build_simple_entry(8, 8, 8, 0xBB);
        let mut block = Vec::new();
        block.write_u32::<BigEndian>(e1.len() as u32).unwrap();
        block.extend_from_slice(&e1);
        block.extend_from_slice(&[0u8; 1]);
        block.write_u32::<BigEndian>(e2.len() as u32).unwrap();
        block.extend_from_slice(&e2);
        let entries =
            parse_samp_block(&block, AbrVersion::V6, 1, TipMode::Eager, MAX_RECORDS).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].as_ref().unwrap().bitmap.width, 4);
        assert_eq!(entries[1].as_ref().unwrap().bitmap.width, 8);
    }

    #[test]
    fn test_malformed_v2_huge_dimensions() {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(2).unwrap();
        d.write_u16::<BigEndian>(1).unwrap();
        d.write_u16::<BigEndian>(2).unwrap();
        d.write_u32::<BigEndian>(64).unwrap();
        d.write_u32::<BigEndian>(0).unwrap();
        d.write_u16::<BigEndian>(0).unwrap();
        d.write_u32::<BigEndian>(0).unwrap();
        d.push(0);
        for _ in 0..4 {
            d.write_i16::<BigEndian>(0).unwrap();
        }
        d.write_i32::<BigEndian>(0).unwrap();
        d.write_i32::<BigEndian>(0).unwrap();
        d.write_i32::<BigEndian>(0).unwrap();
        d.write_i32::<BigEndian>(0x4000_0001).unwrap();
        d.write_u16::<BigEndian>(8).unwrap();
        d.push(0);
        assert!(parse_abr(&d).is_err());
    }

    #[test]
    fn test_malformed_v2_dimension_overflow() {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(2).unwrap();
        d.write_u16::<BigEndian>(1).unwrap();
        d.write_u16::<BigEndian>(2).unwrap();
        d.write_u32::<BigEndian>(64).unwrap();
        d.write_u32::<BigEndian>(0).unwrap();
        d.write_u16::<BigEndian>(0).unwrap();
        d.write_u32::<BigEndian>(0).unwrap();
        d.push(0);
        for _ in 0..4 {
            d.write_i16::<BigEndian>(0).unwrap();
        }
        d.write_i32::<BigEndian>(0).unwrap();
        d.write_i32::<BigEndian>(i32::MIN).unwrap();
        d.write_i32::<BigEndian>(0).unwrap();
        d.write_i32::<BigEndian>(0).unwrap();
        d.write_u16::<BigEndian>(8).unwrap();
        d.push(0);
        assert!(parse_abr(&d).is_err());
    }

    #[test]
    fn test_find_bitmap_header_dimension_overflow() {
        let mut d = Vec::new();
        d.write_i32::<BigEndian>(0).unwrap();
        d.write_i32::<BigEndian>(i32::MIN).unwrap();
        d.write_i32::<BigEndian>(0).unwrap();
        d.write_i32::<BigEndian>(0).unwrap();
        d.write_u16::<BigEndian>(8).unwrap();
        d.push(0);
        assert!(find_bitmap_header(&d).is_none());
    }

    #[test]
    fn test_malformed_v2_huge_name() {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(2).unwrap();
        d.write_u16::<BigEndian>(1).unwrap();
        d.write_u16::<BigEndian>(2).unwrap();
        d.write_u32::<BigEndian>(64).unwrap();
        d.write_u32::<BigEndian>(0).unwrap();
        d.write_u16::<BigEndian>(0).unwrap();
        d.write_u32::<BigEndian>(0x4000_0000).unwrap();
        assert!(parse_abr(&d).is_err());
    }

    #[test]
    fn test_malformed_v6_oversized_dimensions_truncated() {
        let mut entry = Vec::new();
        entry.write_u32::<BigEndian>(0).unwrap();
        entry.write_i32::<BigEndian>(0).unwrap();
        entry.write_i32::<BigEndian>(0).unwrap();
        entry.write_i32::<BigEndian>(16384).unwrap();
        entry.write_i32::<BigEndian>(16384).unwrap();
        entry.write_u16::<BigEndian>(8).unwrap();
        entry.push(0);
        entry.extend_from_slice(&[0u8; 8]);

        let mut block = Vec::new();
        block.write_u32::<BigEndian>(entry.len() as u32).unwrap();
        block.extend_from_slice(&entry);

        let mut d = Vec::new();
        d.write_u16::<BigEndian>(6).unwrap();
        d.write_u16::<BigEndian>(1).unwrap();
        d.extend_from_slice(b"8BIM");
        d.extend_from_slice(b"samp");
        d.write_u32::<BigEndian>(block.len() as u32).unwrap();
        d.extend_from_slice(&block);

        let pack = parse_abr(&d).unwrap();
        assert!(pack.brushes.is_empty());
    }

    #[test]
    fn test_malformed_truncated_header() {
        let d: [u8; 1] = [0];
        assert!(parse_abr(&d).is_err());
    }

    /// A v1 or v2 entry stream holding `entries`, each from [`legacy_entry`].
    fn legacy_stream(version: u16, entries: &[Vec<u8>]) -> Vec<u8> {
        let mut d = Vec::new();
        d.write_u16::<BigEndian>(version).unwrap();
        d.write_u16::<BigEndian>(entries.len() as u16).unwrap();
        for entry in entries {
            d.extend_from_slice(entry);
        }
        d
    }

    /// One sampled entry (brush type 2), with an empty name on v2, followed
    /// by `pixels` as stored. `entry_len` overrides the entry's real length.
    fn legacy_entry(
        version: u16,
        width: i32,
        height: i32,
        depth: u16,
        compression: u8,
        pixels: &[u8],
        entry_len: Option<u32>,
    ) -> Vec<u8> {
        let mut body = Vec::new();
        body.write_u32::<BigEndian>(0).unwrap();
        body.write_u16::<BigEndian>(0).unwrap();
        if version == 2 {
            body.write_u32::<BigEndian>(0).unwrap();
        }
        body.push(0);
        for _ in 0..4 {
            body.write_i16::<BigEndian>(0).unwrap();
        }
        for v in [0, 0, height, width] {
            body.write_i32::<BigEndian>(v).unwrap();
        }
        body.write_u16::<BigEndian>(depth).unwrap();
        body.push(compression);
        body.extend_from_slice(pixels);

        let mut d = Vec::new();
        d.write_u16::<BigEndian>(2).unwrap();
        d.write_u32::<BigEndian>(entry_len.unwrap_or(body.len() as u32))
            .unwrap();
        d.extend_from_slice(&body);
        d
    }

    fn v2_stream_with_depth(depth: u16) -> Vec<u8> {
        legacy_stream(2, &[legacy_entry(2, 8, 8, depth, 0, &[0u8; 64], Some(64))])
    }

    #[test]
    fn v2_accepts_depth_8() {
        assert!(parse_abr(&v2_stream_with_depth(8)).is_ok());
    }

    #[test]
    fn v2_rejects_depth_zero() {
        assert!(parse_abr(&v2_stream_with_depth(0)).is_err());
    }

    #[test]
    fn v2_rejects_depth_256() {
        assert!(parse_abr(&v2_stream_with_depth(256)).is_err());
    }

    fn v2_rle_entry(entry_len: u32) -> Vec<u8> {
        legacy_stream(2, &[legacy_entry(2, 1, 1, 8, 1, &[], Some(entry_len))])
    }

    /// PackBits for `rows`, each row one literal run.
    fn rle_literal_rows(rows: &[Vec<u8>]) -> Vec<u8> {
        let counts: Vec<u16> = rows.iter().map(|row| row.len() as u16 + 1).collect();
        let mut packed = Vec::new();
        for row in rows {
            packed.push(row.len() as u8 - 1);
            packed.extend_from_slice(row);
        }
        rle(&counts, &packed)
    }

    /// A `width` x `height` 8-bit tip whose bytes count up from `seed`,
    /// wrapping at 256.
    fn tip_rows(width: usize, height: usize, seed: u8) -> Vec<Vec<u8>> {
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| seed.wrapping_add((y * width + x) as u8))
                    .collect()
            })
            .collect()
    }

    /// Three RLE tips and one raw tip whose entry is padded past its pixels.
    fn legacy_multi_tip_stream(version: u16) -> Vec<u8> {
        let rle_entry = |w: usize, h: usize, seed: u8| {
            let packed = rle_literal_rows(&tip_rows(w, h, seed));
            legacy_entry(version, w as i32, h as i32, 8, 1, &packed, None)
        };
        let raw_pixels: Vec<u8> = tip_rows(3, 2, 0x70).concat();
        let padded_len = legacy_entry(version, 3, 2, 8, 0, &raw_pixels, None).len() as u32 - 6 + 4;
        legacy_stream(
            version,
            &[
                rle_entry(3, 2, 0x10),
                rle_entry(5, 4, 0x30),
                [
                    legacy_entry(version, 3, 2, 8, 0, &raw_pixels, Some(padded_len)),
                    vec![0xEE; 4],
                ]
                .concat(),
                rle_entry(1, 1, 0x90),
            ],
        )
    }

    /// Asserts that every field but the tip pixels agrees.
    fn assert_same_but_pixels(all: &AbrPack, paired: &AbrPack) {
        assert_eq!(all.version, paired.version);
        assert_eq!(all.preset_count, paired.preset_count);
        assert_eq!(all.brushes.len(), paired.brushes.len());
        for (a, p) in all.brushes.iter().zip(&paired.brushes) {
            assert_eq!(
                (&a.id, &a.name, a.preset_index),
                (&p.id, &p.name, p.preset_index)
            );
            assert_eq!(
                (a.tip.width, a.tip.height, a.tip.depth),
                (p.tip.width, p.tip.height, p.tip.depth)
            );
        }
        assert_eq!(all.computed_presets.len(), paired.computed_presets.len());
        assert_eq!(all.dropped_samp_count, paired.dropped_samp_count);
        assert_eq!(all.skipped_preset_count, paired.skipped_preset_count);
        assert_eq!(all.unsupported_tip_count, paired.unsupported_tip_count);
        assert_eq!(
            all.dropped_tip_details.len(),
            paired.dropped_tip_details.len()
        );
        for (a, p) in all
            .dropped_tip_details
            .iter()
            .zip(&paired.dropped_tip_details)
        {
            assert_eq!(
                (&a.uuid, a.width, a.height, &a.owner_preset_names),
                (&p.uuid, p.width, p.height, &p.owner_preset_names)
            );
            assert_eq!(
                (a.bitmap.width, a.bitmap.height, a.bitmap.depth),
                (p.bitmap.width, p.bitmap.height, p.bitmap.depth)
            );
        }
    }

    /// Asserts that `decode_tip` reproduces `parse_abr`'s tip for every brush.
    fn assert_tips_match_eager(deferred: &DeferredPack<'_>, eager: &AbrPack) {
        assert_eq!(deferred.pack.brushes.len(), eager.brushes.len());
        for (i, brush) in eager.brushes.iter().enumerate() {
            let tip = deferred.decode_tip(i).unwrap();
            assert_eq!(
                (tip.width, tip.height, tip.depth, &tip.data),
                (
                    brush.tip.width,
                    brush.tip.height,
                    brush.tip.depth,
                    &brush.tip.data
                ),
                "brush {i}"
            );
        }
    }

    #[test]
    fn all_deferred_parse_decodes_no_legacy_tip() {
        for version in [1, 2] {
            let d = legacy_multi_tip_stream(version);
            let (eager, eager_decodes) = decodes(|| parse_abr(&d).unwrap());
            assert_eq!(eager.brushes.len(), 4, "v{version}");
            assert_eq!(eager_decodes, 4, "v{version}");

            let (paired, paired_decodes) =
                decodes(|| parse_abr_deferred_without_patterns(&d).unwrap());
            assert_eq!(paired_decodes, 4, "v{version}: the legacy tips stay eager");
            assert!(!paired.is_deferred(0));
            assert_tips_match_eager(&paired, &eager);

            let (all, all_decodes) =
                decodes(|| parse_abr_all_deferred_without_patterns(&d).unwrap());
            assert_eq!(all_decodes, 0, "v{version}");
            assert_same_but_pixels(&all.pack, &paired.pack);
            for (i, brush) in all.pack.brushes.iter().enumerate() {
                assert!(all.is_deferred(i), "v{version}: brush {i}");
                assert!(brush.tip.data.is_empty(), "v{version}: brush {i}");
                assert_eq!(all.tip_decoded_len(i), eager.brushes[i].tip.data.len());
            }

            let (_, one) = decodes(|| all.decode_tip(0).unwrap());
            assert_eq!(one, 1, "v{version}");
            assert_tips_match_eager(&all, &eager);
        }
    }

    #[test]
    fn all_deferred_parse_keeps_an_empty_legacy_tip_eager() {
        let d = legacy_stream(2, &[legacy_entry(2, 0, 0, 8, 0, &[], None)]);
        let all = parse_abr_all_deferred_without_patterns(&d).unwrap();
        assert!(!all.is_deferred(0));
        assert_tips_match_eager(&all, &parse_abr(&d).unwrap());
    }

    #[test]
    fn all_deferred_parse_does_not_decode_a_zero_area_legacy_tip() {
        // Width 0, height 3, RLE, and none of the 3 row byte counts.
        let d = legacy_stream(2, &[legacy_entry(2, 0, 3, 8, 1, &[], None)]);
        for result in [
            parse_abr(&d).map(|_| ()),
            parse_abr_deferred_without_patterns(&d).map(|_| ()),
        ] {
            assert!(
                matches!(&result, Err(AbrError::Decompression(msg)) if msg == "truncated RLE row byte counts"),
                "got {result:?}"
            );
        }

        let all = parse_abr_all_deferred_without_patterns(&d).unwrap();
        assert!(!all.is_deferred(0));
        let tip = all.decode_tip(0).unwrap();
        assert_eq!((tip.width, tip.height, tip.depth), (0, 3, 8));
        assert!(tip.data.is_empty());
    }

    #[test]
    fn raw_legacy_entry_ends_after_its_declared_length_or_its_pixels() {
        for version in [1, 2] {
            let pixels = tip_rows(3, 2, 0x70).concat();
            let exact_len = legacy_entry(version, 3, 2, 8, 0, &pixels, None).len() as u32 - 6;
            let padded = [
                legacy_entry(version, 3, 2, 8, 0, &pixels, Some(exact_len + 5)),
                vec![0xEE; 5],
            ]
            .concat();
            let short = legacy_entry(version, 3, 2, 8, 0, &pixels, Some(exact_len - 6));
            for (case, raw_entry) in [("padded", padded), ("short", short)] {
                let d = legacy_stream(
                    version,
                    &[raw_entry, legacy_entry(version, 1, 1, 8, 0, &[0x42], None)],
                );
                let pack = parse_abr(&d).unwrap();
                let tips: Vec<_> = pack
                    .brushes
                    .iter()
                    .map(|b| (b.tip.width, b.tip.height, b.tip.data.clone()))
                    .collect();
                assert_eq!(
                    tips,
                    [(1, 1, vec![0x42]), (3, 2, pixels.clone())],
                    "v{version} {case}"
                );
            }
        }
    }

    #[test]
    fn legacy_entry_length_past_usize_is_rejected() {
        // On 32-bit targets this `entry_end` truncates to a small `usize`.
        for compression in [0, 1] {
            let d = legacy_stream(
                1,
                &[legacy_entry(1, 1, 1, 8, compression, &[], Some(u32::MAX))],
            );
            assert!(parse_abr(&d).is_err(), "compression {compression}");
            assert!(
                parse_abr_all_deferred_without_patterns(&d).is_err(),
                "compression {compression}"
            );
        }
    }

    #[test]
    fn all_deferred_parse_rejects_raw_legacy_pixels_past_the_input() {
        // The entry length covers the header, so only the pixel read overruns.
        let header_len = legacy_entry(2, 2, 2, 8, 0, &[], None).len() as u32 - 6;
        let d = legacy_stream(
            2,
            &[legacy_entry(2, 2, 2, 8, 0, &[1, 2, 3], Some(header_len))],
        );
        for result in [
            parse_abr(&d).map(|_| ()),
            parse_abr_all_deferred_without_patterns(&d).map(|_| ()),
        ] {
            assert!(
                matches!(&result, Err(AbrError::Decompression(msg)) if msg == "truncated raw data"),
                "got {result:?}"
            );
        }
    }

    fn desc_text(buf: &mut Vec<u8>, key: &[u8], text: &str) {
        write_key(buf, key);
        buf.extend_from_slice(b"TEXT");
        let u: Vec<u16> = text.encode_utf16().collect();
        buf.write_u32::<BigEndian>(u.len() as u32 + 1).unwrap();
        for c in &u {
            buf.write_u16::<BigEndian>(*c).unwrap();
        }
        buf.write_u16::<BigEndian>(0).unwrap();
    }

    fn desc_objc(buf: &mut Vec<u8>, key: &[u8], class_id: &[u8], item_count: u32) {
        write_key(buf, key);
        buf.extend_from_slice(b"Objc");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        write_key(buf, class_id);
        buf.write_u32::<BigEndian>(item_count).unwrap();
    }

    /// A desc block of sampled presets `(name, tip uuid, dual-brush uuid)`.
    fn build_dual_desc_block(presets: &[(&str, &str, Option<&str>)]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.write_u32::<BigEndian>(16).unwrap();
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u16::<BigEndian>(0).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"null");
        buf.write_u32::<BigEndian>(1).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap();
        buf.extend_from_slice(b"Brsh");
        buf.extend_from_slice(b"VlLs");
        buf.write_u32::<BigEndian>(presets.len() as u32).unwrap();

        for (name, uuid, dual) in presets {
            desc_preset_header(&mut buf, 2 + u32::from(dual.is_some()));
            desc_name(&mut buf, name);
            desc_text(&mut buf, b"sampledData", uuid);
            if let Some(dual) = dual {
                desc_objc(&mut buf, b"dualBrush", b"dualBrush", 2);
                write_bool(&mut buf, b"useDualBrush", true);
                desc_objc(&mut buf, b"Brsh", b"sampledBrush", 1);
                desc_text(&mut buf, b"sampledData", dual);
            }
        }
        buf
    }

    const TIP_A: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    const TIP_B: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";
    const TIP_C: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567892";
    const TIP_D: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567893";

    /// Four samp tips. `Second` uses tip B, which `First` also names as its
    /// dual brush; tip D is only `Third`'s dual brush, so pairing drops it.
    fn dual_brush_pack() -> Vec<u8> {
        let mut samp = Vec::new();
        for (uuid, side, val) in [
            (TIP_A, 4, 0x11),
            (TIP_B, 5, 0x22),
            (TIP_C, 6, 0x33),
            (TIP_D, 7, 0x44),
        ] {
            push_framed(&mut samp, &build_v10_entry_with_uuid(uuid, side, side, val));
        }
        let desc = build_dual_desc_block(&[
            ("First", TIP_A, Some(TIP_B)),
            ("Second", TIP_B, None),
            ("Third", TIP_C, Some(TIP_D)),
        ]);
        let mut d = v10_header();
        push_block(&mut d, b"samp", &samp);
        push_block(&mut d, b"desc", &desc);
        d
    }

    #[test]
    fn all_deferred_parse_decodes_no_dual_brush_or_dropped_tip() {
        let d = dual_brush_pack();
        let eager = parse_abr(&d).unwrap();
        let names: Vec<&str> = eager.brushes.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["First", "Second", "Third"]);
        assert_eq!(eager.dropped_tip_details.len(), 1);
        assert_eq!(eager.dropped_tip_details[0].uuid.as_deref(), Some(TIP_D));

        let (paired, paired_decodes) = decodes(|| parse_abr_deferred(&d).unwrap());
        assert_eq!(
            paired_decodes, 2,
            "the dual-brush tip B and the dropped tip D"
        );
        assert!(!paired.is_deferred(1));
        assert_eq!(
            paired.pack.dropped_tip_details[0].bitmap.data,
            eager.dropped_tip_details[0].bitmap.data
        );

        let paired = parse_abr_deferred_without_patterns(&d).unwrap();
        assert_tips_match_eager(&paired, &eager);
        let (all, all_decodes) = decodes(|| parse_abr_all_deferred_without_patterns(&d).unwrap());
        assert_eq!(all_decodes, 0);
        assert_same_but_pixels(&all.pack, &paired.pack);
        assert!((0..3).all(|i| all.is_deferred(i)));
        assert!(all.pack.dropped_tip_details[0].bitmap.data.is_empty());
        assert_tips_match_eager(&all, &eager);
    }

    #[test]
    fn parse_v2_rle_entry_len_overflow_errors() {
        assert!(parse_abr(&v2_rle_entry(u32::MAX)).is_err());
    }

    #[test]
    fn parse_v2_rle_entry_len_underflow_errors() {
        assert!(parse_abr(&v2_rle_entry(4)).is_err());
    }

    #[test]
    fn raw_entry_copies_only_expected() {
        let header = BitmapHeader {
            rect_offset: 0,
            top: 0,
            left: 0,
            bottom: 2,
            right: 2,
            depth: 8,
            compression: 0,
        };
        let mut data = vec![0u8; 19];
        data.extend(std::iter::repeat_n(0xAB, 1000));
        let tip = decode_bitmap(&data, &header).unwrap();
        assert_eq!(tip.data.len(), 4);
        assert!(tip.data.iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn raw_entry_too_short_errors() {
        let header = BitmapHeader {
            rect_offset: 0,
            top: 0,
            left: 0,
            bottom: 2,
            right: 2,
            depth: 8,
            compression: 0,
        };
        let mut data = vec![0u8; 19];
        data.extend([1u8, 2u8]);
        let err = decode_bitmap(&data, &header).unwrap_err();
        assert!(format!("{err}").contains("too short"), "got: {err}");
    }

    #[test]
    fn rle_reservation_bounded() {
        let height = 10_000usize;
        let width = 10_000usize;
        let expected = width * height;
        let data = vec![0u8; height * 2];
        let err = decode_rle(&data, height, width, 1, expected).unwrap_err();
        assert!(format!("{err}").contains("truncated"), "got: {err}");
    }
}
