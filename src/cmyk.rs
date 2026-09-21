use crate::{Result, invalid};
use image::RgbaImage;
use std::io::{Read, Seek};
use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};

/// Preserve the JPEG's four ink channels until Little CMS applies its embedded profile.
/// The ordinary image decoder converts these channels to RGB before exposing pixels.
pub(crate) fn read_jpeg(
    reader: impl Read,
    width: u32,
    height: u32,
    profile: &[u8],
) -> Result<RgbaImage> {
    let mut bytes = Vec::new();
    reader.take(512 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 512 * 1024 * 1024 {
        return Err(invalid("Image exceeds the 512 MiB import limit."));
    }
    let options = DecoderOptions::default()
        .set_strict_mode(false)
        .set_max_width(30_000)
        .set_max_height(30_000);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let failure = |error| {
        invalid(format!(
            "CMYK JPEG could not be decoded: {error}. Re-export the image and try again."
        ))
    };
    decoder.decode_headers().map_err(failure)?;
    if decoder.dimensions() != Some((width as usize, height as usize)) {
        return Err(invalid(
            "The JPEG changed while being imported. Reopen the image.",
        ));
    }
    let space = decoder
        .input_colorspace()
        .ok_or_else(|| invalid("The JPEG color space is missing."))?;
    if !matches!(space, ColorSpace::CMYK | ColorSpace::YCCK) {
        return Err(invalid(
            "The image has a CMYK profile but does not contain CMYK pixels. Assign the correct profile before importing it.",
        ));
    }
    decoder.set_options(options.jpeg_set_out_colorspace(space));
    let mut inks = decoder.decode().map_err(failure)?;
    if inks.len() != width as usize * height as usize * 4 {
        return Err(invalid(
            "The CMYK JPEG pixel count does not match its dimensions.",
        ));
    }
    for pixel in inks.chunks_exact_mut(4) {
        if space == ColorSpace::YCCK {
            let y = pixel[0] as f64;
            let cb = pixel[1] as f64 - 128.;
            let cr = pixel[2] as f64 - 128.;
            // Adobe YCCK stores YCbCr of the non-inverted CMY channels, plus inverted K.
            pixel[0] = (y + 1.402 * cr).round().clamp(0., 255.) as u8;
            pixel[1] = (y - 0.344136 * cb - 0.714136 * cr).round().clamp(0., 255.) as u8;
            pixel[2] = (y + 1.772 * cb).round().clamp(0., 255.) as u8;
            pixel[3] = 255 - pixel[3];
        } else {
            for value in pixel {
                *value = 255 - *value;
            }
        }
    }
    convert(&inks, width, height, profile, lcms2::PixelFormat::CMYK_8)
}

pub(crate) fn read_tiff(reader: impl Read + Seek, profile: &[u8]) -> Result<RgbaImage> {
    let failure = |error| {
        invalid(format!(
            "CMYK TIFF could not be decoded: {error}. Re-export the image and try again."
        ))
    };
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size = 800_000_000;
    let mut decoder = tiff::decoder::Decoder::new(reader)
        .map_err(failure)?
        .with_limits(limits);
    let (width, height) = decoder.dimensions().map_err(failure)?;
    crate::document::validate_size(width, height)?;
    let orientation = decoder
        .find_tag(tiff::tags::Tag::Orientation)
        .map_err(failure)?
        .map(|v| v.into_u16().map_err(failure))
        .transpose()?
        .unwrap_or(1);
    let orientation = u8::try_from(orientation)
        .ok()
        .and_then(image::metadata::Orientation::from_exif)
        .ok_or_else(|| {
            invalid(
                "The TIFF orientation is invalid. Re-export the image with a valid orientation.",
            )
        })?;
    let color = decoder.colortype().map_err(failure)?;
    let (bits, alpha) = match color {
        tiff::ColorType::CMYK(bits @ (8 | 16)) => (bits, false),
        tiff::ColorType::CMYKA(bits @ (8 | 16)) => (bits, true),
        _ => {
            return Err(invalid(
                "This profiled TIFF must contain 8-bit or 16-bit CMYK pixels. Re-export it as RGB or CMYK.",
            ));
        }
    };
    let associated = alpha
        && decoder
            .find_tag(tiff::tags::Tag::ExtraSamples)
            .map_err(failure)?
            .map(|v| v.into_u16_vec().map_err(failure))
            .transpose()?
            .is_some_and(|samples| samples.first() == Some(&1));
    let count = width as usize * height as usize;
    let channels = if alpha { 5 } else { 4 };
    let (inks, coverage, format) = match decoder.read_image().map_err(failure)? {
        tiff::decoder::DecodingResult::U8(mut samples) if bits == 8 => {
            if samples.len() != count * channels {
                return Err(invalid(
                    "The CMYK TIFF pixel count does not match its dimensions.",
                ));
            }
            let coverage = if alpha {
                strip_alpha(&mut samples, 255, associated)?
            } else {
                Vec::new()
            };
            (samples, coverage, lcms2::PixelFormat::CMYK_8)
        }
        tiff::decoder::DecodingResult::U16(mut samples) if bits == 16 => {
            if samples.len() != count * channels {
                return Err(invalid(
                    "The CMYK TIFF pixel count does not match its dimensions.",
                ));
            }
            let coverage = if alpha {
                strip_alpha(&mut samples, 65535, associated)?
            } else {
                Vec::new()
            };
            (
                samples.into_iter().flat_map(u16::to_ne_bytes).collect(),
                coverage,
                lcms2::PixelFormat::CMYK_16,
            )
        }
        _ => {
            return Err(invalid(
                "The CMYK TIFF pixel format does not match its metadata.",
            ));
        }
    };
    let mut pixels = convert(&inks, width, height, profile, format)?;
    for (pixel, alpha) in pixels.pixels_mut().zip(coverage) {
        pixel[3] = alpha;
    }
    let mut image = image::DynamicImage::ImageRgba8(pixels);
    image.apply_orientation(orientation);
    Ok(image.into_rgba8())
}

/// Compact ink samples in place while retaining alpha at the editor's 8-bit depth.
fn strip_alpha<T>(samples: &mut Vec<T>, maximum: u32, associated: bool) -> Result<Vec<u8>>
where
    T: Copy + Into<u32> + TryFrom<u32>,
{
    let count = samples.len() / 5;
    let mut coverage = Vec::with_capacity(count);
    for index in 0..count {
        let alpha = samples[index * 5 + 4].into();
        coverage.push(((alpha * 255 + maximum / 2) / maximum) as u8);
        for channel in 0..4 {
            let sample = samples[index * 5 + channel];
            samples[index * 4 + channel] = if associated {
                let straight = if alpha == 0 {
                    0
                } else {
                    ((sample.into() * maximum + alpha / 2) / alpha).min(maximum)
                };
                // The clamped value fits the input sample's 8-bit or 16-bit range.
                T::try_from(straight).map_err(|_| invalid("The normalized CMYK TIFF channel exceeds its bit depth. Re-export the image."))?
            } else {
                sample
            };
        }
    }
    samples.truncate(count * 4);
    Ok(coverage)
}

fn convert(
    inks: &[u8],
    width: u32,
    height: u32,
    profile: &[u8],
    format: lcms2::PixelFormat,
) -> Result<RgbaImage> {
    let source = lcms2::Profile::new_icc(profile).map_err(|error| {
        invalid(format!(
            "The CMYK color profile is damaged: {error}. Re-export the image with a valid profile."
        ))
    })?;
    let target = lcms2::Profile::new_srgb();
    let conversion = lcms2::Transform::<u8, u8>::new(&source, format, &target, lcms2::PixelFormat::RGBA_8, lcms2::Intent::Perceptual)
        .map_err(|error| invalid(format!("The CMYK profile cannot be converted to sRGB: {error}. Convert the image to sRGB before importing it.")))?;
    let mut pixels = RgbaImage::new(width, height);
    conversion.transform_pixels(inks, pixels.as_mut());
    for pixel in pixels.pixels_mut() {
        pixel[3] = 255;
    }
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alpha_normalization_compacts_samples_and_preserves_16_bit_inks() {
        let mut samples = vec![10_u8, 20, 30, 40, 128, 200, 255, 100, 90, 0];
        assert_eq!(strip_alpha(&mut samples, 255, true).unwrap(), vec![128, 0]);
        assert_eq!(samples, vec![20, 40, 60, 80, 0, 0, 0, 0]);
        let mut samples = vec![
            16384_u16, 8192, 0, 4096, 32768, 65535, 65535, 65535, 65535, 65535,
        ];
        assert_eq!(
            strip_alpha(&mut samples, 65535, true).unwrap(),
            vec![128, 255]
        );
        assert_eq!(
            samples,
            vec![32768, 16384, 0, 8192, 65535, 65535, 65535, 65535]
        );
        let mut samples = vec![10_u8, 20, 30, 40, 128];
        assert_eq!(strip_alpha(&mut samples, 255, false).unwrap(), vec![128]);
        assert_eq!(samples, vec![10, 20, 30, 40]);
    }
}
