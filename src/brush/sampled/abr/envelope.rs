//! Bound framing before the third-party parser and retain legacy tip spacing.
use super::*;

const MAX_RECORDS: usize = 2048;
const MAX_DESCRIPTOR_BYTES: usize = 4 * 1024 * 1024;

pub(super) struct Envelope {
    pub legacy_spacing: Option<Vec<f64>>,
    pub patterns: bool,
}
fn malformed() -> crate::Error {
    invalid(
        "The ABR brush pack is truncated or has invalid record lengths. Export the pack again; no tips were loaded.",
    )
}
fn take<'a>(bytes: &'a [u8], at: &mut usize, count: usize) -> Result<&'a [u8]> {
    let end = at.checked_add(count).ok_or_else(malformed)?;
    let value = bytes.get(*at..end).ok_or_else(malformed)?;
    *at = end;
    Ok(value)
}
fn word(bytes: &[u8], at: &mut usize) -> Result<u32> {
    let value: [u8; 4] = take(bytes, at, 4)?.try_into().map_err(|_| malformed())?;
    Ok(u32::from_be_bytes(value))
}
fn short(bytes: &[u8], at: &mut usize) -> Result<u16> {
    let value: [u8; 2] = take(bytes, at, 2)?.try_into().map_err(|_| malformed())?;
    Ok(u16::from_be_bytes(value))
}
fn bounded_count(count: usize) -> Result<()> {
    if count > MAX_RECORDS {
        return Err(invalid(
            "The ABR pack contains more than 2048 records. Export a smaller pack before importing; no tips were loaded.",
        ));
    }
    Ok(())
}
pub(super) fn inspect(bytes: &[u8]) -> Result<Envelope> {
    let mut at = 0;
    let version = short(bytes, &mut at)?;
    let second = short(bytes, &mut at)?;
    if matches!(version, 1 | 2) {
        return legacy(bytes, at, version, second);
    }
    if !matches!(version, 6 | 7 | 9 | 10) || !matches!(second, 1 | 2) {
        return Err(invalid(
            "Supported ABR versions are 1, 2, 6, 7, 9 and 10 (modern subversions 1 and 2). Export a compatible ABR pack; no tips were loaded.",
        ));
    }
    let mut blocks = 0;
    let mut samples = 0;
    let mut descriptors = 0;
    let mut patterns = false;
    while at < bytes.len() {
        blocks += 1;
        bounded_count(blocks)?;
        if take(bytes, &mut at, 4)? != b"8BIM" {
            return Err(malformed());
        }
        let kind = take(bytes, &mut at, 4)?;
        let length = word(bytes, &mut at)? as usize;
        let body = take(bytes, &mut at, length)?;
        match kind {
            b"desc" => {
                descriptors += length;
                if descriptors > MAX_DESCRIPTOR_BYTES {
                    return Err(invalid(
                        "The ABR pack's preset descriptions exceed 4 MiB. Export a smaller pack; no tips were loaded.",
                    ));
                }
            }
            b"patt" => patterns |= !body.is_empty(),
            b"samp" => {
                let mut cursor = 0;
                while cursor < body.len() {
                    let length = word(body, &mut cursor)? as usize;
                    if length == 0 {
                        return Err(malformed());
                    }
                    take(body, &mut cursor, length)?;
                    cursor = cursor.next_multiple_of(4).min(body.len());
                    samples += 1;
                    bounded_count(samples)?;
                }
            }
            _ => {}
        }
        at = at.next_multiple_of(4).min(bytes.len());
    }
    Ok(Envelope {
        legacy_spacing: None,
        patterns,
    })
}
fn legacy(bytes: &[u8], mut at: usize, version: u16, count: u16) -> Result<Envelope> {
    bounded_count(count as usize)?;
    let mut spacing = Vec::new();
    for _ in 0..count {
        let kind = short(bytes, &mut at)?;
        let length = word(bytes, &mut at)? as usize;
        let start = at;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(malformed)?;
        if kind != 2 {
            at = end;
            continue;
        }
        word(bytes, &mut at)?;
        let value = f64::from(short(bytes, &mut at)?) / 100.;
        if version == 2 {
            let chars = word(bytes, &mut at)? as usize;
            take(bytes, &mut at, chars.checked_mul(2).ok_or_else(malformed)?)?;
        }
        take(bytes, &mut at, 9)?; // Anti-alias flag and four legacy i16 bounds.
        let top = word(bytes, &mut at)? as i32;
        let left = word(bytes, &mut at)? as i32;
        let bottom = word(bytes, &mut at)? as i32;
        let right = word(bytes, &mut at)? as i32;
        let depth = short(bytes, &mut at)?;
        let compression = take(bytes, &mut at, 1)?[0];
        if at > end || !matches!(depth, 8 | 16) {
            return Err(malformed());
        }
        if compression == 0 {
            let width = i64::from(right) - i64::from(left);
            let height = i64::from(bottom) - i64::from(top);
            if width < 0 || height < 0 {
                return Err(malformed());
            }
            let size = (width as u64)
                .checked_mul(height as u64)
                .and_then(|n| n.checked_mul(u64::from(depth / 8)))
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(malformed)?;
            take(bytes, &mut at, size)?;
            at = at.max(end);
        } else if compression == 1 {
            at = end;
        } else {
            return Err(invalid(
                "The legacy ABR pack uses unsupported compression. Export it with uncompressed or RLE tips; no tips were loaded.",
            ));
        }
        spacing.push(value);
    }
    if at != bytes.len() {
        return Err(malformed());
    }
    // brushkit exposes legacy sampled entries in reverse file order.
    spacing.reverse();
    Ok(Envelope {
        legacy_spacing: Some(spacing),
        patterns: false,
    })
}
