//! GBR v2: seven big-endian words, a NUL-terminated name, then gray/RGBA pixels.
//! Format reference: GIMP app/core/gimpbrush-header.h and gimpbrush-load.c.
use super::*;
use std::{io::Read, path::Path};
const MAX_FILE_BYTES: u64 = (MAX_TIP_PIXELS * 4 + 4096) as u64;

pub fn read(path: &Path) -> Result<Tip> {
    if !std::fs::metadata(path)?.is_file() {
        return Err(invalid(
            "A brush must be a regular GBR file. Choose a GBR v2 brush exported from GIMP.",
        ));
    }
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err(invalid(
            "A brush must be a regular GBR file no larger than 16 MiB.",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    parse(&bytes)
}

pub(super) fn parse(data: &[u8]) -> Result<Tip> {
    let malformed = || {
        invalid(
            "The GBR brush is truncated or has an invalid header. Export it again as a GIMP GBR version 2 brush.",
        )
    };
    if data.len() < 29 || data.len() as u64 > MAX_FILE_BYTES {
        return Err(malformed());
    }
    let word = |offset| -> Result<u32> {
        let bytes: [u8; 4] = data
            .get(offset..offset + 4)
            .ok_or_else(malformed)?
            .try_into()
            .map_err(|_| malformed())?;
        Ok(u32::from_be_bytes(bytes))
    };
    if word(4)? != 2 {
        return Err(invalid(
            "Only GBR version 2 brushes are supported. Export the brush as GBR from GIMP; CinePaint float brushes and GIH hoses are not supported yet.",
        ));
    }
    let header = word(0)? as usize;
    let width = word(8)?;
    let height = word(12)?;
    let channels = word(16)? as usize;
    let spacing = word(24)?;
    if data.get(20..24) != Some(b"GIMP")
        || !(29..=4096).contains(&header)
        || width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || u64::from(width) * u64::from(height) > MAX_TIP_PIXELS as u64
        || !matches!(channels, 1 | 4)
        || !(1..=1000).contains(&spacing)
    {
        return Err(malformed());
    }
    let name = data.get(28..header).ok_or_else(malformed)?;
    if name.last() != Some(&0) || name[..name.len() - 1].contains(&0) {
        return Err(malformed());
    }
    let name = String::from_utf8_lossy(&name[..name.len() - 1])
        .trim()
        .to_string();
    let count = width as usize * height as usize;
    let end = header.checked_add(count * channels).ok_or_else(malformed)?;
    if end != data.len() {
        return Err(malformed());
    }
    let raw = &data[header..end];
    let pixels = if channels == 1 {
        raw.to_vec()
    } else {
        raw.chunks_exact(4).map(|pixel| pixel[3]).collect()
    };
    let pixels = GrayImage::from_raw(width, height, pixels).ok_or_else(malformed)?;
    Ok(Tip {
        name: if name.is_empty() {
            "Imported tip".into()
        } else {
            name
        },
        pixels,
        spacing: f64::from(spacing) / 100.,
        colored: channels == 4,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_gimp_fixture_establishes_mask_polarity_and_file_spacing() {
        let pixel = parse(include_bytes!("../../../tests/fixtures/gbr/pixel.gbr")).unwrap();
        assert_eq!(pixel.name(), "Pixel (1x1 square)");
        assert_eq!(pixel.pixels.dimensions(), (1, 1));
        assert_eq!(pixel.pixels[(0, 0)][0], 255);
        assert_eq!(pixel.spacing(), 0.5);
        let bristles = parse(include_bytes!(
            "../../../tests/fixtures/gbr/bristles-01.gbr"
        ))
        .unwrap();
        assert_eq!(bristles.pixels.dimensions(), (64, 64));
        assert!(bristles.pixels.pixels().any(|p| p[0] == 0));
        assert!(bristles.pixels.pixels().any(|p| p[0] == 255));
    }
    #[test]
    fn parses_rgba_as_alpha_and_rejects_corrupt_or_unbounded_files() {
        let mut bytes = include_bytes!("../../../tests/fixtures/gbr/pixel.gbr").to_vec();
        bytes[16..20].copy_from_slice(&4u32.to_be_bytes());
        bytes.pop();
        bytes.extend([20, 40, 60, 180]);
        let tip = parse(&bytes).unwrap();
        assert!(tip.embedded_colors());
        assert_eq!(tip.pixels[(0, 0)][0], 180);
        for (offset, value) in [
            (0, u32::MAX),
            (4, 3),
            (8, 0),
            (8, u32::MAX),
            (12, 4097),
            (16, 2),
            (24, 0),
            (24, 1001),
        ] {
            let mut bad = bytes.clone();
            bad[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
            assert!(parse(&bad).is_err(), "offset {offset}, value {value}");
        }
        for end in 0..bytes.len() {
            assert!(parse(&bytes[..end]).is_err());
        }
        bytes.push(0);
        assert!(parse(&bytes).is_err());
    }
    #[test]
    #[cfg(unix)]
    fn read_rejects_devices_and_fifos_before_opening() {
        assert!(read(Path::new("/dev/zero")).is_err());
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("brush.gbr");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        assert!(read(&fifo).is_err());
    }
}
