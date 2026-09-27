use crate::{Result, document::validate_size, vibrance::Vibrance};
use image::RgbaImage;

pub(super) fn apply(image: &RgbaImage, settings: Vibrance) -> Result<RgbaImage> {
    validate_size(image.width(), image.height())?;
    if settings.identity() {
        return Ok(image.clone());
    }
    if let Some(result) = crate::render::gpu::color_filter::apply(
        image,
        crate::render::gpu::color_filter::Settings::Vibrance(settings),
    )? {
        return Ok(result);
    }
    Ok(crate::vibrance::reference(image, settings))
}

#[cfg(test)]
mod tests;
