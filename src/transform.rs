use crate::{
    Result,
    document::{Document, LayerContent},
    geometry::{Point, Transform},
    invalid,
};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Handle {
    Resize(usize),
    Rotate,
}

/// Hit targets follow the rotated outline, including the entire length of each edge.
pub fn hit_handle(
    handles: [Point; 8],
    rotation: Option<Point>,
    point: Point,
    zoom: f64,
) -> Option<Handle> {
    if !zoom.is_finite() || zoom <= 0. {
        return None;
    }
    let near = |p: Point| (p[0] - point[0]).hypot(p[1] - point[1]) * zoom <= 10.;
    if rotation.is_some_and(near) {
        return Some(Handle::Rotate);
    }
    if let Some(index) = handles.iter().position(|p| near(*p)) {
        return Some(Handle::Resize(index));
    }
    for (start, end, handle) in [(0, 2, 1), (2, 4, 3), (4, 6, 5), (6, 0, 7)] {
        let a = handles[start];
        let b = handles[end];
        let delta = [b[0] - a[0], b[1] - a[1]];
        let length_squared = delta[0].powi(2) + delta[1].powi(2);
        if length_squared > 0. {
            let t = ((point[0] - a[0]) * delta[0] + (point[1] - a[1]) * delta[1]) / length_squared;
            if (0. ..=1.).contains(&t) && near([a[0] + t * delta[0], a[1] + t * delta[1]]) {
                return Some(Handle::Resize(handle));
            }
        }
    }
    None
}

pub fn selection_bounds(doc: &Document, mask_target: bool) -> Option<Transform> {
    let ids = target_ids(doc);
    if doc.selected.len() == 1 && doc.active_layer().is_some_and(|l| !l.is_group()) {
        let layer = doc.active_layer().filter(|l| ids.contains(&l.id))?;
        if mask_target
            && let Some(mask) = &layer.mask
            && !mask.linked
        {
            return Some(mask.placement.unwrap_or(layer.transform));
        }
        return Some(layer.transform);
    }
    let mut bounds: Option<[f64; 4]> = None;
    for layer in doc
        .layers
        .iter()
        .filter(|l| ids.contains(&l.id) && !l.is_group())
    {
        let b = layer.transform.bounds();
        bounds = Some(bounds.map_or(b, |a| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        }));
    }
    bounds.map(|[x, y, right, bottom]| Transform {
        origin: [x, y],
        size: [(right - x).max(1.), (bottom - y).max(1.)],
        ..Transform::new(1, 1)
    })
}

pub fn selected_ids(doc: &Document) -> HashSet<uuid::Uuid> {
    let mut ids = HashSet::new();
    for id in &doc.selected {
        ids.extend(doc.descendants(*id));
    }
    ids
}

/// Visible raster descendants are the only targets of layer transforms.
/// Structural operations such as merging still use `selected_ids`.
pub fn target_ids(doc: &Document) -> HashSet<uuid::Uuid> {
    let selected = selected_ids(doc);
    doc.layers
        .iter()
        .filter(|l| selected.contains(&l.id) && l.raster().is_some() && doc.layer_is_visible(l.id))
        .map(|l| l.id)
        .collect()
}

/// The Move tool picks by visible layer geometry, including transparent pixels.
/// Automatic picking preserves grouped selections while picking the topmost single layer.
pub fn pick(doc: &Document, point: Point, force: bool) -> Option<uuid::Uuid> {
    let contains = |t: Transform| t.unit(point).iter().all(|v| (0. ..=1.).contains(v));
    if !force {
        let grouped = doc.selected.len() > 1 || doc.active_layer().is_some_and(|l| l.is_group());
        if grouped && selection_bounds(doc, false).is_some_and(contains) {
            return None;
        }
    }
    doc.layers
        .iter()
        .rev()
        .find(|l| l.raster().is_some() && doc.layer_is_visible(l.id) && contains(l.transform))
        .map(|l| l.id)
}

