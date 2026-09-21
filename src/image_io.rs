use crate::{
    Result,
    document::{Document, Layer, LayerContent, validate_size},
    invalid, render,
};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbaImage};
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Cursor, Read, Seek, Write},
    path::Path,
    sync::Arc,
};

pub fn read_image(path: &Path) -> Result<RgbaImage> {
    let metadata = path.metadata()?;
    if !metadata.is_file() || metadata.len() > 512 * 1024 * 1024 {
        return Err(invalid(
            "Image must be a regular file no larger than 512 MiB.",
        ));
    }
    decode_image(|| Ok(BufReader::new(File::open(path)?)))
}

/// Decode an encoded clipboard image with the same color and orientation handling as imports.
pub fn read_encoded(bytes: &[u8]) -> Result<RgbaImage> {
    if bytes.len() > 512 * 1024 * 1024 {
        return Err(invalid("Image exceeds the 512 MiB import limit."));
    }
    decode_image(|| Ok(Cursor::new(bytes)))
}

// TIFF metadata and CMYK channels need independent decoder passes over the same source.
fn decode_image<R: BufRead + Seek>(open: impl Fn() -> Result<R>) -> Result<RgbaImage> {
    static HEIF: std::sync::Once = std::sync::Once::new();
    HEIF.call_once(libheif_rs::integration::image::register_all_decoding_hooks);
    let mut input = open()?;
    // image's format guesser recognizes Classic TIFF but omits BigTIFF's magic.
    let big_tiff = matches!(input.fill_buf()?.get(..4), Some(b"II+\0") | Some(b"MM\0+"));
    let mut reader = ImageReader::new(input).with_guessed_format()?;
    if big_tiff {
        reader.set_format(ImageFormat::Tiff);
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(30_000);
    limits.max_image_height = Some(30_000);
    limits.max_alloc = Some(800_000_000);
    reader.limits(limits);
    let format = reader.format();
    // Read TIFF profiles before the image wrapper rejects CMYKA or drops metadata
    // whose allocation is larger than the pixel buffer.
    let (tiff_profile, photometric) = if format == Some(ImageFormat::Tiff) {
        tiff_metadata(open()?)?
    } else {
        (None, 0)
    };
    if matches!(photometric, 8 | 9) {
        return crate::lab_tiff::read(open()?, photometric, tiff_profile.as_deref());
    }
    if let Some(profile) = &tiff_profile
        && profile.get(16..20) == Some(b"CMYK")
    {
        return crate::cmyk::read_tiff(open()?, profile);
    }
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    validate_size(width, height)?;
    let orientation = decoder.orientation()?;
    let profile = if format == Some(ImageFormat::Tiff) {
        tiff_profile
    } else {
        decoder.icc_profile()?
    };
    if format == Some(ImageFormat::Jpeg)
        && let Some(profile) = &profile
        && profile.get(16..20) == Some(b"CMYK")
    {
        drop(decoder);
        let pixels = crate::cmyk::read_jpeg(open()?, width, height, profile)?;
        let mut image = DynamicImage::ImageRgba8(pixels);
        image.apply_orientation(orientation);
        return Ok(image.into_rgba8());
    }
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    let mut pixels = image.into_rgba8();
    if let Some(profile) = profile {
        convert_to_srgb(&mut pixels, &profile)?;
    }
    Ok(pixels)
}

fn tiff_metadata(reader: impl Read + Seek) -> Result<(Option<Vec<u8>>, u16)> {
    let failure = |error| {
        invalid(format!(
            "The TIFF color profile could not be read: {error}. Re-export the image with a valid profile."
        ))
    };
    let mut decoder = tiff::decoder::Decoder::new(reader).map_err(failure)?;
    let profile = decoder
        .find_tag(tiff::tags::Tag::IccProfile)
        .map_err(failure)?
        .map(|value| value.into_u8_vec().map_err(failure))
        .transpose()?;
    let photometric = decoder
        .find_tag(tiff::tags::Tag::PhotometricInterpretation)
        .map_err(failure)?
        .map(|value| value.into_u16().map_err(failure))
        .transpose()?
        .unwrap_or(1);
    Ok((profile, photometric))
}

fn convert_to_srgb(pixels: &mut RgbaImage, icc: &[u8]) -> Result<()> {
    let source = lcms2::Profile::new_icc(icc).map_err(|e| {
        invalid(format!(
            "Image color profile is damaged: {e}. Convert the image to sRGB before importing it."
        ))
    })?;
    let target = lcms2::Profile::new_srgb();
    if source.color_space() == lcms2::ColorSpaceSignature::GrayData {
        if pixels.pixels().any(|p| p[0] != p[1] || p[0] != p[2]) {
            return Err(invalid(
                "The image has a grayscale profile but contains RGB colors. Assign the correct color profile before importing it.",
            ));
        }
        let gray: Vec<u8> = pixels.pixels().flat_map(|p| [p[0], p[3]]).collect();
        let conversion = lcms2::Transform::<u8, u8>::new_flags(
            &source, lcms2::PixelFormat::GRAYA_8,
            &target, lcms2::PixelFormat::RGBA_8,
            lcms2::Intent::Perceptual, lcms2::Flags::COPY_ALPHA,
        ).map_err(|e| invalid(format!("Image grayscale profile could not be converted to sRGB: {e}. Convert the image to sRGB before importing it.")))?;
        conversion.transform_pixels(&gray, pixels.as_mut());
        return Ok(());
    }
    let conversion = lcms2::Transform::<u8, u8>::new_flags(&source, lcms2::PixelFormat::RGBA_8,
        &target, lcms2::PixelFormat::RGBA_8, lcms2::Intent::Perceptual, lcms2::Flags::COPY_ALPHA)
        .map_err(|e| invalid(format!("Image color profile could not be converted to sRGB: {e}. Convert it to an RGB image before importing it.")))?;
    conversion.transform_in_place(pixels.as_mut());
    Ok(())
}

pub fn import(path: &Path) -> Result<Layer> {
    let pixels = read_image(path)?;
    let name = path
        .file_stem()
        .map_or_else(|| "Image".into(), |s| s.to_string_lossy().into_owned());
    let mut layer = Layer::blank(name, pixels.width(), pixels.height());
    layer.content = LayerContent::Raster(Some(Arc::new(pixels)));
    Ok(layer)
}

pub fn encode_jpeg(pixels: &RgbaImage, resolution: f64, quality: u8) -> Result<Vec<u8>> {
    encode_jpeg_with_options(
        pixels,
        resolution,
        JpegOptions {
            quality,
            background: [255; 3],
        },
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JpegOptions {
    pub quality: u8,
    pub background: [u8; 3],
}

pub fn encode_jpeg_with_options(
    pixels: &RgbaImage,
    resolution: f64,
    options: JpegOptions,
) -> Result<Vec<u8>> {
    if options.quality > 100 || !(1. ..=9600.).contains(&resolution) {
        return Err(invalid(
            "JPEG quality must be 0 to 100 and resolution must be 1 to 9600 pixels per inch.",
        ));
    }
    let rgb = image::RgbImage::from_fn(pixels.width(), pixels.height(), |x, y| {
        let pixel = pixels[(x, y)];
        let alpha = pixel[3] as f64 / 255.;
        image::Rgb(std::array::from_fn(|c| {
            (pixel[c] as f64 * alpha + f64::from(options.background[c]) * (1. - alpha)).round()
                as u8
        }))
    });
    let mut bytes = Vec::new();
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, options.quality);
    encoder.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(
        resolution.round() as u16
    ));
    encoder.encode_image(&rgb)?;
    Ok(bytes)
}

/// Write the exact encoded preview, without recompositing or re-encoding it.
pub fn export_encoded(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub fn export(document: &Document, path: &Path, quality: u8) -> Result<()> {
    document.validate()?;
    validate_size(document.width, document.height)?;
    let format = ImageFormat::from_path(path)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Tiff | ImageFormat::WebP
    ) {
        return Err(invalid(
            "Choose a PNG, JPEG, TIFF, or WebP filename for export.",
        ));
    }
    let pixels = render::render(document, document.width, document.height);
    let directory = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        if format == ImageFormat::Jpeg {
            writer.write_all(&encode_jpeg(&pixels, document.resolution, quality)?)?;
        } else if format == ImageFormat::Png {
            let mut encoder = png::Encoder::new(&mut writer, pixels.width(), pixels.height());
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let per_meter = (document.resolution / 0.0254).round() as u32;
            encoder.set_pixel_dims(Some(png::PixelDimensions {
                xppu: per_meter,
                yppu: per_meter,
                unit: png::Unit::Meter,
            }));
            let mut png = encoder
                .write_header()
                .map_err(|e| invalid(format!("PNG header could not be encoded: {e}")))?;
            png.write_image_data(pixels.as_raw())
                .map_err(|e| invalid(format!("PNG pixels could not be encoded: {e}")))?;
            png.finish()
                .map_err(|e| invalid(format!("PNG export could not be finalized: {e}")))?;
        } else {
            DynamicImage::ImageRgba8(pixels).write_to(&mut writer, format)?;
        }
        writer.flush()?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jpeg_preview_and_export_use_identical_quality_and_white_transparency() {
        let mut doc = Document::new(32, 32).unwrap();
        doc.layers[0].content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(32, 32, |x, y| {
                if x < 8 {
                    image::Rgba([0, 255, 0, 0])
                } else {
                    image::Rgba([
                        (x * 19 + y * 17) as u8,
                        (x * 37 + y * 23) as u8,
                        (x * 13 + y * 41) as u8,
                        255,
                    ])
                }
            }))));
        let pixels = render::render(&doc, 32, 32);
        let low = encode_jpeg(&pixels, 72., 10).unwrap();
        let high = encode_jpeg(&pixels, 72., 95).unwrap();
        assert!(high.len() > low.len());
        let preview = image::load_from_memory(&high).unwrap().into_rgb8();
        assert_eq!(preview[(0, 0)], image::Rgb([255, 255, 255]));
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Preview.jpg");
        export(&doc, &path, 95).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), high);
        // The original sheet permits 0%; the encoder maps that to its minimum quality.
        assert!(image::load_from_memory(&encode_jpeg(&pixels, 72., 0).unwrap()).is_ok());
        assert!(encode_jpeg(&pixels, 72., 101).is_err());
    }
    #[test]
    fn jpeg_matte_composites_transparency_and_encoded_export_is_atomic() {
        let pixels = RgbaImage::from_fn(32, 16, |x, _| {
            image::Rgba([200, 100, 50, if x < 16 { 0 } else { 128 }])
        });
        let bytes = encode_jpeg_with_options(
            &pixels,
            144.,
            JpegOptions {
                quality: 100,
                background: [20, 60, 120],
            },
        )
        .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().into_rgb8();
        for (x, expected) in [(4, [20_i16, 60, 120]), (28, [110, 80, 85])] {
            for (actual, expected) in decoded[(x, 8)].0.into_iter().zip(expected) {
                assert!((i16::from(actual) - expected).abs() <= 2);
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preview.jpg");
        std::fs::write(&path, b"old image").unwrap();
        export_encoded(&path, &bytes).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let directory_target = dir.path().join("folder.jpg");
        std::fs::create_dir(&directory_target).unwrap();
        std::fs::write(directory_target.join("keep"), b"preserved").unwrap();
        assert!(export_encoded(&directory_target, &bytes).is_err());
        assert_eq!(
            std::fs::read(directory_target.join("keep")).unwrap(),
            b"preserved"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
    #[test]
    fn srgb_conversion_preserves_transparency() {
        let mut pixels = RgbaImage::from_pixel(1, 1, image::Rgba([128, 64, 32, 123]));
        convert_to_srgb(&mut pixels, &lcms2::Profile::new_srgb().icc().unwrap()).unwrap();
        assert_eq!(pixels[(0, 0)], image::Rgba([128, 64, 32, 123]));
    }
    #[test]
    fn grayscale_profile_converts_tone_and_preserves_alpha_on_import() {
        use std::borrow::Cow;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("linear-gray.png");
        let profile = lcms2::Profile::new_gray(lcms2::CIExyY::d50(), &lcms2::ToneCurve::new(1.))
            .unwrap()
            .icc()
            .unwrap();
        let mut info = png::Info::with_size(1, 1);
        info.color_type = png::ColorType::GrayscaleAlpha;
        info.bit_depth = png::BitDepth::Eight;
        info.icc_profile = Some(Cow::Owned(profile.clone()));
        let encoder = png::Encoder::with_info(File::create(&path).unwrap(), info).unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[128, 123]).unwrap();
        writer.finish().unwrap();
        let image = read_image(&path).unwrap();
        let pixel = image[(0, 0)];
        assert!((187..=189).contains(&pixel[0]));
        assert_eq!(pixel[0], pixel[1]);
        assert_eq!(pixel[1], pixel[2]);
        assert_eq!(pixel[3], 123);
        let mut invalid_image = RgbaImage::from_pixel(1, 1, image::Rgba([128, 50, 20, 255]));
        assert!(convert_to_srgb(&mut invalid_image, &profile).is_err());
    }

    #[test]
    fn tiff_profiles_larger_than_pixel_data_are_applied() {
        use image::ImageEncoder;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("linear-gray.tiff");
        let profile = lcms2::Profile::new_gray(lcms2::CIExyY::d50(), &lcms2::ToneCurve::new(1.))
            .unwrap()
            .icc()
            .unwrap();
        let mut encoder = image::codecs::tiff::TiffEncoder::new(File::create(&path).unwrap());
        encoder.set_icc_profile(profile).unwrap();
        encoder
            .write_image(&[128], 1, 1, image::ExtendedColorType::L8)
            .unwrap();
        let pixel = read_image(&path).unwrap()[(0, 0)];
        assert!((187..=189).contains(&pixel[0]));
        assert_eq!(pixel[0], pixel[1]);
        assert_eq!(pixel[1], pixel[2]);
        assert_eq!(pixel[3], 255);
    }
    #[test]
    fn png_export_keeps_resolution_and_does_not_change_document() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("export.png");
        let mut doc = Document::new(2, 2).unwrap();
        doc.resolution = 300.;
        crate::edits::fill(&mut doc, [255, 0, 0, 128], false, false).unwrap();
        let before = doc.clone();
        export(&doc, &path, 90).unwrap();
        let reader = png::Decoder::new(BufReader::new(File::open(&path).unwrap()))
            .read_info()
            .unwrap();
        let density = reader.info().pixel_dims.unwrap();
        assert_eq!(density.unit, png::Unit::Meter);
        assert_eq!(density.xppu, 11811);
        assert_eq!(doc, before);
        assert_eq!(
            read_image(&path).unwrap()[(0, 0)],
            image::Rgba([255, 0, 0, 128])
        );
    }
}
