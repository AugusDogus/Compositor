//! Artboards are top-level structural layers with document-space descendants.
use crate::{
    Result,
    document::{Document, Layer, LayerContent},
    geometry::{Point, Transform},
    invalid,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artboard {
    pub background: [u8; 4],
}

pub fn validate_frame(frame: Transform) -> Result<()> {
    if !frame.valid()
        || frame.warp.is_some()
        || frame.rotation != 0.
        || frame.flip_x
        || frame.flip_y
        || frame.size.iter().any(|size| *size > 30_000.)
    {
        return Err(invalid(
            "An artboard needs an axis-aligned, unflipped frame between 1 and 30000 pixels on each side.",
        ));
    }
    Ok(())
}

/// Includes the board itself. Invalid parent chains terminate without looping.
pub fn owner(document: &Document, layer: Uuid) -> Option<Uuid> {
    let mut current = Some(layer);
    for _ in 0..=64 {
        let layer = document.layer(current?)?;
        if layer.is_artboard() {
            return Some(layer.id);
        }
        current = layer.parent;
    }
    None
}

pub(crate) fn validate(document: &Document) -> Result<()> {
    if !document.layers.iter().any(Layer::is_artboard) {
        return Ok(());
    }
    for layer in &document.layers {
        if layer.is_artboard() {
            validate_frame(layer.transform)?;
            if layer.parent.is_some() {
                return Err(invalid(
                    "Artboards must remain at the top level. Move the artboard out of its folder before saving.",
                ));
            }
        }
        if let Some(source) = layer.clip_source
            && owner(document, layer.id) != owner(document, source)
        {
            return Err(invalid(
                "Clipping masks cannot cross artboard boundaries. Move the complete clipping stack together, or release its clipping mask first. The document is unchanged.",
            ));
        }
    }
    Ok(())
}

fn name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return Err(invalid(
            "Enter an artboard name of 1 to 256 bytes without control characters.",
        ));
    }
    Ok(())
}

fn edit<T>(document: &mut Document, apply: impl FnOnce(&mut Document) -> Result<T>) -> Result<T> {
    document.validate()?;
    let mut next = document.clone();
    let result = apply(&mut next)?;
    next.validate()?;
    *document = next;
    Ok(result)
}

/// Grow around an outlying frame. Negative coordinates translate the entire
/// document right/down, preserving relative content, guides, and saved paths.
pub(crate) fn make_room(document: &mut Document, frame: Transform) -> Result<()> {
    let left = frame.origin[0].floor().min(0.);
    let top = frame.origin[1].floor().min(0.);
    let right = (frame.origin[0] + frame.size[0])
        .ceil()
        .max(f64::from(document.width));
    let bottom = (frame.origin[1] + frame.size[1])
        .ceil()
        .max(f64::from(document.height));
    if [left, top, right, bottom]
        != [
            0.,
            0.,
            f64::from(document.width),
            f64::from(document.height),
        ]
    {
        if right - left > 30_000. || bottom - top > 30_000. {
            return Err(invalid(
                "This artboard would grow the canvas beyond 30000 pixels on a side. Use a smaller frame or move it closer; the document is unchanged.",
            ));
        }
        crate::edits::crop(document, [left, top], [right, bottom])?;
    }
    Ok(())
}

fn insert(
    document: &mut Document,
    name: &str,
    frame: Transform,
    background: [u8; 4],
) -> Result<Uuid> {
    self::name(name)?;
    validate_frame(frame)?;
    let mut layer = Layer::blank(name, 1, 1);
    layer.transform = frame;
    layer.content = LayerContent::Artboard(Artboard { background });
    let id = layer.id;
    document.add(layer)?;
    make_room(document, frame)?;
    Ok(id)
}

pub fn create(
    document: &mut Document,
    name: &str,
    frame: Transform,
    background: [u8; 4],
) -> Result<Uuid> {
    edit(document, |document| {
        insert(document, name, frame, background)
    })
}

