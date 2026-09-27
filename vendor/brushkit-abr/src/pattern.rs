//! Parser for the ABR `patt` (pattern) block — the embedded texture pixels a
//! Photoshop brush references via `Txtr > Ptrn > Idnt`.
//!
//! All fields big-endian; offsets in this module are relative to each record's
//! DATA start (i.e. after its `u32 pattern_length` prefix).
//!
//! Image modes read: 1 (Grayscale, channel 0 taken as-is), 2 (Indexed, via the
//! record's palette) and 3 (RGB, Rec.601 luma).

use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Cursor, Read};

use super::{DroppedPatternDetail, DroppedPatternReason, UnreadablePattChunk};
use crate::limits::{MAX_DIMENSION, MAX_NAME_CODE_UNITS};

/// One decoded texture pattern from the ABR `patt` block.
#[derive(Debug, Clone)]
pub struct AbrPattern {
    /// Pattern UUID (`pattern_id`, ASCII Pascal string). Matches the brush
    /// descriptor's `Txtr > Ptrn > Idnt` — the key a consumer joins grains on.
    pub id: String,
    /// Pattern name (`name`, UTF-16BE, trailing NUL stripped). Matches the
    /// descriptor's `Txtr > Ptrn > Nm  `.
    pub name: String,
    /// Pattern width in pixels (from the written channel rectangle).
    pub width: u32,
    /// Pattern height in pixels (from the written channel rectangle).
    pub height: u32,
    /// PSD image mode of the source record: 1 = Grayscale, 2 = Indexed,
    /// 3 = RGB.
    pub mode: u32,
    /// 8-bit luminance, row-major, `width * height` long. Grayscale records
    /// contribute channel 0 as-is; indexed pixels are resolved through the
    /// record's palette; RGB planes are combined with the Rec.601 luma formula
    /// (0.299R + 0.587G + 0.114B).
    pub gray: Vec<u8>,
}

pub(crate) struct PattBlockOutcome {
    pub patterns: Vec<AbrPattern>,
    pub dropped: Vec<DroppedPatternDetail>,
    pub unreadable: Vec<UnreadablePattChunk>,
}

/// Parse a `patt` block into every decodable pattern record, plus a detail for
/// each record that was dropped.
///
/// Records are concatenated; each is `4 + pattern_length` bytes, padded to the
/// next **4-byte** boundary.
///
/// A record whose channels or luminance cannot be resolved is skipped and
/// reported in `dropped`; remaining records still decode. A chunk whose header
/// does not parse is reported in `unreadable` and the walk continues past it.
/// An empty block yields empty vecs.
///
/// `block_index` is this block's ordinal among the file's `patt` blocks and
/// `first_record_index` the number of records the earlier blocks held, so the
/// diagnostics index into the whole file.
pub(crate) fn parse_patt_block(
    data: &[u8],
    block_index: usize,
    first_record_index: usize,
) -> PattBlockOutcome {
    let mut patterns = Vec::new();
    let mut dropped = Vec::new();
    let mut unreadable = Vec::new();
    let mut offset = 0usize;
    let mut record_index = first_record_index;

    while offset + 4 <= data.len() {
        let pattern_length = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;

        let record_end = match offset
            .checked_add(4)
            .and_then(|x| x.checked_add(pattern_length))
        {
            Some(end) if end <= data.len() => end,
            _ => break,
        };

        let record = &data[offset + 4..record_end];
        let mut c = Cursor::new(record);
        if let Some(header) = read_record_header(&mut c) {
            match parse_record_body(record, &mut c, &header) {
                Some(pattern) => patterns.push(pattern),
                None => dropped.push(drop_detail(record_index, header)),
            }
        } else {
            unreadable.push(UnreadablePattChunk {
                block_index,
                offset,
                declared_length: pattern_length,
            });
        }
        record_index += 1;

        offset = record_end.next_multiple_of(4);
    }

    PattBlockOutcome {
        patterns,
        dropped,
        unreadable,
    }
}

