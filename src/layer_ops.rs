use crate::{
    Result,
    document::{Document, Layer, LayerContent},
    invalid, render,
};
use std::{collections::HashSet, sync::Arc};
use uuid::Uuid;

fn ancestors(doc: &Document, id: Uuid) -> Vec<Option<Uuid>> {
    let mut result = Vec::new();
    let mut parent = doc.layer(id).and_then(|l| l.parent);
    while let Some(id) = parent {
        result.push(Some(id));
        parent = doc.layer(id).and_then(|l| l.parent);
    }
    result.push(None);
    result
}

fn visit(doc: &Document, parent: Option<Uuid>, ids: &mut Vec<Uuid>) {
    for layer in doc.layers.iter().filter(|l| l.parent == parent) {
        ids.push(layer.id);
        if layer.is_group() {
            visit(doc, Some(layer.id), ids);
        }
    }
}

pub fn group(doc: &mut Document) -> Result<()> {
    let roots: HashSet<_> = doc
        .selected
        .iter()
        .copied()
        .filter(|id| {
            !ancestors(doc, *id)
                .iter()
                .flatten()
                .any(|id| doc.selected.contains(id))
        })
        .collect();
    let mut ordered = Vec::new();
    visit(doc, None, &mut ordered);
    ordered.retain(|id| roots.contains(id));
    let parent = ordered.first().and_then(|first| {
        ancestors(doc, *first)
            .into_iter()
            .find(|candidate| {
                ordered
                    .iter()
                    .all(|id| ancestors(doc, *id).contains(candidate))
            })
            .flatten()
    });
    let branches: HashSet<_> = ordered
        .iter()
        .map(|id| {
            let mut branch = *id;
            while let Some(next) = doc.layer(branch).and_then(|l| l.parent) {
                if Some(next) == parent {
                    break;
                }
                branch = next;
            }
            branch
        })
        .collect();
    let insertion = doc
        .layers
        .iter()
        .rposition(|l| branches.contains(&l.id))
        .map_or(doc.layers.len(), |i| {
            doc.layers[..=i]
                .iter()
                .filter(|l| !roots.contains(&l.id))
                .count()
        });
    let mut number = 1;
    while doc
        .layers
        .iter()
        .any(|l| l.name == format!("Folder {number}"))
    {
        number += 1;
    }
    let mut group = Layer::blank(format!("Folder {number}"), doc.width, doc.height);
    group.content = LayerContent::Group;
    group.parent = parent;
    let id = group.id;
    let children: Vec<_> = ordered
        .iter()
        .filter_map(|id| doc.layer(*id).cloned())
        .map(|mut l| {
            l.parent = Some(id);
            l
        })
        .collect();
    doc.add(group)?;
    let group = doc
        .layers
        .pop()
        .ok_or_else(|| invalid("The new folder is missing."))?;
    doc.layers.retain(|l| !roots.contains(&l.id));
    doc.layers.insert(insertion.min(doc.layers.len()), group);
    doc.layers.extend(children);
    doc.select(id, false);
    Ok(())
}

pub fn next_layer_name(doc: &Document) -> String {
    let mut number = 1;
    while doc
        .layers
        .iter()
        .any(|l| l.name == format!("Layer {number}"))
    {
        number += 1;
    }
    format!("Layer {number}")
}

pub fn add_blank(doc: &mut Document) -> Result<()> {
    let active = doc.active_layer();
    let parent = active.and_then(|l| if l.is_group() { Some(l.id) } else { l.parent });
    let mut insertion = doc
        .layers
        .iter()
        .position(|l| Some(l.id) == doc.active)
        .map_or(doc.layers.len(), |i| i + 1);
    if let Some(group) = active.filter(|l| l.is_group()) {
        let descendants = doc.descendants(group.id);
        if let Some(top) = doc.layers.iter().rposition(|l| descendants.contains(&l.id)) {
            insertion = insertion.max(top + 1);
        }
    }
    let mut layer = Layer::blank(next_layer_name(doc), doc.width, doc.height);
    layer.parent = parent;
    doc.add(layer)?;
    if let Some(layer) = doc.layers.pop() {
        doc.layers.insert(insertion, layer);
    }
    Ok(())
}

