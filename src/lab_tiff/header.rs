use crate::{Result, invalid};

/// Change only the first directory's validated, inline PhotometricInterpretation
/// SHORT. Supports both endian orders and Classic TIFF/BigTIFF directory layouts.
pub(super) fn raw_channels(bytes: &mut [u8], expected: u16) -> Result<()> {
    let little = match bytes.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return Err(invalid("The Lab TIFF byte order is invalid.")),
    };
    fn value(bytes: &[u8], at: usize, size: usize, little: bool) -> Result<u64> {
        let data = at
            .checked_add(size)
            .and_then(|end| bytes.get(at..end))
            .ok_or_else(|| invalid("The Lab TIFF metadata is truncated."))?;
        Ok(if little {
            data.iter().rev().fold(0, |v, b| (v << 8) | u64::from(*b))
        } else {
            data.iter().fold(0, |v, b| (v << 8) | u64::from(*b))
        })
    }
    let (offset, count_size, entry_size, count_width, value_offset) =
        match value(bytes, 2, 2, little)? {
            42 => (value(bytes, 4, 4, little)?, 2, 12, 4, 8),
            43 if value(bytes, 4, 2, little)? == 8 && value(bytes, 6, 2, little)? == 0 => {
                (value(bytes, 8, 8, little)?, 8, 20, 8, 12)
            }
            _ => return Err(invalid("The Lab TIFF header is invalid.")),
        };
    let offset = usize::try_from(offset)
        .map_err(|_| invalid("The Lab TIFF directory offset is too large."))?;
    let count = usize::try_from(value(bytes, offset, count_size, little)?)
        .map_err(|_| invalid("The Lab TIFF directory is too large."))?;
    let start = offset
        .checked_add(count_size)
        .ok_or_else(|| invalid("The Lab TIFF directory offset is too large."))?;
    if count
        .checked_mul(entry_size)
        .and_then(|n| start.checked_add(n))
        .is_none_or(|end| end > bytes.len())
    {
        return Err(invalid("The Lab TIFF directory is incomplete."));
    }
    let mut location = None;
    for i in 0..count {
        let at = start + i * entry_size;
        if value(bytes, at, 2, little)? == 262 {
            if location.is_some()
                || value(bytes, at + 2, 2, little)? != 3
                || value(bytes, at + 4, count_width, little)? != 1
                || value(bytes, at + value_offset, 2, little)? != u64::from(expected)
            {
                return Err(invalid(
                    "The Lab TIFF photometric tag is inconsistent. Re-export the image.",
                ));
            }
            location = Some(at + value_offset);
        }
    }
    let at = location.ok_or_else(|| invalid("The Lab TIFF photometric tag is missing."))?;
    bytes[at..at + 2].copy_from_slice(&if little {
        2_u16.to_le_bytes()
    } else {
        2_u16.to_be_bytes()
    });
    Ok(())
}
