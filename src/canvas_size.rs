use crate::{
    Result,
    document::{Document, Layer, LayerContent, validate_size},
    edits,
    geometry::Point,
    invalid,
};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub enum Unit {
    Pixels,
    Percent,
    Inches,
    Centimeters,
}

pub fn dimensions(doc: &Document, values: Point, unit: Unit, relative: bool) -> Result<[u32; 2]> {
    let original = [doc.width as f64, doc.height as f64];
    let pixels: Point = std::array::from_fn(|i| {
        let value = match unit {
            Unit::Pixels => values[i],
            Unit::Percent => values[i] / 100. * original[i],
            Unit::Inches => values[i] * doc.resolution,
            Unit::Centimeters => values[i] / 2.54 * doc.resolution,
        };
        (value + if relative { original[i] } else { 0. }).round()
    });
    if pixels.iter().any(|v| !(1. ..=30_000.).contains(v)) {
        return Err(invalid(
            "The resulting canvas must be between 1 and 30,000 pixels on each side. Adjust the dimensions or units.",
        ));
    }
    let size = pixels.map(|v| v as u32);
    crate::document::validate_canvas_size(size[0], size[1])?;
    Ok(size)
}

pub fn resize(
    doc: &mut Document,
    size: [u32; 2],
    anchor: Point,
    fill: Option<[u8; 4]>,
) -> Result<()> {
    let old = [doc.width, doc.height];
    if fill.is_some() && (size[0] > old[0] || size[1] > old[1]) {
        validate_size(size[0], size[1])?;
    }
    let offset: Point =
        std::array::from_fn(|i| ((size[i] as f64 - old[i] as f64) * anchor[i]).floor());
    edits::canvas_size(doc, size[0], size[1], anchor)?;
    if let Some(color) = fill
        && (size[0] > old[0] || size[1] > old[1])
    {
        let mut layer = Layer::blank("Canvas Extension", size[0], size[1]);
        layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(
            size[0],
            size[1],
            |x, y| {
                let point = [x as f64 + 0.5 - offset[0], y as f64 + 0.5 - offset[1]];
                if point[0] >= 0.
                    && point[1] >= 0.
                    && point[0] < old[0] as f64
                    && point[1] < old[1] as f64
                {
                    Rgba([0; 4])
                } else {
                    Rgba(color)
                }
            },
        ))));
        let active = doc.active;
        let selected = doc.selected.clone();
        doc.add(layer)?;
        if let Some(layer) = doc.layers.pop() {
            doc.layers.insert(0, layer);
        }
        doc.active = active;
        doc.selected = selected;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_and_relative_dimensions_follow_document_resolution() {
        let mut doc = Document::new(600, 300).unwrap();
        doc.resolution = 300.;
        assert_eq!(
            dimensions(&doc, [2.54, 5.08], Unit::Centimeters, false).unwrap(),
            [300, 600]
        );
        assert_eq!(
            dimensions(&doc, [1., 1.], Unit::Inches, true).unwrap(),
            [900, 600]
        );
        assert_eq!(
            dimensions(&doc, [-50., 100.], Unit::Percent, true).unwrap(),
            [300, 600]
        );
        assert!(dimensions(&doc, [-100., 0.], Unit::Percent, true).is_err());
    }
    #[test]
    fn extension_color_keeps_holes_in_the_old_canvas_transparent() {
        let mut doc = Document::new(2, 2).unwrap();
        let active = doc.active;
        resize(&mut doc, [5, 3], [0.5, 0.5], Some([255, 0, 0, 255])).unwrap();
        assert_eq!(doc.active, active);
        assert_eq!(doc.layers[1].transform.origin, [1., 0.]);
        let pixels = crate::render::render(&doc, 5, 3).unwrap();
        assert_eq!(pixels[(1, 0)], Rgba([0; 4]));
        assert_eq!(pixels[(0, 0)], Rgba([255, 0, 0, 255]));
        assert_eq!(pixels[(1, 2)], Rgba([255, 0, 0, 255]));
        doc.validate().unwrap();
    }
}
