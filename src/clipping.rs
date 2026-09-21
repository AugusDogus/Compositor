use crate::{
    Result,
    document::{Document, LayerContent},
    render,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use uuid::Uuid;

#[derive(Clone, Copy)]
pub enum DeleteMode {
    Bake,
    Unlink,
}

/// The same eligibility decision drives commands, row clicks and cursor feedback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Create(Uuid),
    Release,
}

pub fn change(doc: &Document, target: Uuid) -> Option<Change> {
    let index = doc.layers.iter().position(|layer| layer.id == target)?;
    let layer = &doc.layers[index];
    if layer.is_group() {
        return None;
    }
    if layer.clip_source.is_some() {
        return Some(Change::Release);
    }
    let below = doc.layers[..index]
        .iter()
        .rev()
        .find(|sibling| sibling.parent == layer.parent)?;
    if below.is_group() {
        return None;
    }
    let source = below.clip_source.unwrap_or(below.id);
    // A clipping base must be a pixel layer, and linking must not create a cycle.
    let mut current = source;
    let mut visited = HashSet::from([target]);
    loop {
        if !visited.insert(current) {
            return None;
        }
        let base = doc.layer(current)?;
        if !matches!(base.content, LayerContent::Raster(_)) {
            return None;
        }
        match base.clip_source {
            Some(next) => current = next,
            None => break,
        }
    }
    Some(Change::Create(source))
}

pub fn toggle(doc: &mut Document, target: Uuid) -> Result<()> {
    let change = change(doc, target).ok_or_else(|| crate::invalid(
        "This layer cannot form a clipping mask. Place it directly above a pixel layer or an existing clipping stack. The document is unchanged."
    ))?;
    let index = doc
        .layers
        .iter()
        .position(|layer| layer.id == target)
        .ok_or_else(|| {
            crate::invalid("The clipping target was removed. Choose an existing layer.")
        })?;
    match change {
        Change::Create(source) => doc.layers[index].clip_source = Some(source),
        Change::Release => {
            let parent = doc.layers[index].parent;
            let source = doc.layers[index].clip_source;
            // Like Swift, release this layer and the contiguous siblings above it
            // sharing the same base. Lower siblings and other folders stay intact.
            for layer in doc.layers[index..]
                .iter_mut()
                .filter(|layer| layer.parent == parent)
            {
                if layer.id != target && layer.clip_source != source {
                    break;
                }
                layer.clip_source = None;
            }
        }
    }
    Ok(())
}

fn removed(doc: &Document) -> HashSet<Uuid> {
    doc.selected
        .iter()
        .flat_map(|id| doc.descendants(*id))
        .collect()
}

pub fn deletion_has_dependents(doc: &Document) -> bool {
    let removed = removed(doc);
    doc.layers.iter().any(|layer| {
        !removed.contains(&layer.id)
            && layer
                .clip_source
                .is_some_and(|source| removed.contains(&source))
    })
}

/// A selected folder is one delete command even though it carries its descendants.
pub fn deletion_label(doc: &Document) -> &'static str {
    if doc.selected.len() > 1 {
        "Delete Layers"
    } else {
        "Delete Layer"
    }
}

