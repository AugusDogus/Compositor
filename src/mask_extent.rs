//! Extend masks on their existing pixel grid while keeping layer pixels fixed.
use crate::{
    Result,
    document::Layer,
    invalid,
    raster_extent::{Expansion, expanded_grid},
};
use image::{GrayImage, Luma};
use std::sync::Arc;

pub(crate) fn expand(layer: &mut Layer, bounds: [f64; 4]) -> Result<Option<Expansion>> {
    let mask = layer
        .mask
        .as_mut()
        .ok_or_else(|| invalid("The layer mask is missing."))?;
    let placement = mask.placement.unwrap_or(layer.transform);
    let dimensions = [mask.pixels.width(), mask.pixels.height()];
    let Some((expansion, placement)) = expanded_grid(placement, dimensions, bounds)? else {
        return Ok(None);
    };
    let background = (mask.background() * 255.).round() as u8;
    let mut pixels =
        GrayImage::from_pixel(expansion.size[0], expansion.size[1], Luma([background]));
    image::imageops::replace(
        &mut pixels,
        mask.pixels.as_ref(),
        expansion.offset[0] as i64,
        expansion.offset[1] as i64,
    );
    mask.pixels = Arc::new(pixels);
    mask.placement = Some(placement);
    Ok(Some(expansion))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        brush::{Brush, PaintMode, Stroke},
        document::{Document, Mask},
    };
    #[test]
    fn mask_strokes_extend_past_the_original_pixels_without_moving_the_layer() {
        let mut document = Document::new(40, 40).unwrap();
        let layer = document.active_layer_mut().unwrap();
        layer.transform = crate::geometry::Transform::new(4, 4);
        layer.transform.origin = [18., 18.];
        let original = layer.transform;
        layer.mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(4, 4, Luma([255]))),
            enabled: true,
            linked: false,
            placement: None,
        });
        let brush = Brush {
            diameter: 6.,
            hardness: 1.,
            color: [0, 0, 0, 255],
            ..Brush::default()
        };
        let mut stroke = Stroke::start(
            &mut document,
            [8., 8.],
            brush,
            PaintMode::Paint,
            true,
            false,
        )
        .unwrap();
        stroke.to(&mut document, [30., 30.]).unwrap();
        stroke.finish(&mut document).unwrap();
        let layer = document.active_layer().unwrap();
        assert_eq!(layer.transform, original);
        let mask = layer.mask.as_ref().unwrap();
        assert!(!mask.linked);
        let placement = mask.placement.unwrap();
        for point in [[8., 8.], [30., 30.]] {
            let unit = placement.unit(point);
            assert!(unit.iter().all(|v| (0. ..1.).contains(v)));
            assert_eq!(
                mask.pixels[(
                    (unit[0] * mask.pixels.width() as f64) as u32,
                    (unit[1] * mask.pixels.height() as f64) as u32
                )][0],
                0
            );
        }
    }
}
