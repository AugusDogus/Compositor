use crate::{Result, document::validate_size, selective_color::SelectiveColor};
use image::RgbaImage;

pub(super) fn apply(image: &RgbaImage, settings: SelectiveColor) -> Result<RgbaImage> {
    settings.validate()?;
    validate_size(image.width(), image.height())?;
    if settings.identity() {
        return Ok(image.clone());
    }
    if let Some(result) = crate::render::gpu::color_filter::apply(
        image,
        crate::render::gpu::color_filter::Settings::SelectiveColor(settings),
    )? {
        return Ok(result);
    }
    Ok(crate::selective_color::reference(image, settings))
}

#[cfg(test)]
mod tests;
