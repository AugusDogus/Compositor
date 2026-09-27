//! Move and copy complete layer subtrees while preserving clipping references.
use super::{ancestors, edit, visit};
use crate::{
    Result,
    document::{Document, Layer, LayerContent},
    invalid, render,
};
use std::{collections::HashSet, sync::Arc};
use uuid::Uuid;

#[derive(Clone, Copy)]
pub enum Position {
    Top,
    Above(Uuid),
    Below(Uuid),
}

/// Duplicates the active layer or folder subtree immediately above its source.
/// The caller owns the edit transaction so a subsequent drag can share one undo step.
pub fn duplicate_active(doc: &mut Document) -> Result<()> {
    edit(doc, duplicate_active_in)
}

fn duplicate_active_in(doc: &mut Document) -> Result<()> {
    let index = doc
        .layers
        .iter()
        .position(|layer| Some(layer.id) == doc.active)
        .ok_or_else(|| invalid("Select a layer before duplicating it."))?;
    if doc.layers[index].is_group() {
        let (id, parent) = (doc.layers[index].id, doc.layers[index].parent);
        let mut source = doc.clone();
        source.select(id, false);
        copy_layers(&source, doc, id, true)?;
        let copy = doc
            .active
            .ok_or_else(|| invalid("The duplicated folder is missing."))?;
        place_copies(doc, copy, parent, Position::Above(id))?;
        if let Some(layer) = doc.active_layer_mut() {
            layer.name.push_str(" copy");
        }
        return Ok(());
    }
    if doc.layers.len() >= 10_000 {
        return Err(invalid(
            "Duplicating this layer would exceed the 10,000 layer limit.",
        ));
    }
    let mut copy = doc.layers[index].clone();
    copy.id = Uuid::new_v4();
    copy.name.push_str(" copy");
    let id = copy.id;
    doc.layers.insert(index + 1, copy);
    doc.select(id, false);
    Ok(())
}

/// Duplicate selected roots together above the topmost selected root. Descendants
/// travel with their folder and internal mask links are remapped by copy_layers.
pub fn duplicate_selected(doc: &mut Document) -> Result<()> {
    edit(doc, duplicate_selected_in)
}

fn duplicate_selected_in(doc: &mut Document) -> Result<()> {
    let id = doc
        .active
        .ok_or_else(|| invalid("Select layers before duplicating them."))?;
    let roots = drag_roots(doc, id)?;
    if roots.len() <= 1 {
        return duplicate_active_in(doc);
    }
    let top = *roots
        .last()
        .ok_or_else(|| invalid("Select layers before duplicating them."))?;
    let parent = doc.layer(top).and_then(|l| l.parent);
    let source = doc.clone();
    copy_layers(&source, doc, id, true)?;
    let active = doc
        .active
        .ok_or_else(|| invalid("The duplicated layers are missing."))?;
    let copied = doc.selected.clone();
    for layer in &mut doc.layers {
        if copied.contains(&layer.id) {
            layer.name.push_str(" copy");
        }
    }
    place_copies(doc, active, parent, Position::Above(top))
}

/// The ordered roots a drag carries, excluding descendants of selected folders.
pub fn drag_roots(doc: &Document, id: Uuid) -> Result<Vec<Uuid>> {
    if doc.layer(id).is_none() {
        return Err(invalid("The dragged layer is no longer available."));
    }
    if !doc.selected.contains(&id) {
        return Ok(vec![id]);
    }
    let mut ordered = Vec::new();
    visit(doc, None, &mut ordered);
    ordered.retain(|id| {
        doc.selected.contains(id)
            && !ancestors(doc, *id)
                .iter()
                .flatten()
                .any(|parent| doc.selected.contains(parent))
    });
    Ok(ordered)
}

enum ClippingPlacement {
    NormalizeStack,
    PreserveLinks,
}

pub fn place(doc: &mut Document, id: Uuid, parent: Option<Uuid>, position: Position) -> Result<()> {
    edit(doc, |next| {
        place_with_clipping(
            next,
            id,
            parent,
            position,
            ClippingPlacement::NormalizeStack,
        )
    })
}

/// Copies retain their explicit clipping references. Unlike moving an existing
/// layer, inserting a copy must not change an original layer's clipping stack
/// or silently clip the newly pasted content to the insertion neighbor.
pub(crate) fn place_copies(
    doc: &mut Document,
    id: Uuid,
    parent: Option<Uuid>,
    position: Position,
) -> Result<()> {
    edit(doc, |next| {
        place_with_clipping(next, id, parent, position, ClippingPlacement::PreserveLinks)
    })
}

