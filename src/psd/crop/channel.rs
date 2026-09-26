use super::Plan;
use crate::{Result, invalid};
use ag_psd::psd::Compression;
use std::io::Read;

pub(super) fn crop(
    bytes: &[u8],
    compression: Compression,
    large: bool,
    plan: Plan,
) -> Result<Vec<u8>> {
    if let Some(fill) = plan.fill {
        return Ok(vec![fill; plan.target.pixels() as usize]);
    }
    let (width, height) = (plan.source.width(), plan.source.height());
    let (w, h) = (plan.target.width(), plan.target.height());
    if w == 0 || h == 0 {
        return Ok(Vec::new());
    }
    let (x, y) = (
        (plan.target.left - plan.source.left) as usize,
        (plan.target.top - plan.source.top) as usize,
    );
    let mut output = Vec::with_capacity(w * h);
    match compression {
        Compression::RawData => {
            if bytes.len() != width * height {
                return Err(truncated());
            }
            for row in y..y + h {
                output.extend_from_slice(&bytes[row * width + x..row * width + x + w]);
            }
        }
        Compression::RleCompressed => {
            let entry = if large { 4 } else { 2 };
            let table_size = height * entry;
            let table = bytes.get(..table_size).ok_or_else(truncated)?;
            let mut offset = table_size;
            let mut row_buffer = vec![0; width];
            for (row, length) in table.chunks_exact(entry).enumerate() {
                let length = length
                    .iter()
                    .fold(0usize, |value, byte| (value << 8) | usize::from(*byte));
                let end = offset.checked_add(length).ok_or_else(truncated)?;
                let encoded = bytes.get(offset..end).ok_or_else(truncated)?;
                if (y..y + h).contains(&row) {
                    unpack(encoded, &mut row_buffer)?;
                    output.extend_from_slice(&row_buffer[x..x + w]);
                }
                offset = end;
            }
            if offset != bytes.len() {
                return Err(truncated());
            }
        }
        Compression::ZipWithoutPrediction | Compression::ZipWithPrediction => {
            let mut decoder = flate2::read::ZlibDecoder::new(bytes);
            let mut row_buffer = vec![0; width];
            // Decode only through the last retained scanline. Cropped-away ZIP
            // tails are deliberately not inflated, avoiding decompression bombs.
            for row in 0..y + h {
                decoder
                    .read_exact(&mut row_buffer)
                    .map_err(|_| truncated())?;
                if compression == Compression::ZipWithPrediction {
                    for index in 1..width {
                        row_buffer[index] = row_buffer[index].wrapping_add(row_buffer[index - 1]);
                    }
                }
                if row >= y {
                    output.extend_from_slice(&row_buffer[x..x + w]);
                }
            }
        }
    }
    Ok(output)
}
fn truncated() -> crate::Error {
    invalid(
        "PSD cropped channel data is truncated or has an invalid scanline. Obtain a complete source file; the current document is unchanged.",
    )
}
fn unpack(bytes: &[u8], row: &mut [u8]) -> Result<()> {
    let (mut source, mut target) = (0, 0);
    while source < bytes.len() {
        let code = bytes[source] as i8;
        source += 1;
        match code {
            -128 => {}
            0..=127 => {
                let count = code as usize + 1;
                let input = bytes.get(source..source + count).ok_or_else(truncated)?;
                row.get_mut(target..target + count)
                    .ok_or_else(truncated)?
                    .copy_from_slice(input);
                source += count;
                target += count;
            }
            _ => {
                let count = (1i16 - i16::from(code)) as usize;
                let value = *bytes.get(source).ok_or_else(truncated)?;
                row.get_mut(target..target + count)
                    .ok_or_else(truncated)?
                    .fill(value);
                source += 1;
                target += count;
            }
        }
    }
    if target != row.len() {
        return Err(truncated());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psd::crop::Rect;
    use std::io::Write;
    fn plan() -> Plan {
        Plan {
            source: Rect {
                left: -1,
                top: -1,
                right: 3,
                bottom: 2,
            },
            target: Rect {
                left: 0,
                top: 0,
                right: 2,
                bottom: 2,
            },
            fill: None,
        }
    }
    #[test]
    fn raw_zip_and_predicted_zip_crop_identical_pixels() {
        let pixels: Vec<u8> = (0..12).collect();
        let expected = vec![5, 6, 9, 10];
        assert_eq!(
            crop(&pixels, Compression::RawData, false, plan()).unwrap(),
            expected
        );
        for predicted in [false, true] {
            let mut data = pixels.clone();
            if predicted {
                for row in data.chunks_mut(4) {
                    for i in (1..4).rev() {
                        row[i] = row[i].wrapping_sub(row[i - 1]);
                    }
                }
            }
            let mut zip = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            zip.write_all(&data).unwrap();
            let bytes = zip.finish().unwrap();
            let compression = if predicted {
                Compression::ZipWithPrediction
            } else {
                Compression::ZipWithoutPrediction
            };
            assert_eq!(crop(&bytes, compression, false, plan()).unwrap(), expected);
        }
    }
    #[test]
    fn packbits_rejects_overflow_underflow_and_missing_runs() {
        for bytes in [vec![3, 1], vec![254, 10], vec![250, 10], vec![255]] {
            assert!(unpack(&bytes, &mut [0; 4]).is_err());
        }
        let mut row = [0; 4];
        unpack(&[128, 1, 20, 30, 255, 40], &mut row).unwrap();
        assert_eq!(row, [20, 30, 40, 40]);
    }
}
