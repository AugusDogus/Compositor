use crate::{
    Result,
    blend::Blend,
    document::{Document, Layer, LayerContent, Mask, Shape, validate_size},
    geometry::Point,
    invalid,
    selection::Selection,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

pub fn move_selected(doc: &mut Document, delta: Point, mask_target: bool) -> Result<()> {
    let old = crate::transform::selection_bounds(doc, mask_target)
        .ok_or_else(|| invalid("Select a layer or mask to move."))?;
    let mut new = old;
    new.origin[0] += delta[0];
    new.origin[1] += delta[1];
    crate::transform::apply(doc, old, new, mask_target)
}

pub fn canvas_size(doc: &mut Document, width: u32, height: u32, anchor: Point) -> Result<()> {
    crate::document::validate_canvas_size(width, height)?;
    if !anchor.iter().all(|v| (0. ..=1.).contains(v)) {
        return Err(invalid(
            "Canvas anchors must be between the top-left and bottom-right corners.",
        ));
    }
    let delta = [
        ((width as f64 - doc.width as f64) * anchor[0]).floor(),
        ((height as f64 - doc.height as f64) * anchor[1]).floor(),
    ];
    for layer in &mut doc.layers {
        for (i, d) in delta.into_iter().enumerate() {
            layer.transform.origin[i] += d;
        }
        if let Some(t) = layer.mask.as_mut().and_then(|m| m.placement.as_mut()) {
            for (i, d) in delta.into_iter().enumerate() {
                t.origin[i] += d;
            }
        }
    }
    for guide in &mut doc.guides {
        guide.position += delta[guide.axis.index()];
    }
    doc.width = width;
    doc.height = height;
    doc.selection = None;
    Ok(())
}

pub fn flip_canvas(doc: &mut Document, horizontal: bool) {
    let axis = if horizontal {
        doc.width as f64 / 2.
    } else {
        doc.height as f64 / 2.
    };
    for guide in &mut doc.guides {
        if (guide.axis == crate::guides::Axis::Vertical) == horizontal {
            guide.position = 2. * axis - guide.position;
        }
    }
    for layer in &mut doc.layers {
        layer.transform = layer.transform.mirrored(horizontal, axis);
        if let Some(placement) = layer.mask.as_mut().and_then(|m| m.placement.as_mut()) {
            *placement = placement.mirrored(horizontal, axis);
        }
    }
    if let Some(selection) = &mut doc.selection {
        selection.mirror(horizontal, axis);
    }
}

pub fn crop(doc: &mut Document, a: Point, b: Point) -> Result<()> {
    let left = a[0].min(b[0]).round();
    let top = a[1].min(b[1]).round();
    let width = (a[0] - b[0]).abs().round() as u32;
    let height = (a[1] - b[1]).abs().round() as u32;
    crate::document::validate_canvas_size(width, height)?;
    for layer in &mut doc.layers {
        layer.transform.origin[0] -= left;
        layer.transform.origin[1] -= top;
        if let Some(t) = layer.mask.as_mut().and_then(|m| m.placement.as_mut()) {
            t.origin[0] -= left;
            t.origin[1] -= top;
        }
    }
    for guide in &mut doc.guides {
        guide.position -= [left, top][guide.axis.index()];
    }
    doc.width = width;
    doc.height = height;
    doc.selection = None;
    Ok(())
}

pub fn add_mask(doc: &mut Document, hide: bool) -> Result<()> {
    if doc.selected.len() != 1 {
        return Err(invalid(
            "Select one layer to add a mask. Existing layers and masks are unchanged.",
        ));
    }
    let selection = doc.selection.clone();
    let layer = doc
        .active_layer_mut()
        .ok_or_else(|| invalid("Select a layer to add a mask."))?;
    if layer.mask.is_some() {
        return Err(invalid(
            "This layer already has a mask. Delete it before adding another; the existing mask is unchanged.",
        ));
    }
    let pixels = if let Some(selection) = selection {
        let (w, h) = layer.raster().map_or_else(
            || {
                (
                    layer.transform.size[0].round() as u32,
                    layer.transform.size[1].round() as u32,
                )
            },
            |pixels| pixels.dimensions(),
        );
        validate_size(w, h)?;
        GrayImage::from_fn(w, h, |x, y| {
            let coverage = selection.coverage(
                layer
                    .transform
                    .point([(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64]),
            );
            Luma([((if hide { coverage } else { 1. - coverage }) * 255.).round() as u8])
        })
    } else {
        GrayImage::from_pixel(1, 1, Luma([if hide { 0 } else { 255 }]))
    };
    layer.mask = Some(Mask {
        pixels: Arc::new(pixels),
        enabled: true,
        linked: true,
        placement: None,
    });
    doc.selection = None;
    Ok(())
}

pub fn fill(doc: &mut Document, color: [u8; 4], erase: bool, mask: bool) -> Result<()> {
    let selection = doc.selection.clone();
    let canvas = [doc.width as f64, doc.height as f64];
    let layer = doc
        .active_layer_mut()
        .ok_or_else(|| invalid("Select a layer to fill."))?;
    if !mask && !erase && selection.is_none() && layer.text.is_some() {
        return crate::text::recolor_layer(layer, color);
    }
    if !mask && !erase {
        crate::raster_extent::expand(layer, [0., 0., canvas[0], canvas[1]])?;
    }
    if mask {
        let mask = layer
            .mask
            .as_mut()
            .ok_or_else(|| invalid("The selected layer has no mask."))?;
        let t = mask.placement.unwrap_or(layer.transform);
        let (w, h) = if mask.pixels.dimensions() == (1, 1) && selection.is_some() {
            (t.size[0].round() as u32, t.size[1].round() as u32)
        } else {
            mask.pixels.dimensions()
        };
        validate_size(w, h)?;
        let original = mask.pixels.clone();
        mask.pixels = Arc::new(GrayImage::from_fn(w, h, |x, y| {
            let u = [(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64];
            let coverage = selection.as_ref().map_or(1., |s| s.coverage(t.point(u)));
            let old = original[(
                (u[0] * original.width() as f64) as u32,
                (u[1] * original.height() as f64) as u32,
            )][0] as f64;
            let target = if erase { 0. } else { color[0] as f64 };
            Luma([(old + (target - old) * coverage).round() as u8])
        }));
    } else if let LayerContent::Raster(pixels) = &mut layer.content {
        if pixels.is_none() {
            let w = layer.transform.size[0].round() as u32;
            let h = layer.transform.size[1].round() as u32;
            validate_size(w, h)?;
            *pixels = Some(Arc::new(RgbaImage::new(w, h)));
        }
        if let Some(pixels) = pixels {
            let (w, h) = pixels.dimensions();
            for (x, y, p) in Arc::make_mut(pixels).enumerate_pixels_mut() {
                let point = layer
                    .transform
                    .point([(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64]);
                let coverage = if point[0] >= 0.
                    && point[1] >= 0.
                    && point[0] < canvas[0]
                    && point[1] < canvas[1]
                {
                    selection.as_ref().map_or(1., |s| s.coverage(point))
                } else {
                    0.
                };
                if erase {
                    p[3] = (p[3] as f64 * (1. - coverage)).round() as u8;
                } else {
                    let mut top = color.map(|v| v as f64 / 255.);
                    top[3] *= coverage;
                    *p = Rgba(
                        Blend::Normal
                            .composite(p.0.map(|v| v as f64 / 255.), top)
                            .map(|v| (v * 255.).round() as u8),
                    );
                }
            }
        }
        layer.shape = None;
        layer.text = None;
    } else {
        return Err(invalid("Select a pixel layer or a mask to fill."));
    }
    Ok(())
}

pub fn shape(doc: &mut Document, a: Point, b: Point, mut shape: Shape) -> Result<()> {
    if a.iter()
        .chain(b.iter())
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
    {
        return Err(invalid(
            "Shape coordinates exceed supported bounds. Draw closer to the canvas.",
        ));
    }
    if !shape.valid() {
        return Err(invalid(
            "Shape settings are invalid. Existing layers are unchanged.",
        ));
    }
    let mut a = a.map(f64::round);
    let mut b = b.map(f64::round);
    if a == b {
        return Ok(());
    }
    if let crate::document::ShapeGeometry::Line { line_width, .. } = shape.geometry {
        let from = a;
        let to = b;
        a = std::array::from_fn(|i| (from[i].min(to[i]) - line_width / 2.).floor());
        b = std::array::from_fn(|i| (from[i].max(to[i]) + line_width / 2.).ceil());
        shape.geometry = crate::document::ShapeGeometry::Line {
            line_width,
            start: std::array::from_fn(|i| (from[i] - a[i]) / (b[i] - a[i])),
            end: std::array::from_fn(|i| (to[i] - a[i]) / (b[i] - a[i])),
        };
    }
    let w = (a[0] - b[0]).abs() as u32;
    let h = (a[1] - b[1]).abs() as u32;
    if w == 0 || h == 0 {
        return Ok(());
    }
    validate_size(w, h)?;
    let kind = shape.kind().label();
    let mut number = 1;
    while doc
        .layers
        .iter()
        .any(|l| l.name == format!("{kind} {number}"))
    {
        number += 1;
    }
    let mut layer = Layer::blank(format!("{kind} {number}"), w, h);
    layer.parent = doc
        .active_layer()
        .and_then(|l| if l.is_group() { Some(l.id) } else { l.parent });
    let index = doc
        .layers
        .iter()
        .position(|l| Some(l.id) == doc.active)
        .map_or(doc.layers.len(), |i| i + 1);
    layer.transform.origin = [a[0].min(b[0]), a[1].min(b[1])];
    layer.content = LayerContent::Raster(Some(Arc::new(shape_pixels(w, h, shape))));
    layer.shape = Some(shape);
    doc.add(layer)?;
    if let Some(layer) = doc.layers.pop() {
        doc.layers.insert(index, layer);
    }
    Ok(())
}

pub fn shape_pixels(w: u32, h: u32, shape: Shape) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let mut inside = 0_u32;
        for oy in [0.25, 0.75] {
            for ox in [0.25, 0.75] {
                let px = x as f64 + ox;
                let py = y as f64 + oy;
                let hit = match shape.geometry {
                    crate::document::ShapeGeometry::Line {
                        line_width,
                        start,
                        end,
                    } => {
                        let a = [start[0] * w as f64, start[1] * h as f64];
                        let b = [end[0] * w as f64, end[1] * h as f64];
                        let d = [b[0] - a[0], b[1] - a[1]];
                        let length2 = d[0] * d[0] + d[1] * d[1];
                        let t = if length2 > 0. {
                            ((px - a[0]) * d[0] + (py - a[1]) * d[1]) / length2
                        } else {
                            0.
                        }
                        .clamp(0., 1.);
                        (px - a[0] - t * d[0]).hypot(py - a[1] - t * d[1]) <= line_width / 2.
                    }
                    crate::document::ShapeGeometry::Ellipse => {
                        ((px / w as f64 - 0.5) * 2.).powi(2) + ((py / h as f64 - 0.5) * 2.).powi(2)
                            <= 1.
                    }
                    crate::document::ShapeGeometry::Rectangle => {
                        let r = shape.corner_radius.min(w.min(h) as f64 / 2.);
                        let cx = px.clamp(r, w as f64 - r);
                        let cy = py.clamp(r, h as f64 - r);
                        (px - cx).hypot(py - cy) <= r
                    }
                };
                if hit {
                    inside += 1;
                }
            }
        }
        Rgba([
            (shape.red * 255.).round() as u8,
            (shape.green * 255.).round() as u8,
            (shape.blue * 255.).round() as u8,
            (inside * 255 / 4) as u8,
        ])
    })
}

pub fn load_selection(
    doc: &mut Document,
    mask: bool,
    mode: crate::selection::SelectionMode,
    antialiased: bool,
) -> Result<()> {
    let id = doc
        .active
        .ok_or_else(|| invalid("Select a layer to load its coverage."))?;
    load_selection_from(doc, id, mask, mode, antialiased)
}

pub fn load_selection_from(
    doc: &mut Document,
    id: uuid::Uuid,
    mask: bool,
    mode: crate::selection::SelectionMode,
    antialiased: bool,
) -> Result<()> {
    use crate::selection::SelectionMode;
    let layer = doc.layer(id).ok_or_else(|| {
        invalid("The selection source layer was removed. Choose an existing layer.")
    })?;
    let (coverage, transform) = if mask {
        let mask = layer
            .mask
            .as_ref()
            .ok_or_else(|| invalid("The selected layer has no mask."))?;
        (
            GrayImage::from_fn(mask.pixels.width(), mask.pixels.height(), |x, y| {
                Luma([255 - mask.pixels[(x, y)][0]])
            }),
            mask.placement.unwrap_or(layer.transform),
        )
    } else {
        let Some(pixels) = layer.raster() else {
            return Ok(());
        };
        (
            GrayImage::from_fn(pixels.width(), pixels.height(), |x, y| {
                Luma([pixels[(x, y)][3]])
            }),
            layer.transform,
        )
    };
    if coverage.pixels().all(|p| p[0] < 128) {
        return Ok(());
    }
    let next = Selection::from_coverage(&coverage, transform, doc.width, doc.height, antialiased)?;
    doc.selection = match (&doc.selection, mode) {
        (None, SelectionMode::Subtract | SelectionMode::Intersect) => None,
        (None, _) => Some(next),
        (Some(base), _) => Some(base.combine(&next, mode)?),
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn adding_mask_consumes_selection_on_source_pixel_grid_with_one_undo_step() {
        use super::*;
        for hide in [false, true] {
            let mut doc = Document::new(8, 4).unwrap();
            doc.layers[0].content =
                LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(4, 2, Rgba([255; 4])))));
            doc.selection = Some(Selection::rectangle(8, 4, [0., 0.], [4., 4.], false));
            let original = doc.clone();
            let mut session = crate::session::Session::new(doc, None);
            session.edit("Add Mask", |doc| add_mask(doc, hide)).unwrap();
            let masked = session.document.clone();
            let mask = masked.layers[0].mask.as_ref().unwrap();
            assert_eq!(mask.pixels.dimensions(), (4, 2));
            assert_eq!(mask.pixels[(0, 0)][0], if hide { 255 } else { 0 });
            assert_eq!(mask.pixels[(3, 1)][0], if hide { 0 } else { 255 });
            assert!(masked.selection.is_none());
            assert!(add_mask(&mut session.document, hide).is_err());
            assert_eq!(session.document, masked);
            session.undo();
            assert_eq!(session.document, original);
            session.redo();
            assert_eq!(session.document, masked);
        }
    }
    use super::*;
    use crate::render;
    #[test]
    fn load_selection_traces_dark_masks_and_half_opaque_pixels_before_transforming() {
        use crate::{geometry::Transform, selection::SelectionMode};
        let mut doc = Document::new(200, 200).unwrap();
        doc.layers[0].content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(50, 50, |x, y| {
                Rgba([
                    200,
                    0,
                    0,
                    if (10..40).contains(&x)
                        && (10..40).contains(&y)
                        && !((20..30).contains(&x) && (20..30).contains(&y))
                    {
                        128
                    } else {
                        127
                    },
                ])
            }))));
        doc.layers[0].transform = Transform {
            origin: [50., 50.],
            size: [100., 100.],
            ..Transform::new(50, 50)
        };
        doc.layers[0].mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([0]))),
            enabled: true,
            placement: None,
            linked: true,
        });
        load_selection(&mut doc, false, SelectionMode::Replace, true).unwrap();
        let selected = doc.selection.as_ref().unwrap();
        assert_eq!(selected.bounds(), Some([70., 70., 130., 130.]));
        assert_eq!(selected.coverage([75., 75.]), 1.);
        assert_eq!(selected.coverage([100., 100.]), 0.);
        assert_eq!(selected.coverage([50., 50.]), 0.);
        doc.layers[0].mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_fn(10, 10, |x, y| {
                Luma([if (2..8).contains(&x) && (2..8).contains(&y) {
                    127
                } else {
                    128
                }])
            })),
            enabled: false,
            linked: false,
            placement: Some(Transform {
                origin: [0., 0.],
                size: [50., 50.],
                ..Transform::new(10, 10)
            }),
        });
        load_selection(&mut doc, true, SelectionMode::Add, true).unwrap();
        assert_eq!(doc.selection.as_ref().unwrap().coverage([20., 20.]), 1.);
        assert_eq!(doc.selection.as_ref().unwrap().coverage([75., 75.]), 1.);
        assert_eq!(doc.selection.as_ref().unwrap().coverage([45., 45.]), 0.);
        load_selection(&mut doc, true, SelectionMode::Subtract, true).unwrap();
        assert_eq!(doc.selection.as_ref().unwrap().coverage([20., 20.]), 0.);
        assert_eq!(doc.selection.as_ref().unwrap().coverage([75., 75.]), 1.);
        let before = doc.selection.clone();
        doc.layers[0].mask.as_mut().unwrap().pixels =
            Arc::new(GrayImage::from_pixel(1, 1, Luma([255])));
        load_selection(&mut doc, true, SelectionMode::Replace, true).unwrap();
        assert_eq!(doc.selection, before);
        doc.layers[0].content = LayerContent::Raster(None);
        load_selection(&mut doc, false, SelectionMode::Replace, true).unwrap();
        assert_eq!(doc.selection, before);
    }
    use crate::geometry::Transform;
    #[test]
    fn shape_click_is_empty_and_shapes_follow_active_group_and_numbering() {
        let mut session = crate::session::Session::new(Document::new(100, 100).unwrap(), None);
        let style = Shape {
            geometry: crate::document::ShapeGeometry::Rectangle,
            red: 1.,
            green: 0.,
            blue: 0.,
            corner_radius: 3.,
        };
        let original = session.document.clone();
        session
            .edit("Shape", |doc| shape(doc, [10.2, 10.2], [10.4, 10.4], style))
            .unwrap();
        assert_eq!(session.document, original);
        assert!(session.undo_label().is_none());
        session.group().unwrap();
        let group = session.document.active.unwrap();
        shape(&mut session.document, [10.2, 10.2], [20.6, 30.6], style).unwrap();
        let first = session.document.active.unwrap();
        let layer = session.document.layer(first).unwrap();
        assert_eq!(layer.parent, Some(group));
        assert_eq!(layer.name, "Rectangle 1");
        assert_eq!(layer.transform.size, [11., 21.]);
        shape(&mut session.document, [0., 0.], [10., 10.], style).unwrap();
        let layer = session.document.active_layer().unwrap();
        assert_eq!(layer.parent, Some(group));
        assert_eq!(layer.name, "Rectangle 2");
        let index = session
            .document
            .layers
            .iter()
            .position(|l| l.id == first)
            .unwrap();
        assert_eq!(session.document.layers[index + 1].id, layer.id);
        session.document.validate().unwrap();
    }
    #[test]
    fn canvas_flip_mirrors_layers_masks_and_selection_as_one_image() {
        let mut doc = Document::new(6, 4).unwrap();
        doc.layers[0].content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(6, 4, |x, y| {
                Rgba([x as u8 * 30, y as u8 * 40, 0, 255])
            }))));
        doc.selection = Some(Selection::rectangle(6, 4, [0., 0.], [2., 2.], false));
        add_mask(&mut doc, false).unwrap();
        // Adding the mask consumes its selection. Draw a new selection for the flip.
        doc.selection = Some(Selection::rectangle(6, 4, [0., 0.], [2., 2.], false));
        let before = render::render(&doc, 6, 4).unwrap();
        flip_canvas(&mut doc, true);
        assert_eq!(
            render::render(&doc, 6, 4).unwrap(),
            image::imageops::flip_horizontal(&before)
        );
        assert_eq!(doc.selection.as_ref().unwrap().coverage([4.5, 0.5]), 1.);
        assert_eq!(doc.selection.as_ref().unwrap().coverage([0.5, 0.5]), 0.);
        flip_canvas(&mut doc, true);
        assert_eq!(render::render(&doc, 6, 4).unwrap(), before);
    }

    #[test]
    fn canvas_center_anchor_keeps_odd_resize_offsets_on_whole_pixels() {
        let mut doc = Document::new(4, 4).unwrap();
        canvas_size(&mut doc, 5, 7, [0.5, 0.5]).unwrap();
        assert_eq!(doc.layers[0].transform.origin, [0., 1.]);
        canvas_size(&mut doc, 4, 4, [0.5, 0.5]).unwrap();
        assert_eq!(doc.layers[0].transform.origin, [-1., -1.]);
    }
    #[test]
    fn nudge_respects_mask_link_and_target() {
        let mut doc = Document::new(10, 10).unwrap();
        // Transformable layers have a pixel asset; an unpainted blank has no handles.
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::new(10, 10))));
        add_mask(&mut doc, false).unwrap();
        doc.layers[0].mask.as_mut().unwrap().linked = false;
        let original = doc.layers[0].transform;
        move_selected(&mut doc, [2., 3.], false).unwrap();
        assert_eq!(doc.layers[0].transform.origin, [2., 3.]);
        assert_eq!(
            doc.layers[0].mask.as_ref().unwrap().placement,
            Some(original)
        );
        move_selected(&mut doc, [4., 5.], true).unwrap();
        assert_eq!(doc.layers[0].transform.origin, [2., 3.]);
        assert_eq!(
            doc.layers[0]
                .mask
                .as_ref()
                .unwrap()
                .placement
                .unwrap()
                .origin,
            [4., 5.]
        );
        doc.layers[0].mask.as_mut().unwrap().linked = true;
        move_selected(&mut doc, [1., 1.], true).unwrap();
        assert_eq!(doc.layers[0].transform.origin, [3., 4.]);
        assert_eq!(
            doc.layers[0]
                .mask
                .as_ref()
                .unwrap()
                .placement
                .unwrap()
                .origin,
            [5., 6.]
        );
    }
    #[test]
    fn crop_keeps_original_sources_and_repositions_unlinked_mask() {
        let mut doc = Document::new(10, 10).unwrap();
        fill(&mut doc, [255, 0, 0, 255], false, false).unwrap();
        add_mask(&mut doc, false).unwrap();
        doc.layers[0].mask.as_mut().unwrap().placement = Some(Transform::new(10, 10));
        crop(&mut doc, [2., 3.], [8., 9.]).unwrap();
        assert_eq!((doc.width, doc.height), (6, 6));
        assert_eq!(doc.layers[0].raster().unwrap().dimensions(), (10, 10));
        assert_eq!(
            doc.layers[0]
                .mask
                .as_ref()
                .unwrap()
                .placement
                .unwrap()
                .origin,
            [-2., -3.]
        );
    }
}