fn place_with_clipping(
    doc: &mut Document,
    id: Uuid,
    parent: Option<Uuid>,
    position: Position,
    clipping: ClippingPlacement,
) -> Result<()> {
    let roots = drag_roots(doc, id)?;
    let moved: HashSet<_> = roots.iter().flat_map(|id| doc.descendants(*id)).collect();
    let target = match position {
        Position::Top => None,
        Position::Above(id) | Position::Below(id) => Some(id),
    };
    if target.is_some_and(|target| roots.contains(&target)) {
        return Ok(());
    }
    if parent.is_some_and(|p| moved.contains(&p) || doc.layer(p).is_none_or(|l| !l.is_group()))
        || target.is_some_and(|target| moved.contains(&target))
    {
        return Err(invalid(
            "Layers cannot be dropped inside their own subtrees. Choose a destination outside the dragged groups.",
        ));
    }
    if target.is_some_and(|target| doc.layer(target).is_none_or(|l| l.parent != parent)) {
        return Err(invalid(
            "The drop target is no longer in this folder. Try dragging again.",
        ));
    }
    let mut prospective = doc.clone();
    for layer in &mut prospective.layers {
        if roots.contains(&layer.id) {
            layer.parent = parent;
        }
    }
    crate::artboard::validate(&prospective)?;
    let mut layers = Vec::new();
    for root in &roots {
        let mut layer = doc
            .layer(*root)
            .ok_or_else(|| invalid("A selected layer is no longer available."))?
            .clone();
        layer.parent = parent;
        layers.push(layer);
    }
    let roots_set: HashSet<_> = roots.iter().copied().collect();
    doc.layers.retain(|l| !roots_set.contains(&l.id));
    let mut insertion = target
        .and_then(|id| doc.layers.iter().position(|l| l.id == id))
        .map_or(doc.layers.len(), |i| {
            i + usize::from(matches!(position, Position::Above(_)))
        });
    if matches!(clipping, ClippingPlacement::PreserveLinks) {
        insertion = copy_insertion(doc, &layers, insertion, parent, position);
    }
    doc.layers.splice(insertion..insertion, layers);
    doc.active = if roots.contains(&id) {
        Some(id)
    } else {
        roots.last().copied()
    };
    doc.selected = roots_set;
    if matches!(clipping, ClippingPlacement::PreserveLinks) {
        return Ok(());
    }
    // A layer inserted between a clipping base and its children joins that stack.
    let siblings: Vec<_> = doc.layers.iter().filter(|l| l.parent == parent).collect();
    if roots.len() == 1
        && let Some(index) = siblings.iter().position(|l| l.id == id)
        && index > 0
        && index + 1 < siblings.len()
        && !siblings[index].is_group()
        && let Some(source) = siblings[index + 1].clip_source
        && source != id
        && (siblings[index - 1].id == source || siblings[index - 1].clip_source == Some(source))
    {
        doc.layers[insertion].clip_source = Some(source);
    }
    let mut bases = std::collections::HashMap::new();
    for layer in &mut doc.layers {
        let base = bases.entry(layer.parent).or_insert(None);
        if let Some(source) = layer.clip_source {
            if *base != Some(source) {
                layer.clip_source = None;
                *base = Some(layer.id);
            }
        } else {
            *base = if layer.is_group() {
                None
            } else {
                Some(layer.id)
            };
        }
    }
    Ok(())
}

// Independent copies belong outside the destination clipping stack. Inserting
// between its base and children would change the original stack's alpha/blends.
fn copy_insertion(
    doc: &Document,
    copies: &[Layer],
    insertion: usize,
    parent: Option<Uuid>,
    position: Position,
) -> usize {
    if let Some((base, last)) = crate::clipping::insertion_stack(doc, parent, insertion)
        && !copies
            .iter()
            .all(|l| l.clip_source == Some(doc.layers[base].id))
    {
        return if matches!(position, Position::Below(_)) {
            base
        } else {
            last + 1
        };
    }
    insertion
}

/// Copies the dragged selection roots and their subtrees, remapping shared internal references.
pub fn copy_into(source: &Document, destination: &mut Document, id: Uuid) -> Result<()> {
    copy_layers(source, destination, id, false)
}

