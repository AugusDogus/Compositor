use crate::{Result, adjustment::PhotoFilter, document::validate_size};
use image::RgbaImage;
use rayon::prelude::*;

pub(super) fn apply(image: &RgbaImage, settings: PhotoFilter) -> Result<RgbaImage> {
    settings.validate()?;
    validate_size(image.width(), image.height())?;
    if settings.identity() {
        return Ok(image.clone());
    }
    if let Some(result) = crate::render::gpu::photo_filter::apply(image, settings)? {
        return Ok(result);
    }
    Ok(reference(image, settings))
}

pub(crate) fn reference(image: &RgbaImage, settings: PhotoFilter) -> RgbaImage {
    let mut output = image.clone();
    let bytes: &mut [u8] = output.as_mut();
    bytes.par_chunks_exact_mut(4).for_each(|pixel| {
        pixel.copy_from_slice(&settings.pixel([pixel[0], pixel[1], pixel[2], pixel[3]]));
    });
    output
}

#[cfg(test)]
mod tests;
