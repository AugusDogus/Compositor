//! Shared rendering and insertion boundaries for explicit clipping links.
use crate::document::{Document, LayerContent};
use std::collections::HashMap;
use uuid::Uuid;

pub(crate) struct Stack {
    pub base: usize,
    pub contiguous: Vec<usize>,
    pub adjustments: Vec<usize>,
}

// Placement reserves the stack even when its base, members, or containing
// folder are hidden. Unrelated hidden siblings and empty folders still do not
// separate a stack. Rendering, by contrast, visits only visible content below.
pub(crate) fn insertion_stack(
    doc: &Document,
    parent: Option<Uuid>,
    insertion: usize,
) -> Option<(usize, usize)> {
    fn visible_content(doc: &Document, parent: Uuid) -> bool {
        doc.layers
            .iter()
            .filter(|l| l.parent == Some(parent) && l.visible)
            .any(|l| !l.is_group() || visible_content(doc, l.id))
    }
    for (index, base) in doc.layers[..insertion].iter().enumerate().filter(|(_, l)| {
        l.parent == parent
            && l.clip_source.is_none()
            && matches!(l.content, LayerContent::Raster(_))
    }) {
        let mut last = index;
        for (child_index, child) in doc
            .layers
            .iter()
            .enumerate()
            .skip(index + 1)
            .filter(|(_, l)| l.parent == parent)
        {
            if child.clip_source == Some(base.id) {
                last = child_index;
            } else if child.visible && (!child.is_group() || visible_content(doc, child.id)) {
                break;
            }
        }
        if insertion <= last {
            return Some((index, last));
        }
    }
    None
}

pub(crate) fn stacks(doc: &Document) -> Vec<Stack> {
    fn visit(doc: &Document, parent: Option<Uuid>, out: &mut Vec<usize>) {
        for (index, layer) in doc
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.parent == parent && l.visible)
        {
            if layer.is_group() {
                visit(doc, Some(layer.id), out);
            } else {
                out.push(index);
            }
        }
    }
    let mut ordered = Vec::new();
    visit(doc, None, &mut ordered);
    let mut adjustments: HashMap<Uuid, Vec<(usize, usize)>> = HashMap::new();
    for (position, index) in ordered.iter().copied().enumerate() {
        let layer = &doc.layers[index];
        if matches!(layer.content, LayerContent::Adjustment(_))
            && let Some(base) = layer.clip_source
        {
            adjustments.entry(base).or_default().push((position, index));
        }
    }
    let mut stacks = Vec::new();
    for (position, index) in ordered.iter().copied().enumerate() {
        let base = &doc.layers[index];
        if base.clip_source.is_some() || matches!(base.content, LayerContent::Adjustment(_)) {
            continue;
        }
        let contiguous: Vec<_> = ordered[position + 1..]
            .iter()
            .copied()
            .take_while(|i| {
                let child = &doc.layers[*i];
                child.clip_source == Some(base.id) && child.parent == base.parent
            })
            .collect();
        // Detached rasters are independent masks. Adjustments have no own pixels
        // and must still operate on their explicit base, in document order.
        let end = position + contiguous.len();
        let detached = adjustments
            .get(&base.id)
            .into_iter()
            .flatten()
            .filter_map(|(position, index)| {
                (*position > end && doc.layers[*index].parent == base.parent).then_some(*index)
            })
            .collect::<Vec<_>>();
        if !contiguous.is_empty() || !detached.is_empty() {
            stacks.push(Stack {
                base: index,
                contiguous,
                adjustments: detached,
            });
        }
    }
    stacks
}