/// Classify a record whose header parsed but whose body did not decode.
pub(crate) fn drop_detail(record_index: usize, header: RecordHeader) -> DroppedPatternDetail {
    let reason = if is_supported_mode(header.mode) {
        DroppedPatternReason::Undecodable
    } else {
        DroppedPatternReason::UnsupportedImageMode(header.mode)
    };
    DroppedPatternDetail {
        record_index,
        id: header.id,
        mode: header.mode,
        reason,
    }
}

pub(crate) struct RecordHeader {
    pub(crate) mode: u32,
    pub(crate) name: String,
    pub(crate) id: String,
}

/// Read a record's leading field sequence — version, mode, the header
/// height/width pair, the UTF-16BE name and the ASCII Pascal UUID — leaving the
/// cursor immediately after the UUID. `None` on any malformed field.
///
/// The header's height/width pair is deliberately NOT returned: it is
/// transposed relative to the channel rectangle on at least one corpus record
/// (`kloir-basic-landscape-brushes.abr`), so the pattern's dimensions come from
/// the channel rect further down.
pub(crate) fn read_record_header(c: &mut Cursor<&[u8]>) -> Option<RecordHeader> {
    let _version = c.read_u32::<BigEndian>().ok()?;
    let mode = c.read_u32::<BigEndian>().ok()?;
    let _height = c.read_u16::<BigEndian>().ok()?;
    let _width = c.read_u16::<BigEndian>().ok()?;

    let name_units = c.read_u32::<BigEndian>().ok()? as usize;
    if name_units > MAX_NAME_CODE_UNITS {
        return None;
    }
    let mut name_buf = vec![0u8; name_units.checked_mul(2)?];
    c.read_exact(&mut name_buf).ok()?;
    let name = utf16be_to_string(&name_buf);

    let id_len = c.read_u8().ok()? as usize;
    let mut id_buf = vec![0u8; id_len];
    c.read_exact(&mut id_buf).ok()?;
    let id = String::from_utf8(id_buf).ok()?;

    Some(RecordHeader { mode, name, id })
}

pub(crate) fn parse_record_body(
    record: &[u8],
    c: &mut Cursor<&[u8]>,
    header: &RecordHeader,
) -> Option<AbrPattern> {
    let mode = header.mode;

    // Palette + color-table trailer (indexed mode only): 256 RGB triplets,
    // then two u16 (color count / transparency index — meaning inferred, values
    // unused here) that close the mode-2 color-table section before the VM list.
    let palette = if mode == 2 {
        let mut p = [0u8; 768];
        c.read_exact(&mut p).ok()?;
        let _ = c.read_u16::<BigEndian>().ok()?;
        let _ = c.read_u16::<BigEndian>().ok()?;
        Some(p)
    } else {
        None
    };

    let _vma_version = c.read_u32::<BigEndian>().ok()?;
    let _list_length = c.read_u32::<BigEndian>().ok()?;
    let _rect = read_rect(c)?;
    // Channel-slot count field: Photoshop writes a fixed max (24 on e12) that
    // does NOT match the number of slots actually present, so we bound the walk
    // by the record end instead of trusting it.
    let _channel_slots = c.read_u32::<BigEndian>().ok()?;

    let mut planes: Vec<Vec<u8>> = Vec::new();
    let mut ch_width = 0u32;
    let mut ch_height = 0u32;
    loop {
        let pos = c.position() as usize;
        if pos + 4 > record.len() {
            break;
        }
        let written = c.read_u32::<BigEndian>().ok()?;
        if written == 0 {
            continue;
        }

        let array_length = c.read_u32::<BigEndian>().ok()? as usize;
        if array_length < 23 {
            return None;
        }
        let body_start = c.position() as usize;
        let body_end = body_start.checked_add(array_length)?;
        if body_end > record.len() {
            return None;
        }

        let _pixel_depth = c.read_u32::<BigEndian>().ok()?;
        let (top, left, bottom, right) = read_rect(c)?;
        let _pixel_depth2 = c.read_u16::<BigEndian>().ok()?;
        let compression = c.read_u8().ok()?;

        let w = right.checked_sub(left)?;
        let h = bottom.checked_sub(top)?;
        if w > MAX_DIMENSION || h > MAX_DIMENSION {
            return None;
        }
        let pixel_start = c.position() as usize;
        let plane = decode_channel(compression, &record[pixel_start..body_end], w, h)?;

        planes.push(plane);
        ch_width = w;
        ch_height = h;
        c.set_position(body_end as u64);
    }

    let gray = resolve_gray(mode, palette.as_ref(), &planes, ch_width, ch_height)?;

    Some(AbrPattern {
        id: header.id.clone(),
        name: header.name.clone(),
        width: ch_width,
        height: ch_height,
        mode,
        gray,
    })
}