pub fn update(
    document: &mut Document,
    id: Uuid,
    name: &str,
    frame: Transform,
    background: [u8; 4],
) -> Result<()> {
    self::name(name)?;
    validate_frame(frame)?;
    edit(document, |document| {
        let old = document
            .layer(id)
            .filter(|layer| layer.is_artboard())
            .ok_or_else(|| invalid("The artboard no longer exists. Select an artboard and retry."))?
            .transform;
        let delta = [
            frame.origin[0] - old.origin[0],
            frame.origin[1] - old.origin[1],
        ];
        let descendants = document.descendants(id);
        for layer in document
            .layers
            .iter_mut()
            .filter(|layer| descendants.contains(&layer.id))
        {
            if layer.id == id
                && old.size != frame.size
                && let Some(mask) = &mut layer.mask
            {
                mask.placement.get_or_insert(old);
            }
            for (axis, amount) in delta.into_iter().enumerate() {
                layer.transform.origin[axis] += amount;
            }
            if let Some(placement) = layer.mask.as_mut().and_then(|mask| mask.placement.as_mut()) {
                for (axis, amount) in delta.into_iter().enumerate() {
                    placement.origin[axis] += amount;
                }
            }
            if layer.id == id {
                layer.name = name.to_owned();
                layer.transform = frame;
                layer.content = LayerContent::Artboard(Artboard { background });
            }
        }
        make_room(document, frame)
    })
}

pub fn translate(document: &mut Document, id: Uuid, delta: Point) -> Result<()> {
    let layer = document
        .layer(id)
        .ok_or_else(|| invalid("The artboard no longer exists."))?;
    let LayerContent::Artboard(board) = layer.content else {
        return Err(invalid("Select an artboard to move."));
    };
    let name = layer.name.clone();
    let mut frame = layer.transform;
    for (axis, amount) in delta.into_iter().enumerate() {
        frame.origin[axis] += amount;
    }
    update(document, id, &name, frame, board.background)
}

/// Resize the clipping frame without moving its content or stretching its mask.
pub fn resize_frame(document: &mut Document, id: Uuid, frame: Transform) -> Result<()> {
    validate_frame(frame)?;
    edit(document, |document| {
        let layer = document
            .layers
            .iter_mut()
            .find(|layer| layer.id == id && layer.is_artboard())
            .ok_or_else(|| {
                invalid("The artboard no longer exists. Select an artboard and retry.")
            })?;
        if let Some(mask) = &mut layer.mask {
            mask.placement.get_or_insert(layer.transform);
        }
        layer.transform = frame;
        make_room(document, frame)
    })
}

pub fn from_selection(document: &mut Document, name: &str, background: [u8; 4]) -> Result<Uuid> {
    edit(document, |document| {
        let active = document
            .active
            .ok_or_else(|| invalid("Select layers to place in an artboard."))?;
        let roots = crate::layer_ops::drag_roots(document, active)?;
        if roots
            .iter()
            .any(|id| document.layer(*id).is_some_and(Layer::is_artboard))
        {
            return Err(invalid(
                "Artboards cannot be nested. Select ordinary layers or folders to create an artboard.",
            ));
        }
        let included: std::collections::HashSet<_> = roots
            .iter()
            .flat_map(|id| document.descendants(*id))
            .collect();
        let mut bounds: Option<[f64; 4]> = None;
        for layer in document.layers.iter().filter(|layer| {
            included.contains(&layer.id) && !layer.is_group() && !layer.is_adjustment()
        }) {
            let next = crate::effects::rendered_transform(layer)?.bounds();
            bounds = Some(bounds.map_or(next, |old| {
                [
                    old[0].min(next[0]),
                    old[1].min(next[1]),
                    old[2].max(next[2]),
                    old[3].max(next[3]),
                ]
            }));
        }
        let bounds = bounds.ok_or_else(|| invalid("The selected layers have no image bounds. Select a pixel, text, shape, or blank layer."))?;
        let origin = [bounds[0].floor(), bounds[1].floor()];
        let frame = Transform {
            origin,
            size: [
                (bounds[2].ceil() - origin[0]).max(1.),
                (bounds[3].ceil() - origin[1]).max(1.),
            ],
            ..Transform::new(1, 1)
        };
        let id = insert(document, name, frame, background)?;
        for layer in &mut document.layers {
            if roots.contains(&layer.id) {
                layer.parent = Some(id);
            }
        }
        Ok(id)
    })
}

#[cfg(test)]
mod tests;