pub fn apply(doc: &mut Document, old: Transform, new: Transform, mask_target: bool) -> Result<()> {
    if !new.valid() {
        return Err(invalid("The transform exceeds supported bounds."));
    }
    if mask_target
        && doc.selected.len() == 1
        && doc.active.is_some_and(|id| target_ids(doc).contains(&id))
        && let Some(layer) = doc.active_layer_mut()
        && let Some(mask) = &mut layer.mask
        && !mask.linked
    {
        mask.placement = Some(new);
        return Ok(());
    }
    let ids = target_ids(doc);
    for layer in doc.layers.iter_mut().filter(|l| ids.contains(&l.id)) {
        let before = layer.transform;
        layer.transform = before.following(old, new);
        if let Some(mask) = &mut layer.mask {
            if mask.linked {
                if let Some(placement) = mask.placement {
                    mask.placement = Some(placement.following(old, new));
                }
            } else if mask.placement.is_none() {
                mask.placement = Some(before);
            }
        }
        if let Some(shape) = layer.shape.filter(|_| layer.transform.size != before.size) {
            let w = layer.transform.size[0].round().max(1.) as u32;
            let h = layer.transform.size[1].round().max(1.) as u32;
            crate::document::validate_size(w, h)?;
            if layer
                .raster()
                .is_some_and(|image| image.dimensions() == (w, h))
            {
                continue;
            }
            // Preserve the mask's document placement when replacing the shape's pixel grid.
            if let Some(mask) = &mut layer.mask
                && mask.placement.is_none()
            {
                mask.placement = Some(layer.transform);
            }
            layer.content = LayerContent::Raster(Some(std::sync::Arc::new(
                crate::edits::shape_pixels(w, h, shape),
            )));
        }
    }
    Ok(())
}

pub fn set_sampling(doc: &mut Document, sampling: crate::geometry::Sampling, mask_target: bool) {
    if mask_target
        && doc.selected.len() == 1
        && doc.active.is_some_and(|id| target_ids(doc).contains(&id))
        && let Some(layer) = doc.active_layer_mut()
        && let Some(mask) = &mut layer.mask
        && !mask.linked
    {
        let mut placement = mask.placement.unwrap_or(layer.transform);
        placement.sampling = sampling;
        mask.placement = Some(placement);
        return;
    }
    let ids = target_ids(doc);
    for layer in doc.layers.iter_mut().filter(|l| ids.contains(&l.id)) {
        layer.transform.sampling = sampling;
    }
}

pub fn drag(
    original: Transform,
    start: Point,
    point: Point,
    handle: Handle,
    lock_ratio: bool,
    shift: bool,
    symmetric: bool,
) -> Transform {
    let mut result = original;
    match handle {
        Handle::Rotate => {
            let center = original.geometry_point([0.5, 0.5]);
            let angle = (point[1] - center[1]).atan2(point[0] - center[0])
                - (start[1] - center[1]).atan2(start[0] - center[0]);
            result.rotation += angle.to_degrees();
            if shift {
                result.rotation = (result.rotation / 15.).round() * 15.;
            }
        }
        Handle::Resize(index) => {
            let Some(handle) = Transform::HANDLES.get(index).copied() else {
                return original;
            };
            let anchor_unit = if symmetric {
                [0.5, 0.5]
            } else {
                [1. - handle[0], 1. - handle[1]]
            };
            let anchor = original.geometry_point(anchor_unit);
            let initial = original.geometry_point(handle);
            let dx = initial[0] + point[0] - start[0] - anchor[0];
            let dy = initial[1] + point[1] - start[1] - anchor[1];
            let (sin, cos) = original.rotation.to_radians().sin_cos();
            let span = if symmetric { 2. } else { 1. };
            let x = (dx * cos + dy * sin) * span;
            let y = (-dx * sin + dy * cos) * span;
            let sx = handle[0] * 2. - 1.;
            let sy = handle[1] * 2. - 1.;
            let mut w = if sx == 0. {
                original.size[0]
            } else {
                (x * sx).max(1.)
            };
            let mut h = if sy == 0. {
                original.size[1]
            } else {
                (y * sy).max(1.)
            };
            if lock_ratio != shift {
                let factor = if sx == 0. {
                    h / original.size[1]
                } else if sy == 0. {
                    w / original.size[0]
                } else {
                    ((x * sx * original.size[0] + y * sy * original.size[1])
                        / (original.size[0].powi(2) + original.size[1].powi(2)))
                    .max(1. / original.size[0].min(original.size[1]))
                };
                w = original.size[0] * factor;
                h = original.size[1] * factor;
            }
            let ox = (0.5 - anchor_unit[0]) * w;
            let oy = (0.5 - anchor_unit[1]) * h;
            result.size = [w, h];
            result.origin = [
                anchor[0] + ox * cos - oy * sin - w / 2.,
                anchor[1] + ox * sin + oy * cos - h / 2.,
            ];
        }
    }
    if result.valid() { result } else { original }
}

pub fn snap(
    doc: &Document,
    original: Transform,
    delta: Point,
    tolerance: f64,
) -> (Point, [Option<f64>; 2]) {
    crate::guides::Settings::default().snap_move(doc, original, delta, tolerance)
}

