//! Editable layer snapshots shared by native clipboard and cross-project paste.
use crate::{Result, document::Document, layer_ops};
use uuid::Uuid;

#[derive(Clone)]
pub struct Layers {
    source_id: Uuid,
    top: Uuid,
    document: Document,
}
impl Layers {
    pub fn capture(source: &Document) -> Result<Self> {
        let id = source
            .active
            .ok_or_else(|| crate::invalid("Select layers before copying."))?;
        let top = *layer_ops::drag_roots(source, id)?
            .last()
            .ok_or_else(|| crate::invalid("Select layers before copying."))?;
        let mut document = Document::new(source.width, source.height)?;
        document.layers.clear();
        document.selected.clear();
        document.active = None;
        let center = source
            .layer(id)
            .ok_or_else(|| crate::invalid("The selected layer is missing."))?
            .transform
            .geometry_point([0.5, 0.5]);
        layer_ops::copy_to_project(source, &mut document, id, center)?;
        document.resolution = source.resolution;
        Ok(Self {
            source_id: source.id,
            top,
            document,
        })
    }
    pub fn pixels(&self) -> Result<image::RgbaImage> {
        crate::render::render(&self.document, self.document.width, self.document.height)
    }
    pub fn paste(&self, target: &mut Document) -> Result<()> {
        let id = self
            .document
            .active
            .ok_or_else(|| crate::invalid("The copied layers are missing."))?;
        let source = self
            .document
            .layer(id)
            .ok_or_else(|| crate::invalid("The copied layer is missing."))?;
        let same = self.source_id == target.id;
        let above = if same && target.layer(self.top).is_some() {
            Some(self.top)
        } else {
            target.active
        };
        let parent = above.and_then(|id| target.layer(id)).and_then(|l| l.parent);
        let center = if same {
            source.transform.geometry_point([0.5, 0.5])
        } else {
            [target.width as f64 / 2., target.height as f64 / 2.]
        };
        layer_ops::copy_to_project(&self.document, target, id, center)?;
        if let Some(active) = target.active {
            layer_ops::place_copies(
                target,
                active,
                parent,
                above.map_or(layer_ops::Position::Top, layer_ops::Position::Above),
            )?;
        }
        target.validate()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pasting_respects_rendered_stacks_across_hidden_layers_and_empty_folders() {
        use crate::document::{Layer, LayerContent};
        for folder in [false, true] {
            let mut target = Document::new(1, 1).unwrap();
            crate::edits::fill(&mut target, [255, 0, 0, 128], false, false).unwrap();
            let base = target.active.unwrap();
            let mut between = Layer::blank("Ignored by renderer", 1, 1);
            if folder {
                between.content = LayerContent::Group;
            } else {
                between.visible = false;
            }
            target.add(between).unwrap();
            target.add(Layer::blank("Clipped", 1, 1)).unwrap();
            crate::edits::fill(&mut target, [0, 0, 255, 128], false, false).unwrap();
            target.active_layer_mut().unwrap().clip_source = Some(base);
            let before = crate::render::render(&target, 1, 1).unwrap();
            let clipboard = Layers::capture(&Document::new(1, 1).unwrap()).unwrap();
            target.select(base, false);
            clipboard.paste(&mut target).unwrap();
            assert_eq!(crate::render::render(&target, 1, 1).unwrap(), before);
        }
    }

    #[test]
    fn pasting_empty_content_does_not_disable_a_clipped_adjustment() {
        let mut target = Document::new(8, 8).unwrap();
        crate::edits::fill(&mut target, [255, 0, 0, 128], false, false).unwrap();
        let base = target.active.unwrap();
        let mut invert = crate::document::Layer::blank("Invert", 8, 8);
        invert.content = crate::document::LayerContent::Adjustment(Box::new(
            crate::adjustment::Adjustment::new(crate::adjustment::Kind::Invert),
        ));
        invert.clip_source = Some(base);
        target.add(invert).unwrap();
        let before = crate::render::render(&target, 8, 8).unwrap();
        assert_eq!(before[(4, 4)], image::Rgba([0, 255, 255, 128]));
        let clipboard = Layers::capture(&Document::new(8, 8).unwrap()).unwrap();
        target.select(base, false);
        clipboard.paste(&mut target).unwrap();
        assert_eq!(crate::render::render(&target, 8, 8).unwrap(), before);
    }

    #[test]
    fn paste_above_a_clipping_base_preserves_the_existing_stack() {
        let mut source = Document::new(8, 8).unwrap();
        source.layers[0].name = "Source".into();
        let clipboard = Layers::capture(&source).unwrap();
        let mut target = Document::new(8, 8).unwrap();
        let base = target.active.unwrap();
        let mut clipped = crate::document::Layer::blank("Clipped", 8, 8);
        clipped.clip_source = Some(base);
        target.add(clipped.clone()).unwrap();
        target.select(base, false);
        let originals = target.layers.clone();
        clipboard.paste(&mut target).unwrap();
        for original in originals {
            assert_eq!(target.layer(original.id), Some(&original));
        }
        assert_eq!(target.active_layer().unwrap().clip_source, None);
        target.validate().unwrap();
    }

    #[test]
    fn copying_selected_folder_preserves_text_effects_and_internal_links() {
        let mut doc = Document::new(20, 20).unwrap();
        let a = doc.active.unwrap();
        doc.add(crate::document::Layer::blank("Top", 20, 20))
            .unwrap();
        let b = doc.active.unwrap();
        doc.layers[1].clip_source = Some(a);
        doc.selected.insert(a);
        layer_ops::group(&mut doc).unwrap();
        let folder = doc.active.unwrap();
        doc.selected.insert(b);
        let clip = Layers::capture(&doc).unwrap();
        let original = doc.clone();
        clip.paste(&mut doc).unwrap();
        assert_eq!(doc.layers.len(), 6);
        assert_eq!(doc.selected.len(), 1);
        assert_ne!(doc.active, Some(folder));
        let copied_top = doc
            .layers
            .iter()
            .find(|l| l.name == "Top" && l.id != b)
            .unwrap();
        assert_ne!(copied_top.clip_source, Some(a));
        assert!(copied_top.clip_source.is_some());
        let mut other = Document::new(40, 40).unwrap();
        clip.paste(&mut other).unwrap();
        assert_eq!(other.layers.len(), 4);
        other.validate().unwrap();
        assert_eq!(original.layers.len(), 3);
    }
}
