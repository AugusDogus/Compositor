//! PAT framing follows the standalone 8BPT layout, then Photoshop's version-3
//! virtual-memory array list. Color slots are followed by an unused slot and
//! a transparency slot. Unlike PSD Patt resources, PAT records have no outer
//! length prefix. See https://github.com/psd-tools/psd-tools/blob/main/src/psd_tools/psd/patterns.py.
use super::{MAX_PACK_PIXELS, MAX_PATTERNS, Pattern, validate_size};
use crate::{Result, invalid};
use image::RgbaImage;
use std::collections::HashSet;

fn malformed() -> crate::Error {
    invalid(
        "The PAT file is truncated or has inconsistent record lengths, channels, or pixel data. Export the pattern pack again; no patterns were imported.",
    )
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(count).ok_or_else(malformed)?;
        let bytes = self.bytes.get(self.at..end).ok_or_else(malformed)?;
        self.at = end;
        Ok(bytes)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn short(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().map_err(|_| malformed())?,
        ))
    }
    fn word(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().map_err(|_| malformed())?,
        ))
    }
    fn block(&mut self) -> Result<Reader<'a>> {
        let size = self.word()? as usize;
        Ok(Self::new(self.take(size)?))
    }
    fn finish(self) -> Result<()> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(malformed())
        }
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    top: i32,
    left: i32,
    width: u32,
    height: u32,
}
impl Bounds {
    fn read(reader: &mut Reader<'_>) -> Result<Self> {
        let top = reader.word()? as i32;
        let left = reader.word()? as i32;
        let bottom = reader.word()? as i32;
        let right = reader.word()? as i32;
        let width = u32::try_from(i64::from(right) - i64::from(left)).map_err(|_| malformed())?;
        let height = u32::try_from(i64::from(bottom) - i64::from(top)).map_err(|_| malformed())?;
        validate_size(width, height)?;
        Ok(Self {
            top,
            left,
            width,
            height,
        })
    }
    fn offset(self, child: Self) -> Result<[u32; 2]> {
        let x =
            u32::try_from(i64::from(child.left) - i64::from(self.left)).map_err(|_| malformed())?;
        let y =
            u32::try_from(i64::from(child.top) - i64::from(self.top)).map_err(|_| malformed())?;
        if u64::from(x) + u64::from(child.width) > u64::from(self.width)
            || u64::from(y) + u64::from(child.height) > u64::from(self.height)
        {
            return Err(malformed());
        }
        Ok([x, y])
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Grayscale,
    Rgb,
}
impl Mode {
    fn channels(self) -> usize {
        match self {
            Self::Grayscale => 1,
            Self::Rgb => 3,
        }
    }
}
enum Encoding<'a> {
    Raw(&'a [u8]),
    Rle { lengths: &'a [u8], rows: &'a [u8] },
}
struct Channel<'a> {
    bounds: Bounds,
    encoding: Encoding<'a>,
}
struct Record<'a> {
    id: String,
    name: String,
    bounds: Bounds,
    mode: Mode,
    colors: Vec<Channel<'a>>,
    alpha: Option<Channel<'a>>,
}

pub(super) fn decode(bytes: &[u8]) -> Result<Vec<Pattern>> {
    let mut reader = Reader::new(bytes);
    if reader.take(4)? != b"8BPT" || reader.short()? != 1 {
        return Err(invalid(
            "This is not a supported Photoshop PAT version-1 pack. GIMP PAT files use a different format. Export an Adobe PAT pack; no patterns were imported.",
        ));
    }
    let count = reader.word()? as usize;
    if !(1..=MAX_PATTERNS).contains(&count) {
        return Err(invalid(
            "Import a PAT pack containing 1 to 256 patterns. Export a smaller pack; no patterns were imported.",
        ));
    }
    let mut records = Vec::with_capacity(count);
    let mut ids = HashSet::new();
    let mut pixels = 0_u64;
    for _ in 0..count {
        let record = record(&mut reader)?;
        pixels += u64::from(record.bounds.width) * u64::from(record.bounds.height);
        if pixels > MAX_PACK_PIXELS {
            return Err(invalid(
                "The PAT pack exceeds 32 megapixels across its tiles. Export a smaller pack; no patterns were imported.",
            ));
        }
        if !ids.insert(record.id.clone()) {
            return Err(invalid(
                "The PAT pack repeats a pattern identifier. Export the patterns with distinct identifiers; no patterns were imported.",
            ));
        }
        records.push(record);
    }
    reader.finish()?;
    records.into_iter().map(Record::decode).collect()
}