pub(crate) fn snap_to_targets(
    original: Transform,
    delta: Point,
    tolerance: f64,
    targets: &[Vec<f64>; 2],
) -> (Point, [Option<f64>; 2]) {
    let b = original.bounds();
    let mut offset = delta;
    let mut guides = [None; 2];
    for axis in 0..2 {
        let candidates = [b[axis], (b[axis] + b[axis + 2]) / 2., b[axis + 2]];
        let mut best = tolerance;
        for candidate in candidates {
            for target in &targets[axis] {
                let d = target - candidate - delta[axis];
                if d.abs() < best {
                    best = d.abs();
                    offset[axis] = delta[axis] + d;
                    guides[axis] = Some(*target);
                }
            }
        }
    }
    (offset, guides)
}

#[cfg(test)]
mod tests {
    #[test]
    fn hit_testing_follows_rotated_edges_and_keeps_corner_and_rotation_priority() {
        use super::*;
        for rotation in [0., 33., -87., 180.] {
            let mut bounds = Transform::new(400, 300);
            bounds.origin = [120., -55.];
            bounds.rotation = rotation;
            let handles = Transform::HANDLES.map(|p| bounds.geometry_point(p));
            for (unit, expected) in [
                ([0.2, 0.], 1),
                ([1., 0.3], 3),
                ([0.8, 1.], 5),
                ([0., 0.7], 7),
            ] {
                assert_eq!(
                    hit_handle(handles, None, bounds.geometry_point(unit), 1.),
                    Some(Handle::Resize(expected))
                );
            }
            assert_eq!(
                hit_handle(handles, None, handles[0], 1.),
                Some(Handle::Resize(0))
            );
            assert_eq!(
                hit_handle(handles, Some(handles[0]), handles[0], 1.),
                Some(Handle::Rotate)
            );
            assert_eq!(
                hit_handle(handles, None, bounds.geometry_point([0.3, 0.5]), 1.),
                None
            );
            let near_top = bounds.geometry_point([0.2, -0.02]);
            assert_eq!(
                hit_handle(handles, None, near_top, 1.),
                Some(Handle::Resize(1))
            );
            assert_eq!(hit_handle(handles, None, near_top, 2.), None);
        }
    }
    use super::*;
    #[test]
    fn blank_hidden_and_adjustment_layers_do_not_offer_transforms() {
        let mut doc = Document::new(100, 100).unwrap();
        assert!(selection_bounds(&doc, false).is_none());
        doc.layers[0].content =
            LayerContent::Raster(Some(std::sync::Arc::new(image::RgbaImage::new(100, 100))));
        assert!(selection_bounds(&doc, false).is_some());
        doc.layers[0].visible = false;
        assert!(selection_bounds(&doc, false).is_none());
        doc.layers[0].visible = true;
        doc.layers[0].content = LayerContent::Adjustment(Box::new(
            crate::adjustment::Adjustment::new(crate::adjustment::Kind::Exposure),
        ));
        assert!(selection_bounds(&doc, false).is_none());
    }

