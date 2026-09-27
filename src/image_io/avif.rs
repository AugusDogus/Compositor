//! Still AVIF encoding through the same native codec stack used for HEIF import.
use crate::{Result, invalid};
use image::RgbaImage;
use libheif_rs::{
    Channel, ColorProfileRaw, ColorSpace, CompressionFormat, EncoderParameterValue, EncoderQuality,
    HeifContext, Image, LibHeif, RgbChroma, color_profile_types,
};

pub(super) fn encode(pixels: &RgbaImage, quality: u8) -> Result<Vec<u8>> {
    if quality > 100 {
        return Err(invalid(
            "AVIF quality must be 0 to 100. No file was written.",
        ));
    }
    encode_native(pixels, quality).map_err(|error| {
        invalid(format!(
            "AVIF encoding failed: {error}. The previous destination is unchanged. Retry with PNG or reinstall Compositor if its AOM codec is missing."
        ))
    })
}

fn encode_native(pixels: &RgbaImage, quality: u8) -> Result<Vec<u8>> {
    let heif = LibHeif::new();
    // Use the bundled AOM codec consistently across distributions. Avoid changing
    // encoder behavior based on whichever optional host plugin is installed.
    let descriptor = heif
        .encoder_descriptors(32, Some(CompressionFormat::Av1), None)
        .into_iter()
        .find(|descriptor| descriptor.id() == "aom")
        .ok_or_else(|| invalid("The libheif AOM encoder is unavailable"))?;
    let mut encoder = heif.encoder(descriptor).map_err(image::ImageError::from)?;
    encoder
        .set_quality(EncoderQuality::Lossy(quality))
        .map_err(image::ImageError::from)?;
    for (name, value) in [
        ("speed", EncoderParameterValue::Int(6)),
        ("threads", EncoderParameterValue::Int(4)),
        ("chroma", EncoderParameterValue::String("444".into())),
        ("lossless-alpha", EncoderParameterValue::Bool(true)),
    ] {
        encoder
            .set_parameter_value(name, value)
            .map_err(image::ImageError::from)?;
    }
    let (width, height) = pixels.dimensions();
    let mut image = Image::new(width, height, ColorSpace::Rgb(RgbChroma::Rgba))
        .map_err(image::ImageError::from)?;
    image
        .create_plane(Channel::Interleaved, width, height, 8)
        .map_err(image::ImageError::from)?;
    let plane = image
        .planes_mut()
        .interleaved
        .ok_or_else(|| invalid("The AVIF encoder did not allocate an RGBA plane"))?;
    let row_bytes = width as usize * 4;
    for (source, destination) in pixels
        .as_raw()
        .chunks_exact(row_bytes)
        .zip(plane.data.chunks_exact_mut(plane.stride))
    {
        destination[..row_bytes].copy_from_slice(source);
    }
    image.set_premultiplied_alpha(false);
    let profile = lcms2::Profile::new_srgb()
        .icc()
        .map_err(|error| invalid(format!("Could not encode the sRGB profile: {error}")))?;
    image
        .set_color_profile_raw(&ColorProfileRaw::new(color_profile_types::PROF, profile))
        .map_err(image::ImageError::from)?;
    let mut context = HeifContext::new().map_err(image::ImageError::from)?;
    context
        .encode_image(&image, &mut encoder, None)
        .map_err(image::ImageError::from)?;
    context
        .write_to_bytes()
        .map_err(image::ImageError::from)
        .map_err(Into::into)
}
