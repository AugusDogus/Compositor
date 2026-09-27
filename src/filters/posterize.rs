use crate::{Result, document::validate_size, posterize::Posterize};
use image::RgbaImage;

pub(super) fn apply(image: &RgbaImage, settings: Posterize) -> Result<RgbaImage> {
    validate_size(image.width(), image.height())?;
    if settings.levels() == 256 {
        return Ok(image.clone());
    }
    if let Some(result) = crate::render::gpu::color_filter::apply(
        image,
        crate::render::gpu::color_filter::Settings::Posterize(settings),
    )? {
        return Ok(result);
    }
    Ok(crate::posterize::reference(image, settings))
}

#[cfg(test)]
mod tests;