fn record<'a>(reader: &mut Reader<'a>) -> Result<Record<'a>> {
    if reader.word()? != 1 {
        return Err(invalid(
            "This PAT entry uses an unsupported pattern version. Export a version-1 pattern pack; no patterns were imported.",
        ));
    }
    let mode = match reader.word()? {
        1 => Mode::Grayscale,
        3 => Mode::Rgb,
        _ => {
            return Err(invalid(
                "PAT import supports 8-bit RGB and grayscale patterns. Convert indexed, CMYK, Lab, or multichannel patterns to RGB before exporting; no patterns were imported.",
            ));
        }
    };
    reader.take(4)?; // Header point is not authoritative pixel bounds.
    let units = reader.word()? as usize;
    if units == 0 || units > 4096 {
        return Err(invalid(
            "A PAT pattern name must contain 1 to 4096 UTF-16 characters. Rename the pattern and export again; no patterns were imported.",
        ));
    }
    let utf16: Vec<_> = reader
        .take(units * 2)?
        .chunks_exact(2)
        .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
        .collect();
    let name = String::from_utf16(&utf16).map_err(|_| invalid("A PAT pattern name contains invalid UTF-16. Rename it and export again; no patterns were imported."))?.trim_end_matches('\0').to_owned();
    let name = if let Some((_, label)) = name
        .strip_prefix("$$$/")
        .and_then(|name| name.split_once('='))
    {
        label.replace("^C", "©").replace("^R", "®")
    } else {
        name
    };
    let length = reader.byte()? as usize;
    let id = String::from_utf8(reader.take(length)?.to_vec()).map_err(|_| malformed())?;
    super::validate_labels(&id, &name)?;
    if reader.word()? != 3 {
        return Err(invalid(
            "This PAT entry uses an unsupported pixel-array version. Export a compatible Photoshop PAT pack; no patterns were imported.",
        ));
    }
    let mut data = reader.block()?;
    let bounds = Bounds::read(&mut data)?;
    let slots = data.word()? as usize;
    if !(1..=64).contains(&slots) {
        return Err(malformed());
    }
    let mut colors = Vec::with_capacity(mode.channels());
    for _ in 0..slots {
        if let Some(channel) = channel(&mut data, bounds)? {
            if colors.len() == mode.channels() {
                return Err(invalid(
                    "This PAT pattern contains extra color or spot channels. Convert it to ordinary RGB or grayscale before exporting; no patterns were imported.",
                ));
            }
            colors.push(channel);
        }
    }
    if channel(&mut data, bounds)?.is_some() {
        return Err(invalid(
            "This PAT pattern uses an unsupported auxiliary channel. Remove auxiliary channels and export again; no patterns were imported.",
        ));
    }
    let alpha = channel(&mut data, bounds)?;
    data.finish()?;
    if colors.len() != mode.channels() {
        return Err(malformed());
    }
    Ok(Record {
        id,
        name,
        bounds,
        mode,
        colors,
        alpha,
    })
}