    #[test]
    fn group_transforms_use_only_visible_pixel_descendants() {
        use crate::document::Layer;
        let mut doc = Document::new(100, 100).unwrap();
        let mut group = Layer::blank("Folder", 100, 100);
        group.content = LayerContent::Group;
        let group_id = group.id;
        doc.add(group).unwrap();
        doc.layers[0].parent = Some(group_id);
        let mut painted = Layer::blank("Painted", 10, 20);
        painted.parent = Some(group_id);
        painted.content =
            LayerContent::Raster(Some(std::sync::Arc::new(image::RgbaImage::new(10, 20))));
        let painted_id = painted.id;
        doc.add(painted.clone()).unwrap();
        let mut hidden = painted.clone();
        hidden.id = uuid::Uuid::new_v4();
        hidden.visible = false;
        hidden.transform.origin = [200., 300.];
        let hidden_id = hidden.id;
        doc.add(hidden.clone()).unwrap();
        doc.select(group_id, false);
        assert_eq!(selection_bounds(&doc, false), Some(painted.transform));
        let mut moved = painted.transform;
        moved.origin = [30., 40.];
        apply(&mut doc, painted.transform, moved, false).unwrap();
        assert_eq!(doc.layer(painted_id).unwrap().transform, moved);
        assert_eq!(doc.layer(hidden_id).unwrap(), &hidden);
        assert_eq!(doc.layers[0].transform.origin, [0., 0.]);
        assert_eq!(doc.layer(group_id).unwrap().transform.origin, [0., 0.]);
        doc.layers
            .iter_mut()
            .find(|l| l.id == group_id)
            .unwrap()
            .visible = false;
        assert!(selection_bounds(&doc, false).is_none());
    }
    #[test]
    fn shape_resize_keeps_corner_radius_and_mask_placement() {
        use crate::document::Shape;
        let mut doc = Document::new(100, 100).unwrap();
        edits::shape(
            &mut doc,
            [0., 0.],
            [20., 20.],
            Shape {
                geometry: crate::document::ShapeGeometry::Rectangle,
                red: 1.,
                green: 0.,
                blue: 0.,
                corner_radius: 8.,
            },
        )
        .unwrap();
        let index = doc.layers.len() - 1;
        doc.layers[index].mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(20, 20, Luma([255]))),
            enabled: true,
            linked: true,
            placement: None,
        });
        let old = doc.layers[index].transform;
        let new = Transform {
            size: [40., 40.],
            ..old
        };
        apply(&mut doc, old, new, false).unwrap();
        let pixels = doc.layers[index].raster().unwrap();
        assert_eq!(pixels.dimensions(), (40, 40));
        assert_eq!(pixels[(0, 0)][3], 0);
        assert_eq!(pixels[(3, 3)][3], 255);
        assert_eq!(
            doc.layers[index].mask.as_ref().unwrap().placement,
            Some(new)
        );
        let pixels = pixels.clone();
        let fractional = Transform {
            size: [40.1, 40.1],
            ..new
        };
        apply(&mut doc, new, fractional, false).unwrap();
        assert!(Arc::ptr_eq(&pixels, doc.layers[index].raster().unwrap()));
    }
    #[test]
    fn picking_prefers_foreground_and_ignores_hidden_ancestors() {
        let mut doc = Document::new(100, 100).unwrap();
        crate::edits::fill(&mut doc, [100, 50, 25, 255], false, false).unwrap();
        let bottom = doc.layers[0].id;
        let mut top = crate::document::Layer::blank("Transparent", 20, 20);
        top.content =
            LayerContent::Raster(Some(std::sync::Arc::new(image::RgbaImage::new(20, 20))));
        let top_id = top.id;
        doc.add(top).unwrap();
        doc.select(bottom, false);
        assert_eq!(pick(&doc, [10., 10.], false), Some(top_id));
        assert_eq!(pick(&doc, [10., 10.], true), Some(top_id));
        doc.select(top_id, false);
        assert_eq!(pick(&doc, [50., 50.], false), Some(bottom));
        let mut group = crate::document::Layer::blank("Hidden group", 100, 100);
        group.content = LayerContent::Group;
        group.visible = false;
        let group_id = group.id;
        doc.add(group).unwrap();
        doc.layers
            .iter_mut()
            .find(|l| l.id == top_id)
            .unwrap()
            .parent = Some(group_id);
        assert_eq!(pick(&doc, [10., 10.], true), Some(bottom));
    }
    use crate::{document::Mask, edits};
    use image::{GrayImage, Luma};
    use std::sync::Arc;
    #[test]
    fn rotated_resize_keeps_opposite_anchor_at_every_handle() {
        let mut original = Transform::new(200, 100);
        original.rotation = 37.;
        for (index, handle) in Transform::HANDLES.into_iter().enumerate() {
            let anchor = [1. - handle[0], 1. - handle[1]];
            let before = original.geometry_point(anchor);
            let start = original.geometry_point(handle);
            let changed = drag(
                original,
                start,
                [start[0] + 30., start[1] + 40.],
                Handle::Resize(index),
                true,
                false,
                false,
            );
            let after = changed.geometry_point(anchor);
            assert!((after[0] - before[0]).abs() < 1e-8);
            assert!((after[1] - before[1]).abs() < 1e-8);
        }
    }
    #[test]
    fn unlinked_mask_stays_put_while_linked_placement_follows() {
        let mut doc = Document::new(20, 20).unwrap();
        edits::fill(&mut doc, [255; 4], false, false).unwrap();
        doc.layers[0].mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([255]))),
            enabled: true,
            linked: false,
            placement: None,
        });
        let old = doc.layers[0].transform;
        let mut new = old;
        new.origin = [10., 0.];
        apply(&mut doc, old, new, false).unwrap();
        assert_eq!(doc.layers[0].mask.as_ref().unwrap().placement, Some(old));
        doc.layers[0].mask.as_mut().unwrap().linked = true;
        let mut next = new;
        next.origin[0] += 10.;
        apply(&mut doc, new, next, false).unwrap();
        assert_eq!(
            doc.layers[0]
                .mask
                .as_ref()
                .unwrap()
                .placement
                .unwrap()
                .origin,
            [10., 0.]
        );
    }
}
