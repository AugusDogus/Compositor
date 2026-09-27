//! Convert color data only. Alpha, masks and adjustment parameters are not image colors.
use crate::{
    Result,
    document::{Document, LayerContent},
    image_io, invalid,
};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

pub(super) fn validate(bytes: &[u8], grayscale: bool) -> Result<()> {
    let profile = lcms2::Profile::new_icc(bytes).map_err(|error| invalid(format!(
        "PSD color profile is damaged: {error}. Re-export the source with a valid profile; the current document is unchanged."
    )))?;
    let expected = if grayscale {
        lcms2::ColorSpaceSignature::GrayData
    } else {
        lcms2::ColorSpaceSignature::RgbData
    };
    if profile.color_space() != expected {
        return Err(invalid(
            "PSD color profile does not match its RGB or grayscale channels. Assign the correct profile in the source editor before importing; the current document is unchanged.",
        ));
    }
    Ok(())
}

pub(super) fn convert(document: &mut Document, profile: &[u8]) -> Result<()> {
    for layer in &mut document.layers {
        if let LayerContent::Raster(Some(pixels)) = &mut layer.content {
            image_io::convert_to_srgb(Arc::make_mut(pixels), profile)?;
        }
        if let Some(shape) = &mut layer.shape {
            [shape.red, shape.green, shape.blue] =
                color([shape.red, shape.green, shape.blue], profile)?;
        }
        if let Some(text) = &mut layer.text {
            [text.red, text.green, text.blue] = color(text.base_color(), profile)?;
            for run in text.color_runs.iter_mut().flatten() {
                [run.red, run.green, run.blue] = color([run.red, run.green, run.blue], profile)?;
            }
        }
    }
    Ok(())
}

fn color(rgb: [f64; 3], profile: &[u8]) -> Result<[f64; 3]> {
    let mut pixel = RgbaImage::from_pixel(
        1,
        1,
        Rgba([
            (rgb[0] * 255.).round() as u8,
            (rgb[1] * 255.).round() as u8,
            (rgb[2] * 255.).round() as u8,
            255,
        ]),
    );
    image_io::convert_to_srgb(&mut pixel, profile)?;
    Ok([0, 1, 2].map(|channel| f64::from(pixel[(0, 0)][channel]) / 255.))
}