fn read_rect(c: &mut Cursor<&[u8]>) -> Option<(u32, u32, u32, u32)> {
    let top = c.read_u32::<BigEndian>().ok()?;
    let left = c.read_u32::<BigEndian>().ok()?;
    let bottom = c.read_u32::<BigEndian>().ok()?;
    let right = c.read_u32::<BigEndian>().ok()?;
    Some((top, left, bottom, right))
}

/// Decode one 8-bit channel plane. `compression`: 0 = raw, 1 = PackBits (RLE).
fn decode_channel(compression: u8, data: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let expected = (width as usize).checked_mul(height as usize)?;
    match compression {
        0 => {
            if data.len() < expected {
                return None;
            }
            Some(data[..expected].to_vec())
        }
        1 => super::parser::decode_rle(data, height as usize, width as usize, 1, expected).ok(),
        _ => None,
    }
}

/// The PSD image modes [`resolve_gray`] decodes: 1 (Grayscale), 2 (Indexed) and
/// 3 (RGB).
const SUPPORTED_MODES: &[u32] = &[1, 2, 3];

fn is_supported_mode(mode: u32) -> bool {
    SUPPORTED_MODES.contains(&mode)
}

fn resolve_gray(
    mode: u32,
    palette: Option<&[u8; 768]>,
    planes: &[Vec<u8>],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    if !is_supported_mode(mode) {
        return None;
    }
    // Target plane length. A plane can be sized to a DIFFERENT channel than
    // width/height (multi-channel records), so every arm rejects a short plane
    // and slices an over-long one to exactly `n` — keeping `gray.len() ==
    // width*height`.
    let n = (width as usize).checked_mul(height as usize)?;
    match mode {
        // Grayscale: channel 0 IS the luminance plane. A record may carry a
        // second channel (an alpha; `Size Flow Gang.abr` has one, uniform 255),
        // so take the first plane rather than requiring exactly one.
        1 => {
            let gray = planes.first()?;
            if gray.len() < n {
                return None;
            }
            Some(gray[..n].to_vec())
        }
        2 => {
            let palette = palette?;
            let indices = planes.first()?;
            if indices.len() < n {
                return None;
            }
            let gray = indices[..n]
                .iter()
                .map(|&i| {
                    let base = i as usize * 3;
                    luma(palette[base], palette[base + 1], palette[base + 2])
                })
                .collect();
            Some(gray)
        }
        // RGB: the first three planes are R, G, B → luma. A record may carry
        // MORE than three (the granite tile in `Rocks and Water Brushes.abr`
        // has R/G/B raw plus a fourth PackBits plane), so take the first three
        // rather than requiring exactly three.
        3 => match planes.len() {
            1 => {
                if planes[0].len() < n {
                    return None;
                }
                Some(planes[0][..n].to_vec())
            }
            len if len >= 3 => {
                let (r, g, b) = (&planes[0], &planes[1], &planes[2]);
                if r.len() < n || g.len() < n || b.len() < n {
                    return None;
                }
                Some((0..n).map(|i| luma(r[i], g[i], b[i])).collect())
            }
            _ => None,
        },
        _ => None,
    }
}

fn luma(r: u8, g: u8, b: u8) -> u8 {
    let y = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
    y.round().clamp(0.0, 255.0) as u8
}