pub fn delete_selected(doc: &mut Document, mode: DeleteMode) -> Result<()> {
    let removed = removed(doc);
    let mut baked = HashMap::new();
    if matches!(mode, DeleteMode::Bake) {
        for layer in doc.layers.iter().filter(|layer| {
            !removed.contains(&layer.id)
                && layer
                    .clip_source
                    .is_some_and(|source| removed.contains(&source))
        }) {
            if let Some(pixels) = render::clipped_pixels(doc, layer)? {
                baked.insert(layer.id, Arc::new(pixels));
            }
        }
    }
    let active_index = doc
        .layers
        .iter()
        .position(|l| Some(l.id) == doc.active)
        .unwrap_or(0);
    doc.layers.retain(|l| !removed.contains(&l.id));
    for layer in &mut doc.layers {
        if layer
            .clip_source
            .is_some_and(|source| removed.contains(&source))
        {
            layer.clip_source = None;
            if let Some(pixels) = baked.remove(&layer.id) {
                layer.content = LayerContent::Raster(Some(pixels));
                layer.shape = None;
            }
        }
    }
    if doc.active.is_some_and(|id| removed.contains(&id)) {
        doc.active = doc
            .layers
            .get(active_index.min(doc.layers.len().saturating_sub(1)))
            .map(|l| l.id);
    }
    doc.selected = doc.active.into_iter().collect();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::{Layer, Mask},
        geometry::{Sampling, Transform},
        session::Session,
    };
    use image::{GrayImage, Luma, Rgba, RgbaImage};

    #[test]
    fn clipping_toggle_uses_the_immediate_sibling_and_releases_only_the_upper_stack() {
        let mut doc = Document::new(8, 8).unwrap();
        let base = doc.layers[0].id;
        for name in ["Lower", "Middle", "Upper", "Gap", "Separate"] {
            doc.add(crate::document::Layer::blank(name, 8, 8)).unwrap();
        }
        let lower = doc.layers[1].id;
        let middle = doc.layers[2].id;
        let upper = doc.layers[3].id;
        for id in [lower, middle, upper] {
            assert_eq!(change(&doc, id), Some(Change::Create(base)));
            toggle(&mut doc, id).unwrap();
        }
        doc.layers[5].clip_source = Some(base);
        toggle(&mut doc, middle).unwrap();
        assert_eq!(doc.layers[1].clip_source, Some(base));
        assert_eq!(doc.layers[2].clip_source, None);
        assert_eq!(doc.layers[3].clip_source, None);
        assert_eq!(doc.layers[5].clip_source, Some(base));
        doc.layers[4].content = LayerContent::Group;
        doc.layers[5].clip_source = None;
        assert_eq!(change(&doc, doc.layers[4].id), None);
        assert_eq!(change(&doc, doc.layers[5].id), None);
        assert_eq!(change(&doc, base), None);
        let before = doc.clone();
        assert!(toggle(&mut doc, base).is_err());
        assert_eq!(doc, before);
    }

    #[test]
    fn clipping_rejects_adjustment_bases_and_cycles() {
        let mut doc = Document::new(8, 8).unwrap();
        doc.add(crate::document::Layer::blank("Top", 8, 8)).unwrap();
        let top = doc.layers[1].id;
        doc.layers[0].clip_source = Some(top);
        assert_eq!(change(&doc, top), None);
        doc.layers[0].clip_source = None;
        doc.layers[0].content = LayerContent::Adjustment(Box::new(
            crate::adjustment::Adjustment::new(crate::adjustment::Kind::Levels),
        ));
        assert_eq!(change(&doc, top), None);
    }

    #[test]
    fn baking_a_deleted_clipping_source_preserves_coverage_and_editable_target_appearance() {
        let mut doc = Document::new(20, 20).unwrap();
        let source = doc.layers[0].id;
        doc.layers[0].transform = Transform {
            origin: [5., 6.],
            sampling: Sampling::Nearest,
            ..Transform::new(4, 2)
        };
        doc.layers[0].visible = false;
        doc.layers[0].opacity = 0.5;
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            4,
            2,
            Rgba([100, 200, 40, 128]),
        ))));
        doc.layers[0].mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_fn(4, 2, |x, _| {
                Luma([if x < 2 { 255 } else { 0 }])
            })),
            enabled: true,
            linked: true,
            placement: None,
        });
        let mut target = Layer::blank("Clipped", 4, 2);
        target.transform = doc.layers[0].transform;
        target.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            4,
            2,
            Rgba([230, 40, 80, 200]),
        ))));
        target.opacity = 0.75;
        target.mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(4, 2, Luma([180]))),
            enabled: true,
            linked: true,
            placement: None,
        });
        target.clip_source = Some(source);
        let target_id = target.id;
        doc.add(target.clone()).unwrap();
        doc.select(source, false);
        let original = doc.clone();
        assert!(deletion_has_dependents(&doc));
        let mut session = Session::new(doc, None);
        session
            .edit("Delete Layers", |doc| {
                delete_selected(doc, DeleteMode::Bake)
            })
            .unwrap();
        let baked = session.document.layer(target_id).unwrap();
        assert_eq!(baked.mask, target.mask);
        assert_eq!(baked.opacity, target.opacity);
        assert_eq!(baked.transform, target.transform);
        assert_eq!(baked.clip_source, None);
        assert_eq!(baked.raster().unwrap()[(0, 0)], Rgba([230, 40, 80, 50]));
        assert_eq!(baked.raster().unwrap()[(3, 0)][3], 0);
        for (before, after) in render::render(&original, 20, 20)
            .pixels()
            .zip(render::render(&session.document, 20, 20).pixels())
        {
            assert!(
                before
                    .0
                    .iter()
                    .zip(after.0)
                    .all(|(a, b)| a.abs_diff(b) <= 1)
            );
        }
        session.undo();
        assert_eq!(session.document, original);
        session
            .edit("Unlink", |doc| delete_selected(doc, DeleteMode::Unlink))
            .unwrap();
        assert_eq!(
            session.document.layer(target_id).unwrap().raster(),
            target.raster()
        );
    }
}