/// Copies inside one document, retaining clipping links on both original and
/// copied layers. The caller owns the transaction.
pub fn duplicate_to(
    doc: &mut Document,
    id: Uuid,
    parent: Option<Uuid>,
    position: Position,
) -> Result<()> {
    let source = doc.clone();
    edit(doc, |next| {
        copy_layers(&source, next, id, true)?;
        let copied = next
            .active
            .ok_or_else(|| invalid("The copied layer is missing."))?;
        place_with_clipping(
            next,
            copied,
            parent,
            position,
            ClippingPlacement::PreserveLinks,
        )
    })
}

fn copy_layers(
    source: &Document,
    destination: &mut Document,
    id: Uuid,
    retain_external: bool,
) -> Result<()> {
    let roots = drag_roots(source, id)?;
    let ids: HashSet<_> = roots
        .iter()
        .flat_map(|id| source.descendants(*id))
        .collect();
    if destination.layers.len() + ids.len() > 10_000 {
        return Err(invalid(
            "Copying these layers would exceed the 10,000 layer limit.",
        ));
    }
    let mapping: std::collections::HashMap<_, _> =
        ids.iter().map(|id| (*id, Uuid::new_v4())).collect();
    for layer in source.layers.iter().filter(|l| ids.contains(&l.id)) {
        let mut copy = layer.clone();
        copy.id = mapping[&layer.id];
        copy.parent = layer.parent.and_then(|id| mapping.get(&id).copied());
        copy.clip_source = layer.clip_source.and_then(|id| {
            mapping
                .get(&id)
                .copied()
                .or_else(|| retain_external.then_some(id))
        });
        destination.layers.push(copy);
    }
    destination.selected = roots
        .iter()
        .filter_map(|id| mapping.get(id).copied())
        .collect();
    destination.active = roots
        .iter()
        .find(|root| **root == id)
        .or_else(|| roots.last())
        .and_then(|root| mapping.get(root).copied());
    Ok(())
}

/// A project transfer preserves clipping coverage from sources that stay behind,
/// and centers the dragged layer while translating its copied subtree and masks.
pub fn copy_to_project(
    source: &Document,
    destination: &mut Document,
    id: Uuid,
    center: crate::geometry::Point,
) -> Result<()> {
    edit(destination, |next| {
        copy_to_project_in(source, next, id, center)
    })
}

fn copy_to_project_in(
    source: &Document,
    destination: &mut Document,
    id: Uuid,
    center: crate::geometry::Point,
) -> Result<()> {
    if center.iter().any(|v| !v.is_finite()) {
        return Err(invalid(
            "The layer drop position is invalid. Drop onto the canvas again.",
        ));
    }
    let anchor = source
        .layer(id)
        .ok_or_else(|| invalid("The dragged layer no longer exists."))?
        .transform
        .geometry_point([0.5, 0.5]);
    let roots = drag_roots(source, id)?;
    let included: HashSet<_> = roots
        .iter()
        .flat_map(|id| source.descendants(*id))
        .collect();
    let mut prepared = source.clone();
    for layer in prepared
        .layers
        .iter_mut()
        .filter(|l| included.contains(&l.id))
    {
        if layer
            .clip_source
            .is_some_and(|source| !included.contains(&source))
        {
            if layer.is_path_shape() {
                layer.require_rasterized()?;
            }
            if let Some(pixels) = render::clipped_pixels(source, layer)? {
                layer.content = LayerContent::Raster(Some(Arc::new(pixels)));
                layer.shape = None;
                layer.text = None;
                layer.raw = None;
            }
            layer.clip_source = None;
        }
    }
    let start = destination.layers.len();
    copy_into(&prepared, destination, id)?;
    for layer in &mut destination.layers[start..] {
        for axis in 0..2 {
            layer.transform.origin[axis] += center[axis] - anchor[axis];
        }
        if let Some(placement) = layer.mask.as_mut().and_then(|m| m.placement.as_mut()) {
            for axis in 0..2 {
                placement.origin[axis] += center[axis] - anchor[axis];
            }
        }
    }
    let boards: Vec<_> = destination.layers[start..]
        .iter()
        .filter(|layer| layer.is_artboard())
        .map(|layer| layer.id)
        .collect();
    for board in boards {
        let frame = destination
            .layer(board)
            .ok_or_else(|| invalid("The copied artboard is missing."))?
            .transform;
        crate::artboard::make_room(destination, frame)?;
    }
    Ok(())
}
