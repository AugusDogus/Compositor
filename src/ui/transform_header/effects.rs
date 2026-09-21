use super::*;
pub(in crate::ui) struct EffectsDistortion {
    original: Document,
    bounds: Transform,
    corners: [Point; 4],
}
impl EffectsDistortion {
    pub(in crate::ui) fn prepare(&self, current: &Document) -> Result<Document> {
        compositor::effects::distorted_preview(current, &self.original, self.bounds, self.corners)
    }
}
impl Editor {
    pub(in crate::ui) fn effects_distortion_preview(&self) -> Option<EffectsDistortion> {
        if self.tools.mask_target {
            return None;
        }
        let edit = self.transform_edit.as_ref()?;
        let Draft::Perspective {
            original,
            bounds,
            corners,
        } = &edit.current
        else {
            return None;
        };
        let targets = compositor::transform::target_ids(original);
        if !original.layers.iter().any(|l| {
            targets.contains(&l.id) && l.effects.as_ref().is_some_and(|e| !e.visible().is_empty())
        }) {
            return None;
        }
        Some(EffectsDistortion {
            original: original.as_ref().clone(),
            bounds: *bounds,
            corners: *corners,
        })
    }
}