pub fn move_out_of_group(doc: &mut Document) -> Result<()> {
    let Some(layer) = doc.active_layer() else {
        return Ok(());
    };
    let Some(group) = layer.parent.and_then(|id| doc.layer(id)) else {
        return Ok(());
    };
    let (id, group_id, parent) = (layer.id, group.id, group.parent);
    // This command moves only the active layer, even with a multiple selection.
    doc.select(id, false);
    place(doc, id, parent, Position::Above(group_id))
}

pub fn merge(doc: &mut Document, all: bool) -> Result<()> {
    let active = doc
        .active_layer()
        .ok_or_else(|| invalid("Select layers to merge."))?;
    let mut selected = if all {
        doc.layers.iter().map(|l| l.id).collect()
    } else {
        crate::transform::selected_ids(doc)
    };
    let (name, parent, anchor) = if all {
        ("Merged".to_owned(), None, doc.layers.len())
    } else if doc.selected.len() > 1 {
        let (index, top) = doc
            .layers
            .iter()
            .enumerate()
            .rev()
            .find(|(_, l)| doc.selected.contains(&l.id))
            .ok_or_else(|| invalid("Select layers to merge."))?;
        (top.name.clone(), top.parent, index)
    } else if active.is_group() {
        (
            active.name.clone(),
            active.parent,
            doc.layers
                .iter()
                .position(|l| l.id == active.id)
                .unwrap_or(0),
        )
    } else {
        let index = doc
            .layers
            .iter()
            .position(|l| l.id == active.id)
            .ok_or_else(|| invalid("The selected layer is missing."))?;
        let below = doc.layers[..index]
            .iter()
            .rev()
            .find(|l| l.parent == active.parent)
            .filter(|l| !l.is_group())
            .ok_or_else(|| {
                invalid(
                    "There is no pixel or adjustment layer below this layer in the same folder.",
                )
            })?;
        selected.insert(below.id);
        (below.name.clone(), active.parent, index)
    };
    if !doc
        .layers
        .iter()
        .any(|l| selected.contains(&l.id) && !l.is_group())
    {
        return Err(invalid("The selected folders contain no layers to merge."));
    }
    let mut source = doc.clone();
    source.layers.retain(|l| selected.contains(&l.id));
    for layer in &mut source.layers {
        if layer.parent.is_some_and(|id| !selected.contains(&id)) {
            layer.parent = None;
        }
        if layer.clip_source.is_some_and(|id| !selected.contains(&id)) {
            layer.clip_source = None;
        }
    }
    let region =
        if u64::from(doc.width) * u64::from(doc.height) > crate::document::MAX_SURFACE_PIXELS {
            // Adjustments and masks only modify existing alpha. The union of pixel
            // layer bounds therefore contains the complete merged result.
            let mut b = [doc.width as f64, doc.height as f64, 0., 0.];
            for layer in source
                .layers
                .iter()
                .filter(|layer| layer.raster().is_some())
            {
                let bounds = layer.transform.bounds();
                b = [
                    b[0].min(bounds[0]),
                    b[1].min(bounds[1]),
                    b[2].max(bounds[2]),
                    b[3].max(bounds[3]),
                ];
            }
            b = [
                b[0].floor().max(0.),
                b[1].floor().max(0.),
                b[2].ceil().min(doc.width as f64),
                b[3].ceil().min(doc.height as f64),
            ];
            if b[0] >= b[2] || b[1] >= b[3] {
                [0., 0., 1., 1.]
            } else {
                b
            }
        } else {
            [0., 0., doc.width as f64, doc.height as f64]
        };
    let (width, height) = (
        (region[2] - region[0]) as u32,
        (region[3] - region[1]) as u32,
    );
    crate::document::validate_size(width, height)?;
    let pixels = render::region(&source, width, height, [region[0], region[1]], [1., 1.])?;
    let mut bounds = [width, height, 0, 0];
    for (x, y, p) in pixels.enumerate_pixels() {
        if p[3] > 0 {
            bounds = [
                bounds[0].min(x),
                bounds[1].min(y),
                bounds[2].max(x + 1),
                bounds[3].max(y + 1),
            ];
        }
    }
    if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
        bounds = [0, 0, width, height];
    }
    let [left, top, right, bottom] = bounds;
    let mut layer = Layer::blank(name, right - left, bottom - top);
    layer.parent = parent;
    layer.transform.origin = [region[0] + left as f64, region[1] + top as f64];
    layer.content = LayerContent::Raster(Some(Arc::new(
        image::imageops::crop_imm(&pixels, left, top, right - left, bottom - top).to_image(),
    )));
    let id = layer.id;
    let insertion = doc.layers[..anchor]
        .iter()
        .filter(|l| !selected.contains(&l.id))
        .count();
    doc.layers.retain(|l| !selected.contains(&l.id));
    for layer in &mut doc.layers {
        if layer.clip_source.is_some_and(|id| selected.contains(&id)) {
            layer.clip_source = Some(id);
        }
    }
    doc.layers.insert(insertion.min(doc.layers.len()), layer);
    doc.select(id, false);
    Ok(())
}

