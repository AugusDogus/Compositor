use crate::{
    Result,
    document::{Document, Layer, LayerContent},
    geometry::Point,
    invalid, render,
};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

#[derive(Clone)]
pub struct PixelClipboard {
    pub pixels: Arc<RgbaImage>,
    pub origin: Point,
}

pub fn copy(doc: &Document, merged: bool, mask: bool) -> Result<PixelClipboard> {
    let bounds = match &doc.selection {
        Some(selection) => selection
            .bounds()
            .ok_or_else(|| invalid("The selection is empty. Select pixels before copying."))?,
        None => [0., 0., doc.width as f64, doc.height as f64],
    };
    let width = (bounds[2] - bounds[0]) as u32;
    let height = (bounds[3] - bounds[1]) as u32;
    crate::document::validate_size(width, height)?;
    let layer = doc.active_layer();
    if !merged
        && layer.is_none_or(|l| {
            if mask {
                l.mask.is_none()
            } else {
                l.raster().is_none()
            }
        })
    {
        return Err(invalid("Select a pixel layer or a layer mask to copy."));
    }
    let sampler = render::Sampler::new(doc);
    let pixels = RgbaImage::from_fn(width, height, |x, y| {
        let point = [x as f64 + bounds[0] + 0.5, y as f64 + bounds[1] + 0.5];
        let mut color = if merged {
            sampler.sample(point)
        } else if let Some(layer) = layer {
            if mask {
                if let Some(mask) = &layer.mask {
                    let unit = mask.placement.unwrap_or(layer.transform).unit(point);
                    let coverage = if unit.iter().all(|v| (0. ..1.).contains(v)) {
                        mask.pixels[(
                            (unit[0] * mask.pixels.width() as f64) as u32,
                            (unit[1] * mask.pixels.height() as f64) as u32,
                        )][0] as f64
                            / 255.
                    } else {
                        0.
                    };
                    [coverage, coverage, coverage, 1.]
                } else {
                    [0.; 4]
                }
            } else if let Some(pixels) = layer.raster() {
                render::pixel(
                    pixels,
                    layer.transform.unit(point),
                    layer.transform.sampling,
                )
            } else {
                [0.; 4]
            }
        } else {
            [0.; 4]
        };
        color[3] *= doc.selection.as_ref().map_or(1., |s| s.coverage(point));
        Rgba(color.map(|v| (v.clamp(0., 1.) * 255.).round() as u8))
    });
    Ok(PixelClipboard {
        pixels: Arc::new(pixels),
        origin: [bounds[0], bounds[1]],
    })
}

pub fn paste(doc: &mut Document, clipboard: PixelClipboard) -> Result<()> {
    let mut layer = Layer::blank(
        crate::layer_ops::next_layer_name(doc),
        clipboard.pixels.width(),
        clipboard.pixels.height(),
    );
    layer.transform.origin = clipboard.origin;
    layer.content = LayerContent::Raster(Some(clipboard.pixels));
    let parent = doc
        .active_layer()
        .and_then(|l| if l.is_group() { Some(l.id) } else { l.parent });
    let index = doc
        .layers
        .iter()
        .position(|l| Some(l.id) == doc.active)
        .map(|i| i + 1)
        .unwrap_or(doc.layers.len());
    layer.parent = parent;
    let id = layer.id;
    doc.add(layer)?;
    if let Some(layer) = doc.layers.pop() {
        doc.layers.insert(index, layer);
    }
    doc.select(id, false);
    doc.selection = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{edits, selection::Selection};

    #[test]
    fn paste_into_a_selected_folder_uses_a_unique_layer_name() {
        let mut doc = Document::new(4, 4).unwrap();
        crate::layer_ops::group(&mut doc).unwrap();
        let group = doc.active.unwrap();
        paste(
            &mut doc,
            PixelClipboard {
                pixels: Arc::new(RgbaImage::from_pixel(2, 2, Rgba([255; 4]))),
                origin: [1., 1.],
            },
        )
        .unwrap();
        let pasted = doc.active_layer().unwrap();
        assert_eq!(pasted.parent, Some(group));
        assert_eq!(pasted.name, "Layer 2");
        assert_eq!(doc.layers[1].id, pasted.id);
        doc.validate().unwrap();
    }
    #[test]
    fn copy_ignores_layer_appearance_and_pastes_at_original_location() {
        let mut doc = Document::new(100, 40).unwrap();
        edits::fill(&mut doc, [255, 0, 0, 255], false, false).unwrap();
        doc.layers[0].opacity = 0.2;
        doc.layers[0].visible = false;
        doc.selection = Some(Selection::rectangle(100, 40, [40., 10.], [60., 30.], false));
        let clip = copy(&doc, false, false).unwrap();
        assert_eq!(clip.origin, [40., 10.]);
        assert_eq!(clip.pixels.dimensions(), (20, 20));
        assert_eq!(clip.pixels[(0, 0)], Rgba([255, 0, 0, 255]));
        paste(&mut doc, clip).unwrap();
        assert_eq!(doc.layers[1].transform.origin, [40., 10.]);
        assert!(doc.selection.is_none());
    }
    #[test]
    fn cut_and_paste_restore_selected_pixels() {
        let mut doc = Document::new(20, 20).unwrap();
        edits::fill(&mut doc, [255, 0, 0, 255], false, false).unwrap();
        doc.selection = Some(Selection::rectangle(20, 20, [5., 5.], [10., 10.], false));
        let clip = copy(&doc, false, false).unwrap();
        edits::fill(&mut doc, [0; 4], true, false).unwrap();
        assert_eq!(doc.layers[0].raster().unwrap()[(6, 6)][3], 0);
        paste(&mut doc, clip).unwrap();
        assert_eq!(render::render(&doc, 20, 20)[(6, 6)], Rgba([255, 0, 0, 255]));
    }
}
