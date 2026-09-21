//! Lab TIFF channel decoding and color management. The TIFF crate rejects signed
//! CIELab a/b channels. A private copy marks these as raw RGB samples for its
//! decompressor; only Little CMS interprets their colors, using the original Lab
//! encoding and profile. The file on disk is never modified.
mod header;
use crate::{Result, invalid};
use image::RgbaImage;
use lcms2::{CIELabExt, CIEXYZExt};
use std::io::{Cursor, Read, Seek};

pub(crate) fn read(
    reader: impl Read + Seek,
    photometric: u16,
    profile: Option<&[u8]>,
) -> Result<RgbaImage> {
    let mut bytes = Vec::new();
    reader.take(512 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 512 * 1024 * 1024 {
        return Err(invalid("Image exceeds the 512 MiB import limit."));
    }
    header::raw_channels(&mut bytes, photometric)?;
    let failure = |e| {
        invalid(format!(
            "Lab TIFF could not be decoded: {e}. Re-export the image with valid Lab metadata."
        ))
    };
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size = 800_000_000;
    let mut decoder = tiff::decoder::Decoder::new(Cursor::new(bytes))
        .map_err(failure)?
        .with_limits(limits);
    let (width, height) = decoder.dimensions().map_err(failure)?;
    crate::document::validate_size(width, height)?;
    let (bits, channels) = match decoder.colortype().map_err(failure)? {
        tiff::ColorType::RGB(bits @ (8 | 16)) => (bits, 3),
        tiff::ColorType::RGBA(bits @ (8 | 16)) => (bits, 4),
        _ => {
            return Err(invalid(
                "Lab TIFF requires three 8-bit or 16-bit color channels and optional alpha. Re-export as RGB TIFF.",
            ));
        }
    };
    let orientation = decoder
        .find_tag(tiff::tags::Tag::Orientation)
        .map_err(failure)?
        .map(|v| v.into_u16().map_err(failure))
        .transpose()?
        .unwrap_or(1);
    let orientation = u8::try_from(orientation)
        .ok()
        .and_then(image::metadata::Orientation::from_exif)
        .ok_or_else(|| invalid("The Lab TIFF orientation is invalid. Re-export the image."))?;
    let associated = decoder
        .find_tag(tiff::tags::Tag::ExtraSamples)
        .map_err(failure)?
        .map(|v| v.into_u16_vec().map_err(failure))
        .transpose()?
        .is_some_and(|v| v.first() == Some(&1));
    let white = if profile.is_none() {
        decoder
            .find_tag(tiff::tags::Tag::Unknown(318))
            .map_err(failure)?
            .map(white_point)
            .transpose()?
    } else {
        None
    };
    let mut samples = tiff::decoder::DecodingResult::U8(Vec::new());
    let layout = decoder
        .read_image_to_buffer(&mut samples)
        .map_err(failure)?;
    let count = width as usize * height as usize;
    let samples: Vec<u16> = match samples {
        tiff::decoder::DecodingResult::U8(v) if bits == 8 => v.into_iter().map(u16::from).collect(),
        tiff::decoder::DecodingResult::U16(v) if bits == 16 => v,
        _ => {
            return Err(invalid(
                "Lab TIFF samples do not match their declared bit depth.",
            ));
        }
    };
    if samples.len() != count * channels || (layout.planes != 1 && layout.planes != channels) {
        return Err(invalid(
            "Lab TIFF sample count does not match its dimensions. The image may be incomplete.",
        ));
    }
    let source = if let Some(icc) = profile {
        let p = lcms2::Profile::new_icc(icc)
            .map_err(|e| invalid(format!("The Lab color profile is damaged: {e}")))?;
        if p.color_space() != lcms2::ColorSpaceSignature::LabData {
            return Err(invalid(
                "This TIFF contains Lab channels but has a different color-space profile. Assign a Lab profile before importing it.",
            ));
        }
        p
    } else {
        lcms2::Profile::new_lab4_context(lcms2::GlobalContext::default(), lcms2::CIExyY::d50())
            .map_err(|e| {
                invalid(format!(
                    "Could not initialize standard D50 Lab color conversion: {e}"
                ))
            })?
    };
    let convert = lcms2::Transform::<[f64; 3], [u8; 3]>::new(
        &source,
        lcms2::PixelFormat::Lab_DBL,
        &lcms2::Profile::new_srgb(),
        lcms2::PixelFormat::RGB_8,
        lcms2::Intent::Perceptual,
    )
    .map_err(|e| {
        invalid(format!(
            "The Lab profile cannot be converted to sRGB: {e}. Re-export as sRGB TIFF."
        ))
    })?;
    let max = if bits == 8 { 255. } else { 65535. };
    // Adobe Photoshop TIFF Technical Notes (2002), pp. 12-13: ICCLab
    // retains the ICC v2 lightness encoding; CIELab uses the full u16 range.
    let lightness_max = if bits == 16 && photometric == 9 {
        65280.
    } else {
        max
    };
    let component = |i: usize, c: usize| {
        samples[if layout.planes == 1 {
            i * channels + c
        } else {
            c * count + i
        }]
    };
    // Convert in bounded batches rather than allocating a full f64 Lab image.
    let mut output = RgbaImage::new(width, height);
    for start in (0..count).step_by(4096) {
        let end = (start + 4096).min(count);
        let mut colors = Vec::with_capacity(end - start);
        let mut alphas = Vec::with_capacity(end - start);
        for i in start..end {
            let alpha = if channels == 4 {
                f64::from(component(i, 3)) / max
            } else {
                1.
            };
            let ab = |sample: u16| {
                if photometric == 8 {
                    if bits == 8 {
                        f64::from(sample as u8 as i8)
                    } else {
                        f64::from(sample as i16) / 256.
                    }
                } else if bits == 8 {
                    f64::from(sample) - 128.
                } else {
                    f64::from(sample) / 256. - 128.
                }
            };
            let mut lab = [
                f64::from(component(i, 0)) * 100. / lightness_max,
                ab(component(i, 1)),
                ab(component(i, 2)),
            ];
            if associated {
                lab = if alpha > 0. {
                    lab.map(|v| v / alpha)
                } else {
                    [0.; 3]
                };
            }
            if let Some(white) = &white {
                let xyz = lcms2::CIELab {
                    L: lab[0],
                    a: lab[1],
                    b: lab[2],
                }
                .to_xyz(white);
                let adapted=xyz.adapt_to_illuminant(white,lcms2::CIEXYZ::d50())
                    .ok_or_else(||invalid("The Lab TIFF white point cannot be adapted to sRGB. Re-export with a valid color profile."))?
                    .to_lab(lcms2::CIEXYZ::d50());
                lab = [adapted.L, adapted.a, adapted.b];
            }
            colors.push(lab);
            alphas.push((alpha * 255.).round() as u8);
        }
        let mut rgb = vec![[0; 3]; colors.len()];
        convert.transform_pixels(&colors, &mut rgb);
        let raw: &mut [u8] = output.as_mut();
        for (pixel, (rgb, alpha)) in raw[start * 4..end * 4]
            .chunks_exact_mut(4)
            .zip(rgb.into_iter().zip(alphas))
        {
            pixel.copy_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
        }
    }
    let mut image = image::DynamicImage::ImageRgba8(output);
    image.apply_orientation(orientation);
    Ok(image.into_rgba8())
}

fn white_point(value: tiff::decoder::ifd::Value) -> Result<lcms2::CIEXYZ> {
    use tiff::decoder::ifd::Value;
    let Value::List(values) = value else {
        return Err(invalid(
            "The Lab TIFF white point must contain two rational coordinates.",
        ));
    };
    if values.len() != 2 {
        return Err(invalid(
            "The Lab TIFF white point must contain two coordinates.",
        ));
    }
    let mut xy = [0.; 2];
    for (out, value) in xy.iter_mut().zip(values) {
        let Value::Rational(n, d) = value else {
            return Err(invalid(
                "The Lab TIFF white point coordinates must be rational numbers.",
            ));
        };
        if d == 0 {
            return Err(invalid(
                "The Lab TIFF white point contains a zero denominator.",
            ));
        }
        *out = f64::from(n) / f64::from(d);
    }
    if xy.iter().any(|v| *v <= 0. || *v >= 1.) || xy[0] + xy[1] > 1. {
        return Err(invalid(
            "The Lab TIFF white point is outside the valid chromaticity range.",
        ));
    }
    Ok(lcms2::CIEXYZ::from(lcms2::CIExyY {
        x: xy[0],
        y: xy[1],
        Y: 1.,
    }))
}

#[cfg(test)]
mod tests;