fn channel<'a>(reader: &mut Reader<'a>, parent: Bounds) -> Result<Option<Channel<'a>>> {
    match reader.word()? {
        0 => return Ok(None),
        1 => (),
        _ => return Err(malformed()),
    }
    let mut data = reader.block()?;
    if data.bytes.is_empty() {
        return Ok(None);
    }
    let depth = data.word()?;
    let bounds = Bounds::read(&mut data)?;
    parent.offset(bounds)?;
    if depth != 8 || data.short()? != 8 {
        return Err(invalid(
            "PAT import currently requires 8-bit channels. Convert the patterns to 8-bit RGB or grayscale before exporting; no patterns were imported.",
        ));
    }
    let compression = data.byte()?;
    let bytes = data.take(data.bytes.len() - data.at)?;
    let encoding = match compression {
        0 if bytes.len() == bounds.width as usize * bounds.height as usize => Encoding::Raw(bytes),
        0 => return Err(malformed()),
        1 => {
            let split = bounds.height as usize * 2;
            let lengths = bytes.get(..split).ok_or_else(malformed)?;
            let rows = bytes.get(split..).ok_or_else(malformed)?;
            let expected: usize = lengths
                .chunks_exact(2)
                .map(|row| usize::from(u16::from_be_bytes([row[0], row[1]])))
                .sum();
            if expected != rows.len() {
                return Err(malformed());
            }
            Encoding::Rle { lengths, rows }
        }
        _ => {
            return Err(invalid(
                "PAT import supports uncompressed and PackBits/RLE channels. Export without ZIP compression; no patterns were imported.",
            ));
        }
    };
    Ok(Some(Channel { bounds, encoding }))
}

impl Record<'_> {
    fn decode(self) -> Result<Pattern> {
        let mut pixels = RgbaImage::from_pixel(
            self.bounds.width,
            self.bounds.height,
            image::Rgba([0, 0, 0, 255]),
        );
        for (index, channel) in self.colors.into_iter().enumerate() {
            write_channel(
                &mut pixels,
                self.bounds,
                channel,
                match self.mode {
                    Mode::Grayscale => None,
                    Mode::Rgb => Some(index),
                },
            )?;
        }
        if let Some(alpha) = self.alpha {
            // Pixels outside a cropped transparency plane are transparent.
            for pixel in pixels.pixels_mut() {
                pixel[3] = 0;
            }
            write_channel(&mut pixels, self.bounds, alpha, Some(3))?;
        }
        Pattern::new(self.id, self.name, pixels)
    }
}

fn write_channel(
    pixels: &mut RgbaImage,
    parent: Bounds,
    channel: Channel<'_>,
    component: Option<usize>,
) -> Result<()> {
    let [left, top] = parent.offset(channel.bounds)?;
    let width = channel.bounds.width as usize;
    let mut row = Vec::new();
    let mut offset = 0;
    for y in 0..channel.bounds.height as usize {
        let samples = match &channel.encoding {
            Encoding::Raw(bytes) => &bytes[y * width..(y + 1) * width],
            Encoding::Rle { lengths, rows } => {
                let count = usize::from(u16::from_be_bytes([lengths[y * 2], lengths[y * 2 + 1]]));
                unpack_row(&rows[offset..offset + count], width, &mut row)?;
                offset += count;
                &row
            }
        };
        for (x, value) in samples.iter().copied().enumerate() {
            let pixel = pixels.get_pixel_mut(left + x as u32, top + y as u32);
            match component {
                Some(index) => pixel[index] = value,
                None => pixel.0[..3].fill(value),
            }
        }
    }
    Ok(())
}

fn unpack_row(bytes: &[u8], width: usize, out: &mut Vec<u8>) -> Result<()> {
    out.clear();
    let mut reader = Reader::new(bytes);
    while reader.at < bytes.len() {
        let code = reader.byte()? as i8;
        match code {
            0..=127 => {
                let count = code as usize + 1;
                if out.len() + count > width {
                    return Err(malformed());
                }
                out.extend_from_slice(reader.take(count)?);
            }
            -127..=-1 => {
                let count = (1 - i16::from(code)) as usize;
                if out.len() + count > width {
                    return Err(malformed());
                }
                out.resize(out.len() + count, reader.byte()?);
            }
            -128 => (),
        }
    }
    if out.len() != width {
        return Err(malformed());
    }
    Ok(())
}
