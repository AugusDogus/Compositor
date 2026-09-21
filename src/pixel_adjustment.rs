use crate::{
    Result,
    adjustment::Adjustment,
    document::{Layer, LayerContent},
    invalid,
    selection::Selection,
};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

/// Applies color settings to source pixels, preserving alpha and the layer's placement/mask.
pub fn apply(
    layer: &mut Layer,
    settings: &Adjustment,
    selection: Option<&Selection>,
) -> Result<()> {
    settings.validate()?;
    layer.require_rasterized()?;
    let source = layer
        .raster()
        .ok_or_else(|| invalid("Select a layer containing pixels to adjust its colors."))?;
    let result = RgbaImage::from_fn(source.width(), source.height(), |x, y| {
        let point = layer.transform.point([
            (x as f64 + 0.5) / source.width() as f64,
            (y as f64 + 0.5) / source.height() as f64,
        ]);
        let coverage = selection.map_or(1., |s| s.coverage(point));
        let before = source[(x, y)];
        if coverage == 0. || before[3] == 0 {
            return before;
        }
        let adjusted = settings.apply(before.0.map(|v| v as f64 / 255.), point);
        let mut result = before.0;
        for k in 0..3 {
            result[k] = (before[k] as f64 + (adjusted[k] * 255. - before[k] as f64) * coverage)
                .clamp(0., 255.)
                .round() as u8;
        }
        Rgba(result)
    });
    layer.content = LayerContent::Raster(Some(Arc::new(result)));
    layer.shape = None;
    layer.text = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adjustment::{Exposure, Kind},
        geometry::Transform,
    };
    #[test]
    fn adjustments_respect_document_selection_on_a_rotated_flipped_layer() {
        let mut layer = Layer::blank("Pixels", 2, 1);
        layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            2,
            1,
            Rgba([60, 80, 100, 128]),
        ))));
        layer.transform = Transform {
            origin: [2., 2.],
            rotation: 90.,
            flip_x: true,
            ..Transform::new(2, 1)
        };
        let selection = Selection::rectangle(6, 6, [0., 0.], [6., 3.], false);
        let mut settings = Adjustment::new(Kind::Exposure);
        settings.exposure_settings = Some(Exposure {
            exposure: 1.,
            ..Exposure::default()
        });
        let before = layer.clone();
        apply(&mut layer, &settings, Some(&selection)).unwrap();
        let pixels = layer.raster().unwrap();
        assert_eq!(pixels[(0, 0)], Rgba([60, 80, 100, 128]));
        assert!(pixels[(1, 0)][0] > 60);
        assert_eq!(pixels[(1, 0)][3], 128);
        assert_eq!(layer.transform, before.transform);
    }
}