fn utf16be_to_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_be_bytes(*c))
        .collect();
    let mut s = String::from_utf16_lossy(&units);
    if s.ends_with('\0') {
        s.pop();
    }
    s
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use byteorder::WriteBytesExt;

    fn packbits_channel(rows: &[Vec<u8>]) -> Vec<u8> {
        let mut counts = Vec::new();
        let mut body = Vec::new();
        for row in rows {
            let mut encoded = Vec::new();
            encoded.push((row.len() - 1) as u8);
            encoded.extend_from_slice(row);
            counts.write_u16::<BigEndian>(encoded.len() as u16).unwrap();
            body.extend_from_slice(&encoded);
        }
        let mut out = counts;
        out.extend_from_slice(&body);
        out
    }

    pub(crate) fn channel_slot(w: u32, h: u32, compression: u8, pixel_data: &[u8]) -> Vec<u8> {
        let mut header = Vec::new();
        header.write_u32::<BigEndian>(8).unwrap();
        for v in [0u32, 0, h, w] {
            header.write_u32::<BigEndian>(v).unwrap();
        }
        header.write_u16::<BigEndian>(8).unwrap();
        header.write_u8(compression).unwrap();
        header.extend_from_slice(pixel_data);

        let mut slot = Vec::new();
        slot.write_u32::<BigEndian>(1).unwrap();
        slot.write_u32::<BigEndian>(header.len() as u32).unwrap();
        slot.extend_from_slice(&header);
        slot
    }

    pub(crate) fn record_body(
        mode: u32,
        dims: (u32, u32),
        name: &str,
        uuid: &str,
        palette: Option<&[u8; 768]>,
        slots: &[Vec<u8>],
        unwritten_slots: usize,
    ) -> Vec<u8> {
        let (w, h) = dims;
        let mut r = Vec::new();
        r.write_u32::<BigEndian>(1).unwrap();
        r.write_u32::<BigEndian>(mode).unwrap();
        r.write_u16::<BigEndian>(h as u16).unwrap();
        r.write_u16::<BigEndian>(w as u16).unwrap();

        let utf16: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        r.write_u32::<BigEndian>(utf16.len() as u32).unwrap();
        for u in &utf16 {
            r.write_u16::<BigEndian>(*u).unwrap();
        }

        r.write_u8(uuid.len() as u8).unwrap();
        r.extend_from_slice(uuid.as_bytes());

        if let Some(p) = palette {
            r.extend_from_slice(p);
            r.write_u16::<BigEndian>(256).unwrap();
            r.write_u16::<BigEndian>(255).unwrap();
        }

        r.write_u32::<BigEndian>(3).unwrap();
        r.write_u32::<BigEndian>(0).unwrap();
        for v in [0u32, 0, h, w] {
            r.write_u32::<BigEndian>(v).unwrap();
        }
        r.write_u32::<BigEndian>((slots.len() + unwritten_slots) as u32)
            .unwrap();
        for slot in slots {
            r.extend_from_slice(slot);
        }
        for _ in 0..unwritten_slots {
            r.write_u32::<BigEndian>(0).unwrap();
        }
        r
    }

    pub(crate) fn record_block(bodies: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for body in bodies {
            out.write_u32::<BigEndian>(body.len() as u32).unwrap();
            out.extend_from_slice(body);
            while out.len() % 4 != 0 {
                out.push(0);
            }
        }
        out
    }

    fn gray_palette() -> [u8; 768] {
        let mut p = [0u8; 768];
        for i in 0..256 {
            p[i * 3] = i as u8;
            p[i * 3 + 1] = i as u8;
            p[i * 3 + 2] = i as u8;
        }
        p
    }

    #[test]
    fn mode2_indexed_packbits() {
        let palette = gray_palette();
        let rows = vec![vec![10u8, 20], vec![30u8, 40]];
        let channel = channel_slot(2, 2, 1, &packbits_channel(&rows));
        let body = record_body(
            2,
            (2, 2),
            "tex",
            "uuid-indexed",
            Some(&palette),
            &[channel],
            23,
        );
        let block = record_block(&[body]);

        let patterns = parse_patt_block(&block, 0, 0).patterns;
        assert_eq!(patterns.len(), 1);
        let p = &patterns[0];
        assert_eq!(p.mode, 2);
        assert_eq!((p.width, p.height), (2, 2));
        assert_eq!(p.id, "uuid-indexed");
        assert_eq!(p.name, "tex");
        assert_eq!(p.gray, vec![10, 20, 30, 40]);
    }

    #[test]
    fn mode3_rgb_three_raw_channels() {
        let r = channel_slot(2, 2, 0, &[255, 0, 0, 0]);
        let g = channel_slot(2, 2, 0, &[0, 255, 0, 0]);
        let b = channel_slot(2, 2, 0, &[0, 0, 255, 0]);
        let body = record_body(3, (2, 2), "rgb", "uuid-rgb", None, &[r, g, b], 21);
        let block = record_block(&[body]);

        let patterns = parse_patt_block(&block, 0, 0).patterns;
        assert_eq!(patterns.len(), 1);
        let p = &patterns[0];
        assert_eq!(p.mode, 3);
        assert_eq!(p.gray, vec![76, 150, 29, 0]);
    }

    #[test]
    fn mode3_rgb_fourth_plane_ignored() {
        let r = channel_slot(2, 2, 0, &[255, 0, 0, 100]);
        let g = channel_slot(2, 2, 0, &[0, 255, 0, 100]);
        let b = channel_slot(2, 2, 0, &[0, 0, 255, 100]);
        let extra = channel_slot(2, 2, 1, &packbits_channel(&[vec![7, 7], vec![7, 7]]));
        let body = record_body(3, (2, 2), "rgb4", "uuid-rgb4", None, &[r, g, b, extra], 20);
        let block = record_block(&[body]);

        let patterns = parse_patt_block(&block, 0, 0).patterns;
        assert_eq!(patterns.len(), 1);
        let p = &patterns[0];
        assert_eq!(p.mode, 3);
        assert_eq!((p.width, p.height), (2, 2));
        assert_eq!(p.gray, vec![76, 150, 29, 100]);
    }

    #[test]
    fn mode3_rgb_two_planes_rejected() {
        let r = channel_slot(2, 2, 0, &[255, 0, 0, 0]);
        let g = channel_slot(2, 2, 0, &[0, 255, 0, 0]);
        let body = record_body(3, (2, 2), "rgb2", "uuid-rgb2", None, &[r, g], 22);
        let block = record_block(&[body]);

        let parsed = parse_patt_block(&block, 0, 0);
        assert!(parsed.patterns.is_empty());
        assert_eq!(parsed.dropped.len(), 1);
    }

    #[test]
    fn mode1_grayscale_single_raw_channel() {
        let channel = channel_slot(2, 2, 0, &[10, 20, 30, 40]);
        let body = record_body(1, (2, 2), "gray", "uuid-gray", None, &[channel], 23);
        let block = record_block(&[body]);

        let patterns = parse_patt_block(&block, 0, 0).patterns;
        assert_eq!(patterns.len(), 1);
        let p = &patterns[0];
        assert_eq!(p.mode, 1);
        assert_eq!((p.width, p.height), (2, 2));
        assert_eq!(p.id, "uuid-gray");
        assert_eq!(p.gray, vec![10, 20, 30, 40]);
    }

    #[test]
    fn mode1_grayscale_extra_alpha_channel_ignored() {
        let gray = channel_slot(2, 2, 0, &[10, 20, 30, 40]);
        let alpha = channel_slot(2, 2, 0, &[255; 4]);
        let body = record_body(1, (2, 2), "gray", "uuid-gray-a", None, &[gray, alpha], 22);
        let block = record_block(&[body]);

        let patterns = parse_patt_block(&block, 0, 0).patterns;
        assert_eq!(patterns.len(), 1);
        let p = &patterns[0];
        assert_eq!(p.mode, 1);
        assert_eq!(p.gray, vec![10, 20, 30, 40]);
    }

    #[test]
    fn two_records_four_byte_padding() {
        let palette = gray_palette();
        let ch_a = channel_slot(2, 2, 1, &packbits_channel(&[vec![1u8, 2], vec![3u8, 4]]));
        let body_a = record_body(2, (2, 2), "a", "uuid-aaaa", Some(&palette), &[ch_a], 2);
        let ch_b = channel_slot(2, 2, 1, &packbits_channel(&[vec![5u8, 6], vec![7u8, 8]]));
        let body_b = record_body(2, (2, 2), "b", "uuid-bbbb", Some(&palette), &[ch_b], 2);
        let block = record_block(&[body_a, body_b]);

        let patterns = parse_patt_block(&block, 0, 0).patterns;
        assert_eq!(patterns.len(), 2);
        assert_eq!(patterns[0].id, "uuid-aaaa");
        assert_eq!(patterns[0].gray, vec![1, 2, 3, 4]);
        assert_eq!(patterns[1].id, "uuid-bbbb");
        assert_eq!(patterns[1].gray, vec![5, 6, 7, 8]);
    }

    #[test]
    fn records_separated_by_two_and_three_byte_gaps_all_decode() {
        let mut bodies = Vec::new();
        for (uuid, pixels) in [
            ("pad2-aaa", [1u8, 2, 3, 4]),
            ("pad3-bb", [5u8, 6, 7, 8]),
            ("tail-cc", [9u8, 10, 11, 12]),
        ] {
            let channel = channel_slot(2, 2, 0, &pixels);
            bodies.push(record_body(1, (2, 2), "gray", uuid, None, &[channel], 23));
        }
        let gaps: Vec<usize> = bodies.iter().map(|b| (4 - (4 + b.len()) % 4) % 4).collect();
        assert_eq!(gaps[0], 2, "first gap must be the 2-byte case");
        assert_eq!(gaps[1], 3, "second gap must be the 3-byte case");

        let block = record_block(&bodies);
        let outcome = parse_patt_block(&block, 0, 0);

        assert!(outcome.dropped.is_empty(), "no record should be dropped");
        let ids: Vec<&str> = outcome.patterns.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["pad2-aaa", "pad3-bb", "tail-cc"]);
        assert_eq!(outcome.patterns[2].gray, vec![9, 10, 11, 12]);
    }

    #[test]
    fn empty_block_yields_no_patterns() {
        assert_eq!(parse_patt_block(&[], 0, 0).patterns.len(), 0);
    }

    #[test]
    fn unsupported_image_mode_is_reported_and_the_healthy_neighbour_survives() {
        let bad_ch = channel_slot(2, 2, 0, &[10, 20, 30, 40]);
        let bad = record_body(4, (2, 2), "unsupported", "uuid-mode4", None, &[bad_ch], 23);
        let good_ch = channel_slot(2, 2, 0, &[1, 2, 3, 4]);
        let good = record_body(1, (2, 2), "fine", "uuid-good", None, &[good_ch], 23);
        let block = record_block(&[bad, good]);

        let outcome = parse_patt_block(&block, 0, 0);
        assert_eq!(outcome.patterns.len(), 1);
        assert_eq!(outcome.patterns[0].id, "uuid-good");
        assert_eq!(outcome.dropped.len(), 1);
        let d = &outcome.dropped[0];
        assert_eq!(d.record_index, 0);
        assert_eq!(d.mode, 4);
        assert_eq!(d.reason, DroppedPatternReason::UnsupportedImageMode(4));
        assert_eq!(d.id, "uuid-mode4");
    }

    #[test]
    fn record_truncated_mid_channel_is_reported_as_undecodable() {
        let ch = channel_slot(2, 2, 0, &[10, 20, 30, 40]);
        let mut body = record_body(1, (2, 2), "cut", "uuid-cut", None, &[ch], 0);
        body.truncate(body.len() - 5);
        let block = record_block(&[body]);

        let outcome = parse_patt_block(&block, 0, 0);
        assert!(outcome.patterns.is_empty());
        assert_eq!(outcome.dropped.len(), 1);
        let d = &outcome.dropped[0];
        assert_eq!(d.record_index, 0);
        assert_eq!(d.reason, DroppedPatternReason::Undecodable);
        assert_eq!(d.mode, 1);
    }

    pub(crate) fn unreadable_chunk() -> Vec<u8> {
        let mut junk = Vec::new();
        junk.write_u32::<BigEndian>(1).unwrap();
        junk.write_u32::<BigEndian>(1).unwrap();
        junk.write_u16::<BigEndian>(2).unwrap();
        junk.write_u16::<BigEndian>(2).unwrap();
        junk.write_u32::<BigEndian>(u32::MAX).unwrap();
        junk
    }

    #[test]
    fn chunk_with_an_unreadable_header_is_counted_but_is_not_a_drop() {
        let good_ch = channel_slot(2, 2, 0, &[1, 2, 3, 4]);
        let good = record_body(1, (2, 2), "fine", "uuid-good", None, &[good_ch], 23);
        let junk = unreadable_chunk();
        let block = record_block(&[good.clone(), junk.clone(), good.clone()]);

        let outcome = parse_patt_block(&block, 0, 0);
        assert_eq!(outcome.dropped.len(), 0);
        assert_eq!(outcome.patterns.len(), 2);
        assert_eq!(outcome.patterns[0].gray, vec![1, 2, 3, 4]);
        assert_eq!(outcome.patterns[1].gray, vec![1, 2, 3, 4]);

        assert_eq!(outcome.unreadable.len(), 1);
        let u = &outcome.unreadable[0];
        assert_eq!(u.offset, (4 + good.len()).next_multiple_of(4));
        assert_eq!(u.declared_length, junk.len());
        assert_eq!(u.block_index, 0);
    }

    #[test]
    fn multi_block_record_index_counts_unreadable_chunks() {
        let good_ch = channel_slot(2, 2, 0, &[1, 2, 3, 4]);
        let good = record_body(1, (2, 2), "fine", "uuid-good", None, &[good_ch], 23);
        let junk = unreadable_chunk();
        let block_a = record_block(&[good.clone(), junk.clone()]);

        let bad_ch = channel_slot(2, 2, 0, &[10, 20, 30, 40]);
        let bad = record_body(4, (2, 2), "unsupported", "uuid-mode4", None, &[bad_ch], 23);
        let block_b = record_block(&[bad]);

        let mut file = Vec::new();
        file.write_u16::<BigEndian>(10).unwrap();
        file.write_u16::<BigEndian>(2).unwrap();
        for block in [&block_a, &block_b] {
            file.extend_from_slice(b"8BIM");
            file.extend_from_slice(b"patt");
            file.write_u32::<BigEndian>(block.len() as u32).unwrap();
            file.extend_from_slice(block);
        }

        let pack = crate::parse_abr(&file).unwrap();
        assert_eq!(pack.patterns.len(), 1);
        assert_eq!(pack.dropped_pattern_count, 1);
        assert_eq!(pack.dropped_pattern_details[0].record_index, 2);

        assert_eq!(pack.unreadable_patt_chunk_count, 1);
        let u = &pack.unreadable_patt_chunks[0];
        assert_eq!(u.block_index, 0);
        assert_eq!(u.offset, (4 + good.len()).next_multiple_of(4));
        assert_eq!(u.declared_length, junk.len());
    }

    #[test]
    fn undersized_channel_array_length_is_skipped() {
        let palette = gray_palette();
        let mut bad_slot = Vec::new();
        bad_slot.write_u32::<BigEndian>(1).unwrap();
        bad_slot.write_u32::<BigEndian>(0).unwrap();
        bad_slot.extend_from_slice(&[0u8; 23]);
        let body = record_body(2, (2, 2), "bad", "uuid-bad", Some(&palette), &[bad_slot], 0);
        let block = record_block(&[body]);

        let out = parse_patt_block(&block, 0, 0);
        assert!(out.patterns.is_empty());
        assert_eq!(out.dropped.len(), 1, "the record must surface as a drop");
        assert_eq!(out.dropped[0].id, "uuid-bad");
        assert_eq!(out.dropped[0].record_index, 0);
        assert!(
            matches!(out.dropped[0].reason, DroppedPatternReason::Undecodable),
            "mode 2 is supported, so an undersized slot is a body failure: {:?}",
            out.dropped[0].reason
        );
    }

    #[test]
    fn truncated_record_is_skipped() {
        let palette = gray_palette();
        let ch = channel_slot(2, 2, 1, &packbits_channel(&[vec![1u8, 2], vec![3u8, 4]]));
        let body = record_body(2, (2, 2), "t", "uuid-trunc", Some(&palette), &[ch], 2);
        let good = record_body(
            1,
            (2, 2),
            "fine",
            "uuid-good",
            None,
            &[channel_slot(2, 2, 0, &[1, 2, 3, 4])],
            23,
        );
        let mut block = record_block(&[good, body]);
        block.truncate(block.len() - 20);

        let out = parse_patt_block(&block, 0, 0);
        assert_eq!(out.patterns.len(), 1, "the intact record must still decode");
        assert_eq!(out.patterns[0].id, "uuid-good");
        assert!(
            out.dropped.is_empty(),
            "a truncated tail is not a decodable-record loss: {:?}",
            out.dropped
        );
        assert!(
            out.unreadable.is_empty(),
            "nor a chunk whose header we read: {:?}",
            out.unreadable
        );
    }

    #[test]
    fn patt_oversized_name_skips_record() {
        let mut body = Vec::new();
        body.write_u32::<BigEndian>(1).unwrap();
        body.write_u32::<BigEndian>(2).unwrap();
        body.write_u16::<BigEndian>(2).unwrap();
        body.write_u16::<BigEndian>(2).unwrap();
        body.write_u32::<BigEndian>(u32::MAX).unwrap();
        let block = record_block(&[body]);

        let patterns = parse_patt_block(&block, 0, 0).patterns;
        assert!(patterns.is_empty());
    }

    #[test]
    fn patt_oversized_channel_skips_record() {
        let palette = gray_palette();
        let pixels = vec![7u8; 20000];
        let big = channel_slot(20000, 1, 0, &pixels);
        let body = record_body(2, (20000, 1), "big", "uuid-big", Some(&palette), &[big], 0);
        let block = record_block(&[body]);

        let patterns = parse_patt_block(&block, 0, 0).patterns;
        assert!(patterns.is_empty());
    }

    #[test]
    fn patt_short_gray_plane_yields_none_or_valid() {
        let palette = gray_palette();
        let ch0 = channel_slot(1, 1, 0, &[100u8]);
        let ch1 = channel_slot(2, 2, 0, &[1u8, 2, 3, 4]);
        let body = record_body(
            2,
            (2, 2),
            "short",
            "uuid-short",
            Some(&palette),
            &[ch0, ch1],
            0,
        );
        let block = record_block(&[body]);

        let patterns = parse_patt_block(&block, 0, 0).patterns;
        assert!(patterns
            .iter()
            .all(|p| p.gray.len() == (p.width * p.height) as usize));
    }

    #[test]
    fn patt_long_gray_plane_yields_none_or_valid() {
        let palette = gray_palette();
        let ch0 = channel_slot(2, 2, 0, &[1u8, 2, 3, 4]);
        let ch1 = channel_slot(1, 1, 0, &[9u8]);
        let body = record_body(
            2,
            (1, 1),
            "long",
            "uuid-long",
            Some(&palette),
            &[ch0, ch1],
            0,
        );
        let block = record_block(&[body]);

        let patterns = parse_patt_block(&block, 0, 0).patterns;
        assert!(patterns
            .iter()
            .all(|p| p.gray.len() == (p.width * p.height) as usize));
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].gray.len(), 1);
    }
}
