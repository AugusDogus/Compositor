pub mod adjustment;
pub mod background;
pub mod blend;
pub mod brush;
pub mod canvas_size;
pub mod clipboard;
pub mod clipping;
mod cmyk;
pub mod crop;
pub mod distort;
pub mod document;
pub mod edits;
pub mod effects;
pub mod filters;
pub mod floating;
pub mod geometry;
mod gpu;
pub mod gradient;
pub mod guides;
pub mod histogram;
pub mod hue_band;
pub mod hue_sample;
pub mod image_io;
pub mod image_resize;
mod inference;
mod lab_tiff;
pub mod layer_ops;
pub mod levels_sample;
pub mod native_clipboard;
mod native_pixels;
pub mod object_selection;
pub mod palette;
pub mod pixel_adjustment;
pub mod project;
pub mod psd;
mod raster_extent;
pub mod raw;
pub mod render;
mod resample;
pub mod selection;
pub mod selection_coverage;
mod selection_geometry;
pub mod session;
pub mod text;
pub mod thumbnail;
pub mod transform;
pub mod wand;
pub mod warp;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("File operation failed: {0}. The current document is preserved.")]
    Io(#[from] std::io::Error),
    #[error("Image could not be decoded or encoded: {0}.")]
    Image(#[from] image::ImageError),
    #[error("Project metadata is invalid: {0}. The current document is preserved.")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

pub mod update;
