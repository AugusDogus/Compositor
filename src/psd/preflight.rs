//! Validate allocation-driving geometry before handing bytes to the PSD decoder.
use super::ConversionReport;
use crate::{Result, document::validate_size, invalid};
use std::borrow::Cow;

pub(super) struct Prepared<'a> {
    pub report: ConversionReport,
    pub metadata: Vec<super::vector_metadata::Metadata>,
    pub bytes: Cow<'a, [u8]>,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or_else(|| invalid("PSD section length overflow."))?;
        let bytes = self.bytes.get(self.offset..end).ok_or_else(|| {
            invalid("PSD is truncated. Obtain a complete copy of the source file.")
        })?;
        self.offset = end;
        Ok(bytes)
    }
    fn u16(&mut self) -> Result<u16> {
        let v = self.take(2)?;
        Ok(u16::from_be_bytes([v[0], v[1]]))
    }
    fn u32(&mut self) -> Result<u32> {
        let v = self.take(4)?;
        Ok(u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
    }
    fn length(&mut self, large: bool) -> Result<usize> {
        let high = if large { self.u32()? } else { 0 };
        let low = self.u32()?;
        usize::try_from((u64::from(high) << 32) | u64::from(low))
            .map_err(|_| invalid("Photoshop section length exceeds this platform's address space."))
    }
    fn sized_section(&mut self, large: bool) -> Result<Cursor<'a>> {
        let size = self.length(large)?;
        Ok(Self::new(self.take(size)?))
    }
    fn section(&mut self) -> Result<Cursor<'a>> {
        self.sized_section(false)
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }
}
fn rectangle(c: &mut Cursor<'_>, budget: &mut u64) -> Result<()> {
    let top = i64::from(c.u32()? as i32);
    let left = i64::from(c.u32()? as i32);
    let bottom = i64::from(c.u32()? as i32);
    let right = i64::from(c.u32()? as i32);
    let width = right - left;
    let height = bottom - top;
    if width < 0 || height < 0 || width > 30_000 || height > 30_000 {
        return Err(invalid(
            "PSD layer or mask bounds exceed the 30,000 pixel limit.",
        ));
    }
    if width > 0 && height > 0 {
        validate_size(width as u32, height as u32)?;
    }
    *budget += (width * height) as u64;
    crate::document::validate_pixel_budget(*budget)?;
    Ok(())
}
pub(super) fn validate(bytes: &[u8]) -> Result<Prepared<'_>> {
    if bytes.len() > 512 * 1024 * 1024 {
        return Err(invalid("PSD exceeds the 512 MiB file limit."));
    }
    let mut c = Cursor::new(bytes);
    if c.take(4)? != b"8BPS" {
        return Err(invalid("This file is not a Photoshop PSD."));
    }
    let large = match c.u16()? {
        1 => false,
        2 => true,
        _ => {
            return Err(invalid(
                "Unsupported Photoshop document version. Use PSD or PSB.",
            ));
        }
    };
    c.take(6)?;
    let channels = c.u16()?;
    if !(1..=56).contains(&channels) {
        return Err(invalid("PSD channel count is invalid."));
    }
    let height = c.u32()?;
    let width = c.u32()?;
    validate_size(width, height)?;
    if c.u16()? != 8 {
        return Err(invalid(
            "PSD import supports 8-bit channels. Convert the source to 8-bit RGB first.",
        ));
    }
    let color_mode = c.u16()?;
    if !matches!(color_mode, 1 | 3) {
        return Err(invalid(
            "PSD import supports RGB and grayscale, not CMYK or other color modes. Convert the source to RGB first.",
        ));
    }
    c.section()?;
    let resource_length_offset = c.offset;
    let mut resources = c.section()?;
    let mut retained_resources = Vec::new();
    let mut report = ConversionReport::default();
    if channels > if color_mode == 1 { 2 } else { 4 } {
        report.note("Extra Photoshop alpha or spot channels are omitted.");
    }
    while resources.remaining() > 0 {
        if resources.remaining() < 12 {
            return Err(invalid("PSD image resources are truncated."));
        }
        let start = resources.offset;
        if !matches!(
            resources.take(4)?,
            b"8BIM" | b"MeSa" | b"PHUT" | b"AgHg" | b"DCSR"
        ) {
            return Err(invalid("PSD image resource signature is invalid."));
        }
        let id = resources.u16()?;
        let name = usize::from(resources.take(1)?[0]);
        resources.take(name)?;
        if !(name + 1).is_multiple_of(2) {
            resources.take(1)?;
        }
        let len = resources.u32()? as usize;
        resources.take(len)?;
        if !len.is_multiple_of(2) {
            resources.take(1)?;
        }
        if ag_psd::image_resources::RESOURCE_IDS.contains(&id) {
            retained_resources.push(start..resources.offset);
        }
        if id == 1039 {
            report.note("Embedded PSD color profiles are not converted by this importer; pixels are interpreted as sRGB.");
        }
    }
    let decoder_bytes = super::resources::decoder_bytes(
        bytes,
        resource_length_offset,
        c.offset,
        &retained_resources,
    );
    let mut section = c.sized_section(large)?;
    if section.remaining() == 0 {
        return Ok(Prepared {
            report,
            metadata: Vec::new(),
            bytes: decoder_bytes,
        });
    }
    let mut layers = section.sized_section(large)?;
    if layers.remaining() == 0 {
        return Ok(Prepared {
            report,
            metadata: Vec::new(),
            bytes: decoder_bytes,
        });
    }
    let count = (layers.u16()? as i16).unsigned_abs();
    if count > 10_000 {
        return Err(invalid("PSD exceeds the 10,000 layer limit."));
    }
    let mut budget = u64::from(width) * u64::from(height);
    let mut channel_bytes = 0usize;
    let mut folder_depth = 0usize;
    let mut metadata = Vec::new();
    for _ in 0..count {
        let mut entry = super::vector_metadata::Metadata::default();
        let mut divider = false;
        rectangle(&mut layers, &mut budget)?;
        let channels = layers.u16()?;
        if channels > 56 {
            return Err(invalid("PSD layer has too many channels."));
        }
        for _ in 0..channels {
            layers.take(2)?;
            let len = layers.length(large)?;
            channel_bytes = channel_bytes
                .checked_add(len)
                .ok_or_else(|| invalid("PSD channel sizes overflow."))?;
        }
        if layers.take(4)? != b"8BIM" {
            return Err(invalid("PSD layer signature is invalid."));
        }
        let blend = layers.take(4)?;
        if ![
            b"norm", b"mul ", b"scrn", b"over", b"sLit", b"dark", b"lite", b"diff", b"div ",
            b"idiv", b"lbrn", b"lddg", b"hLit", b"vLit", b"lLit", b"pLit", b"hMix", b"smud",
            b"fsub", b"fdiv", b"hue ", b"sat ", b"colr", b"lum ", b"pass",
        ]
        .iter()
        .any(|known| known.as_slice() == blend)
        {
            report.note("Unsupported Photoshop blend modes are converted to Normal.");
        }
        let flags = layers.take(4)?;
        if flags[2] & 1 != 0 {
            report.note("Photoshop transparency locks are not preserved.");
        }
        let mut extra = layers.section()?;
        let mut mask = extra.section()?;
        if mask.remaining() != 0 {
            rectangle(&mut mask, &mut budget)?;
            mask.take(2)?;
            if mask.remaining() >= 18 {
                mask.take(2)?;
                rectangle(&mut mask, &mut budget)?;
            }
        }
        let blending_ranges = extra.section()?;
        if blending_ranges
            .bytes
            .chunks(4)
            .any(|range| range != [0, 0, 255, 255])
        {
            report
                .note("Photoshop Blend If ranges are omitted; affected layer blending may differ.");
        }
        let name = usize::from(extra.take(1)?[0]);
        extra.take(name)?;
        let pad = (4 - (name + 1) % 4) % 4;
        extra.take(pad)?;
        while extra.remaining() >= 12 {
            let signature = extra.take(4)?;
            if !matches!(signature, b"8BIM" | b"8B64") {
                return Err(invalid("Photoshop layer metadata signature is invalid."));
            }
            let key = extra.take(4)?;
            let large_key =
                std::str::from_utf8(key).is_ok_and(ag_psd::additional_info::is_large_key);
            let len = extra.length(signature == b"8B64" || (large && large_key))?;
            let payload = extra.take(len)?;
            super::adjustments::validate_record(key, payload)?;
            entry.read(key, payload)?;
            if entry.unsupported_stroke_blend {
                report.note("Photoshop vector stroke blending is converted to Normal.");
            }
            if key == b"lsct" && payload.len() >= 4 {
                let kind = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                match kind {
                    3 => {
                        divider = true;
                        folder_depth += 1;
                        if folder_depth > 128 {
                            return Err(invalid("PSD folders exceed the 128-level nesting limit."));
                        }
                    }
                    1 | 2 => folder_depth = folder_depth.saturating_sub(1),
                    _ => {}
                }
            }
            if !len.is_multiple_of(2) && extra.remaining() > 0 {
                extra.take(1)?;
            }
            if matches!(key, b"lrFX" | b"lfx2" | b"lmfx") {
                report
                    .note("Photoshop layer effects are not imported; layer appearance may differ.");
            }
            if !matches!(
                key,
                b"luni"
                    | b"lyid"
                    | b"lyvr"
                    | b"lsct"
                    | b"lsdk"
                    | b"iOpa"
                    | b"TySh"
                    | b"Txt2"
                    | b"lrFX"
                    | b"lfx2"
                    | b"lmfx"
                    | b"levl"
                    | b"curv"
                    | b"hue2"
                    | b"blnc"
                    | b"blwh"
                    | b"nvrt"
                    | b"CgEd"
                    | b"vmsk"
                    | b"vsms"
                    | b"SoCo"
                    | b"vogk"
                    | b"vstk"
                    | b"vscg"
            ) {
                report.note(format!(
                    "Photoshop layer metadata '{}' is not preserved as editable metadata.",
                    String::from_utf8_lossy(key)
                ));
            }
        }
        if !divider {
            metadata.push(entry);
        }
    }
    if channel_bytes > layers.remaining() {
        return Err(invalid("PSD layer channel data is truncated."));
    }
    Ok(Prepared {
        report,
        metadata,
        bytes: decoder_bytes,
    })
}
