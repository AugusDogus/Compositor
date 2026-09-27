use crate::{Result, document::validate_size, threshold::Threshold};
use image::RgbaImage;

pub(super) fn apply(image: &RgbaImage, settings: Threshold) -> Result<RgbaImage> {
    validate_size(image.width(), image.height())?;
    if let Some(result) = crate::render::gpu::color_filter::apply(
        image,
        crate::render::gpu::color_filter::Settings::Threshold(settings),
    )? {
        return Ok(result);
    }
    Ok(crate::threshold::reference(image, settings))
}

#[cfg(test)]
mod tests;
