use crate::{
    Result,
    document::{Layer, LayerContent, validate_size},
    geometry::Transform,
    invalid,
};
use image::{GrayImage, Luma, RgbaImage};
use std::sync::Arc;

pub(crate) struct Expansion {
    pub offset: [u32; 2],
    pub size: [u32; 2],
}

/// Give an empty large layer a one-pixel source at the first brush position.
/// Subsequent expansion allocates only the stroke's bounds, on its original grid.
pub(crate) fn seed(layer: &mut Layer, point: crate::geometry::Point) -> Result<()> {
    let old = layer.transform;
    if !old.valid() {
        return Err(invalid("The layer's paint transform is invalid."));
    }
    let grid = old.size.map(f64::ceil);
    let unit = old.unit(point);
    let center = old.point([
        ((unit[0] * grid[0]).floor() + 0.5) / grid[0],
        ((unit[1] * grid[1]).floor() + 0.5) / grid[1],
    ]);
    let size = [old.size[0] / grid[0], old.size[1] / grid[1]];
    let transform = Transform {
        origin: [center[0] - size[0] / 2., center[1] - size[1] / 2.],
        size,
        ..old
    };
    if !transform.valid() {
        return Err(invalid(
            "Painting here would exceed the supported layer bounds.",
        ));
    }
    if let Some(mask) = &mut layer.mask {
        mask.placement.get_or_insert(old);
    }
    layer.transform = transform;
    layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::new(1, 1))));
    Ok(())
}

/// Pads a pixel layer to cover a document-space rectangle, preserving its pixel grid and appearance.
/// Rotated and flipped layers keep the same mapping for every existing source pixel.
pub(crate) fn expand(layer: &mut Layer, bounds: [f64; 4]) -> Result<Option<Expansion>> {
    let LayerContent::Raster(pixels) = &layer.content else {
        return Err(invalid("This edit requires a pixel layer."));
    };
    let old = layer.transform;
    let (width, height) = pixels.as_ref().map_or(
        (old.size[0].ceil() as u32, old.size[1].ceil() as u32),
        |p| p.dimensions(),
    );
    let mut extent = [0., 0., width as f64, height as f64];
    for p in [
        [bounds[0], bounds[1]],
        [bounds[2], bounds[1]],
        [bounds[2], bounds[3]],
        [bounds[0], bounds[3]],
    ] {
        let unit = old.unit(p);
        let (x, y) = (unit[0] * width as f64, unit[1] * height as f64);
        extent[0] = extent[0].min((x + 1e-9).floor());
        extent[1] = extent[1].min((y + 1e-9).floor());
        extent[2] = extent[2].max((x - 1e-9).ceil());
        extent[3] = extent[3].max((y - 1e-9).ceil());
    }
    let size = [
        (extent[2] - extent[0]) as u32,
        (extent[3] - extent[1]) as u32,
    ];
    if size == [width, height] {
        return Ok(None);
    }
    validate_size(size[0], size[1])?;
    let dimensions = [
        size[0] as f64 * old.size[0] / width as f64,
        size[1] as f64 * old.size[1] / height as f64,
    ];
    let center = old.point([
        (extent[0] + size[0] as f64 / 2.) / width as f64,
        (extent[1] + size[1] as f64 / 2.) / height as f64,
    ]);
    let transform = Transform {
        origin: [
            center[0] - dimensions[0] / 2.,
            center[1] - dimensions[1] / 2.,
        ],
        size: dimensions,
        ..old
    };
    if !transform.valid() {
        return Err(invalid(
            "Painting here would exceed the supported layer bounds.",
        ));
    }
    let offset = [(-extent[0]) as u32, (-extent[1]) as u32];
    let mut expanded = RgbaImage::new(size[0], size[1]);
    if let Some(pixels) = pixels {
        image::imageops::replace(
            &mut expanded,
            pixels.as_ref(),
            offset[0] as i64,
            offset[1] as i64,
        );
    }
    if let Some(mask) = &mut layer.mask
        && mask.placement.is_none()
    {
        let mut grown = GrayImage::from_pixel(size[0], size[1], Luma([255]));
        let original = image::imageops::resize(
            mask.pixels.as_ref(),
            width,
            height,
            image::imageops::FilterType::Nearest,
        );
        image::imageops::replace(&mut grown, &original, offset[0] as i64, offset[1] as i64);
        mask.pixels = Arc::new(grown);
    }
    layer.content = LayerContent::Raster(Some(Arc::new(expanded)));
    layer.transform = transform;
    Ok(Some(Expansion { offset, size }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    #[test]
    fn expansion_preserves_rotated_flipped_pixel_coordinates() {
        let mut layer = Layer::blank("Source", 2, 3);
        layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            2,
            3,
            Rgba([12, 34, 56, 78]),
        ))));
        layer.transform.origin = [5., 4.];
        layer.transform.rotation = 37.;
        layer.transform.flip_x = true;
        let before = layer.transform;
        let expanded = expand(&mut layer, [0., 0., 10., 10.]).unwrap().unwrap();
        for y in 0..3 {
            for x in 0..2 {
                let p = before.point([(x as f64 + 0.5) / 2., (y as f64 + 0.5) / 3.]);
                let q = layer.transform.point([
                    (x as f64 + expanded.offset[0] as f64 + 0.5) / expanded.size[0] as f64,
                    (y as f64 + expanded.offset[1] as f64 + 0.5) / expanded.size[1] as f64,
                ]);
                assert!((p[0] - q[0]).abs() < 1e-8 && (p[1] - q[1]).abs() < 1e-8);
                assert_eq!(
                    layer.raster().unwrap()[(x + expanded.offset[0], y + expanded.offset[1])],
                    Rgba([12, 34, 56, 78])
                );
            }
        }
    }
}