#[derive(Clone, Copy)]
pub enum Position {
    Top,
    Above(Uuid),
    Below(Uuid),
}

/// Duplicates the active layer or folder subtree immediately above its source.
/// The caller owns the edit transaction so a subsequent drag can share one undo step.
pub fn duplicate_active(doc: &mut Document) -> Result<()> {
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
    let id = doc
        .active
        .ok_or_else(|| invalid("Select layers before duplicating them."))?;
    let roots = drag_roots(doc, id)?;
    if roots.len() <= 1 {
        return duplicate_active(doc);
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
    place_with_clipping(doc, id, parent, position, ClippingPlacement::NormalizeStack)
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
    place_with_clipping(doc, id, parent, position, ClippingPlacement::PreserveLinks)
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
    for stack in crate::clipping::stacks(doc) {
        let base = &doc.layers[stack.base];
        let Some(last) = stack.contiguous.last().copied() else {
            continue;
        };
        if base.parent == parent
            && stack.base < insertion
            && insertion <= last
            && !copies.iter().all(|l| l.clip_source == Some(base.id))
        {
            return if matches!(position, Position::Below(_)) {
                stack.base
            } else {
                last + 1
            };
        }
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
    copy_layers(&source, doc, id, true)?;
    let copied = doc
        .active
        .ok_or_else(|| invalid("The copied layer is missing."))?;
    place_copies(doc, copied, parent, position)
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{edits, session::Session};
    use image::Rgba;

    #[test]
    fn duplicating_multiple_layers_preserves_original_and_copied_clipping() {
        let mut doc = Document::new(8, 8).unwrap();
        let a = doc.active.unwrap();
        let mut b = Layer::blank("B", 8, 8);
        b.clip_source = Some(a);
        doc.add(b).unwrap();
        let b = doc.active.unwrap();
        doc.add(Layer::blank("C", 8, 8)).unwrap();
        let c = doc.active.unwrap();
        doc.active_layer_mut().unwrap().content =
            LayerContent::Raster(Some(Arc::new(image::RgbaImage::from_fn(8, 8, |x, _| {
                Rgba([30, 40, 50, if x < 4 { 255 } else { 0 }])
            }))));
        let mut d = Layer::blank("D", 8, 8);
        d.content = LayerContent::Raster(Some(Arc::new(image::RgbaImage::from_pixel(
            8,
            8,
            Rgba([200, 30, 10, 255]),
        ))));
        d.clip_source = Some(c);
        doc.add(d).unwrap();
        doc.select(b, false);
        doc.select(c, true);
        let originals = doc.layers.clone();
        let before = render::render(&doc, 8, 8).unwrap();
        duplicate_selected(&mut doc).unwrap();
        for original in originals {
            assert_eq!(doc.layer(original.id), Some(&original));
        }
        let copied_b = doc.layers.iter().find(|l| l.name == "B copy").unwrap();
        assert_eq!(copied_b.clip_source, Some(a));
        doc.validate().unwrap();
        for layer in &mut doc.layers {
            if doc.selected.contains(&layer.id) {
                layer.visible = false;
            }
        }
        assert_eq!(render::render(&doc, 8, 8).unwrap(), before);
    }

    #[test]
    fn cross_project_copy_centers_pixels_and_positioned_masks_and_bakes_external_clipping() {
        let mut source = Document::new(100, 80).unwrap();
        edits::fill(&mut source, [100, 200, 20, 128], false, false).unwrap();
        let base = source.layers[0].id;
        let mut layer = Layer::blank("Clipped", 4, 2);
        layer.transform.origin = [10., 20.];
        layer.clip_source = Some(base);
        layer.content = LayerContent::Raster(Some(Arc::new(image::RgbaImage::from_pixel(
            4,
            2,
            Rgba([200, 40, 80, 255]),
        ))));
        layer.mask = Some(crate::document::Mask {
            pixels: Arc::new(image::GrayImage::from_pixel(2, 2, image::Luma([180]))),
            enabled: true,
            linked: false,
            placement: Some(crate::geometry::Transform {
                origin: [8., 19.],
                ..crate::geometry::Transform::new(6, 4)
            }),
        });
        layer.opacity = 0.7;
        let id = layer.id;
        source.add(layer.clone()).unwrap();
        let original = source.clone();
        let mut session = Session::new(Document::new(200, 160).unwrap(), None);
        let before = session.document.clone();
        session
            .edit("Copy", |doc| copy_to_project(&source, doc, id, [100., 80.]))
            .unwrap();
        let copy = session.document.active_layer().unwrap();
        assert_eq!(copy.transform.geometry_point([0.5, 0.5]), [100., 80.]);
        assert_eq!(
            copy.mask.as_ref().unwrap().placement.unwrap().origin,
            [96., 78.]
        );
        assert_eq!(
            copy.mask.as_ref().unwrap().pixels,
            layer.mask.as_ref().unwrap().pixels
        );
        assert_eq!(copy.opacity, 0.7);
        assert_eq!(copy.clip_source, None);
        assert_eq!(copy.raster().unwrap()[(0, 0)], Rgba([200, 40, 80, 128]));
        assert_eq!(source, original);
        session.undo();
        assert_eq!(session.document, before);
    }

    #[test]
    fn blank_layers_use_unique_names_and_folder_placement_and_can_move_out_with_undo() {
        let mut session = Session::new(Document::new(4, 4).unwrap(), None);
        let original = session.document.active.unwrap();
        session.group().unwrap();
        let group = session.document.active.unwrap();
        session.edit("New Layer", add_blank).unwrap();
        let added = session.document.active.unwrap();
        assert_eq!(session.document.layer(added).unwrap().parent, Some(group));
        assert_ne!(
            session.document.layer(added).unwrap().name,
            session.document.layer(original).unwrap().name
        );
        assert_eq!(session.document.layers.last().unwrap().id, added);
        session.document.select(original, false);
        session.edit("New Layer", add_blank).unwrap();
        let middle = session.document.active.unwrap();
        let siblings: Vec<_> = session
            .document
            .layers
            .iter()
            .filter(|l| l.parent == Some(group))
            .map(|l| l.id)
            .collect();
        assert_eq!(siblings, [original, middle, added]);
        session.document.select(original, true);
        let before = session.document.clone();
        session.edit("Move Out", move_out_of_group).unwrap();
        assert_eq!(session.document.layer(original).unwrap().parent, None);
        assert_eq!(session.document.layer(middle).unwrap().parent, Some(group));
        assert_eq!(session.document.layer(added).unwrap().parent, Some(group));
        assert_eq!(session.document.selected, HashSet::from([original]));
        assert_eq!(session.document.layers[1].id, original);
        session.document.validate().unwrap();
        session.undo();
        assert_eq!(session.document, before);
    }

    #[test]
    fn moving_selected_clipping_stack_preserves_order_references_and_selection() {
        let mut doc = Document::new(4, 4).unwrap();
        let base = doc.active.unwrap();
        let mut clip = Layer::blank("Clipped", 4, 4);
        clip.clip_source = Some(base);
        let clip_id = clip.id;
        doc.add(clip).unwrap();
        let target = Layer::blank("Target", 4, 4);
        let target_id = target.id;
        doc.add(target).unwrap();
        doc.select(base, false);
        doc.select(clip_id, true);
        place(&mut doc, clip_id, None, Position::Above(target_id)).unwrap();
        assert_eq!(
            doc.layers.iter().map(|l| l.id).collect::<Vec<_>>(),
            vec![target_id, base, clip_id]
        );
        assert_eq!(doc.layer(clip_id).unwrap().clip_source, Some(base));
        assert_eq!(doc.selected, HashSet::from([base, clip_id]));
        doc.validate().unwrap();
    }

    #[test]
    fn copying_selected_group_and_child_copies_each_layer_once() {
        let mut session = Session::new(Document::new(4, 4).unwrap(), None);
        let child = session.document.active.unwrap();
        session.group().unwrap();
        let group = session.document.active.unwrap();
        session.document.select(child, true);
        let sibling = Layer::blank("Sibling", 4, 4);
        let sibling_id = sibling.id;
        session.document.add(sibling).unwrap();
        session.document.selected = HashSet::from([group, child, sibling_id]);
        let source = session.document.clone();
        let mut target = Document::new(4, 4).unwrap();
        copy_into(&source, &mut target, child).unwrap();
        assert_eq!(target.layers.len(), 4);
        assert_eq!(target.selected.len(), 2);
        let copied_group = target.layers.iter().find(|l| l.is_group()).unwrap();
        assert_eq!(
            target
                .layers
                .iter()
                .filter(|l| l.parent == Some(copied_group.id))
                .count(),
            1
        );
        target.validate().unwrap();
        assert_eq!(session.document, source);
    }

    #[test]
    fn placing_layers_preserves_subtrees_and_rejects_cycles() {
        let mut session = Session::new(Document::new(4, 4).unwrap(), None);
        let child = session.document.active.unwrap();
        session.group().unwrap();
        let inner = session.document.active.unwrap();
        session.group().unwrap();
        let outer = session.document.active.unwrap();
        let before = session.document.clone();
        assert!(place(&mut session.document, outer, Some(inner), Position::Top).is_err());
        assert_eq!(session.document, before);
        place(&mut session.document, inner, None, Position::Above(outer)).unwrap();
        assert_eq!(session.document.layer(child).unwrap().parent, Some(inner));
        assert_eq!(session.document.layer(inner).unwrap().parent, None);
        session.document.validate().unwrap();
    }

    #[test]
    fn cross_project_copy_remaps_clipping_and_parent_ids_and_keeps_assets() {
        let mut session = Session::new(Document::new(4, 4).unwrap(), None);
        edits::fill(&mut session.document, [255; 4], false, false).unwrap();
        let base = session.document.active.unwrap();
        let mut top = Layer::blank("Clipped", 4, 4);
        top.clip_source = Some(base);
        session.document.add(top).unwrap();
        session.document.selected.insert(base);
        session.group().unwrap();
        let source = session.document.clone();
        let mut target = Document::new(4, 4).unwrap();
        copy_into(&source, &mut target, source.active.unwrap()).unwrap();
        let group = target.active.unwrap();
        assert_eq!(target.descendants(group).len(), 3);
        assert!(!source.layers.iter().any(|l| target.layer(l.id).is_some()));
        let cloned_base = target.layers.iter().find(|l| l.raster().is_some()).unwrap();
        let cloned_top = target.layers.iter().find(|l| l.name == "Clipped").unwrap();
        assert_eq!(cloned_top.clip_source, Some(cloned_base.id));
        assert!(Arc::ptr_eq(
            cloned_base.raster().unwrap(),
            source.layer(base).unwrap().raster().unwrap()
        ));
        assert_eq!(source, session.document);
        target.validate().unwrap();
    }

    #[test]
    fn merging_noncontiguous_layers_uses_top_selected_slot_and_trims_pixels() {
        let mut doc = Document::new(10, 10).unwrap();
        edits::fill(&mut doc, [255, 0, 0, 255], false, false).unwrap();
        let lower = doc.layers[0].id;
        doc.add(Layer::blank("Middle", 10, 10)).unwrap();
        let middle = doc.active.unwrap();
        doc.add(Layer::blank("Top", 2, 2)).unwrap();
        edits::fill(&mut doc, [0, 0, 255, 255], false, false).unwrap();
        doc.selected.insert(lower);
        merge(&mut doc, false).unwrap();
        assert_eq!(doc.layers[0].id, middle);
        assert_eq!(doc.layers[1].name, "Top");
        assert_eq!(
            doc.layers[1].raster().unwrap()[(0, 0)],
            Rgba([0, 0, 255, 255])
        );
        doc.validate().unwrap();
    }

    #[test]
    fn merging_group_keeps_parent_and_trims_transparent_margins() {
        let mut session = Session::new(Document::new(10, 10).unwrap(), None);
        session.group().unwrap();
        let inner = session.document.active.unwrap();
        session.group().unwrap();
        let outer = session.document.active.unwrap();
        let doc = &mut session.document;
        let child = doc.layers.iter_mut().find(|l| !l.is_group()).unwrap();
        child.content = LayerContent::Raster(Some(Arc::new(image::RgbaImage::from_pixel(
            1,
            1,
            Rgba([255; 4]),
        ))));
        child.transform = crate::geometry::Transform::new(1, 1);
        child.transform.origin = [3., 4.];
        doc.select(inner, false);
        merge(doc, false).unwrap();
        let merged = doc.active_layer().unwrap();
        assert_eq!(merged.parent, Some(outer));
        assert_eq!(merged.transform.origin, [3., 4.]);
        assert_eq!(merged.raster().unwrap().dimensions(), (1, 1));
        doc.validate().unwrap();
    }

    #[test]
    fn grouping_across_folders_uses_common_parent_and_keeps_selected_subtrees() {
        let mut session = Session::new(Document::new(10, 10).unwrap(), None);
        session.group().unwrap();
        let folder = session.document.active.unwrap();
        let child = session
            .document
            .layers
            .iter()
            .find(|l| !l.is_group())
            .unwrap()
            .id;
        session
            .document
            .add(Layer::blank("Sibling", 10, 10))
            .unwrap();
        let sibling = session.document.active.unwrap();
        session.document.selected.extend([folder, child]);
        session.group().unwrap();
        let wrapper = session.document.active.unwrap();
        assert_eq!(
            session.document.layer(folder).unwrap().parent,
            Some(wrapper)
        );
        assert_eq!(
            session.document.layer(sibling).unwrap().parent,
            Some(wrapper)
        );
        assert_eq!(session.document.layer(child).unwrap().parent, Some(folder));
        session.document.validate().unwrap();
    }
}
