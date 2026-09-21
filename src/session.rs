mod history_retention;

use crate::{Result, document::Document, geometry::Point, invalid};
use std::{collections::HashSet, path::PathBuf};
use uuid::Uuid;

#[derive(Clone)]
struct HistoryEntry {
    label: String,
    document: Document,
    revision: Uuid,
}

pub struct Session {
    /// Identifies this open tab even when another tab has the same saved document ID.
    pub id: Uuid,
    pub document: Document,
    pub path: Option<PathBuf>,
    pub zoom: f64,
    pub backing_scale: f64,
    pub pan: Point,
    pub fit: bool,
    pub collapsed: HashSet<Uuid>,
    revision: Uuid,
    saved_revision: Option<Uuid>,
    past: Vec<HistoryEntry>,
    future: Vec<HistoryEntry>,
    pending: Option<HistoryEntry>,
    // The UI owns the welcome/document transition; ordinary history starts at this document.
    creation: Option<String>,
}

impl Session {
    pub fn new(document: Document, path: Option<PathBuf>) -> Self {
        let revision = Uuid::new_v4();
        Self {
            id: Uuid::new_v4(),
            revision,
            saved_revision: path.as_ref().map(|_| revision),
            document,
            path,
            zoom: 1.,
            backing_scale: 1.,
            pan: [0., 0.],
            fit: true,
            collapsed: HashSet::new(),
            past: Vec::new(),
            future: Vec::new(),
            pending: None,
            creation: None,
        }
    }
    pub fn created(document: Document, label: &str) -> Result<Self> {
        document.validate()?;
        let mut session = Self::new(document, None);
        session.creation = Some(label.into());
        Ok(session)
    }

    pub fn creation_label(&self) -> Option<&str> {
        self.creation.as_deref()
    }

    /// Branch from an undone creation without treating a previously saved project as new.
    pub fn replace_creation(&mut self, document: Document, label: &str) -> Result<()> {
        document.validate()?;
        self.document = document;
        self.revision = Uuid::new_v4();
        self.past.clear();
        self.future.clear();
        self.pending = None;
        self.creation = Some(label.into());
        self.fit = true;
        self.pan = [0., 0.];
        Ok(())
    }
    /// A later edit remains unsaved even when its pixels equal the saved revision.
    pub fn dirty(&self) -> bool {
        self.saved_revision != Some(self.revision)
    }

    /// Zoom is measured in display pixels. The anchor and pan are logical view coordinates.
    pub fn zoom_at(&mut self, zoom: f64, anchor_from_center: Point) {
        if !zoom.is_finite() || anchor_from_center.iter().any(|v| !v.is_finite()) {
            return;
        }
        let zoom = zoom.clamp(0.001, 32.);
        let ratio = zoom / self.zoom;
        for (pan, anchor) in self.pan.iter_mut().zip(anchor_from_center) {
            *pan = anchor + (*pan - anchor) * ratio;
        }
        self.zoom = zoom;
        self.fit = false;
    }

    pub fn set_backing_scale(&mut self, scale: f64) {
        if !scale.is_finite() || scale <= 0. || scale == self.backing_scale {
            return;
        }
        for pan in &mut self.pan {
            *pan *= self.backing_scale / scale;
        }
        self.backing_scale = scale;
    }
    pub fn mark_saved(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.saved_revision = Some(self.revision);
    }
    pub fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        format!("{name}{}", if self.dirty() { " *" } else { "" })
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.past.last().map(|e| e.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.future.last().map(|e| e.label.as_str())
    }
    pub fn has_pending_edit(&self) -> bool {
        self.pending.is_some()
    }

    /// Layer selection is navigation, so closing a pixel preview must not roll it back.
    pub fn select_layer(&mut self, id: Uuid, extend: bool) {
        self.document.select(id, extend);
        if let Some(pending) = &mut self.pending {
            pending.document.select(id, extend);
        }
    }

    pub fn begin(&mut self, label: impl Into<String>) -> Result<()> {
        if self.pending.is_some() {
            return Err(invalid(
                "Finish or cancel the current edit before starting another.",
            ));
        }
        self.pending = Some(HistoryEntry {
            label: label.into(),
            document: self.document.clone(),
            revision: self.revision,
        });
        Ok(())
    }
    pub fn commit(&mut self) -> Result<()> {
        if let Err(error) = self.document.validate() {
            self.cancel();
            return Err(error);
        }
        if let Some(entry) = self.pending.take()
            && entry.document != self.document
        {
            self.past.push(entry);
            self.revision = Uuid::new_v4();
            self.future.clear();
            self.trim_history();
        }
        Ok(())
    }
    /// Name a transaction from its final operation, such as an affine drag that became a distortion.
    pub fn commit_named(&mut self, label: &str) -> Result<()> {
        if let Some(entry) = &mut self.pending {
            entry.label = label.into();
        }
        self.commit()
    }
    pub fn cancel(&mut self) {
        if let Some(entry) = self.pending.take() {
            self.document = entry.document;
            self.revision = entry.revision;
        }
    }
    pub fn edit(
        &mut self,
        label: &str,
        edit: impl FnOnce(&mut Document) -> Result<()>,
    ) -> Result<()> {
        self.begin(label)?;
        match edit(&mut self.document) {
            Ok(()) => self.commit(),
            Err(error) => {
                self.cancel();
                Err(error)
            }
        }
    }
    /// The committed image beneath any temporary preview transaction.
    pub fn committed_document(&self) -> &Document {
        self.pending
            .as_ref()
            .map_or(&self.document, |entry| &entry.document)
    }

    /// Record an independent edit beneath a preview without committing or discarding it.
    /// The caller refreshes the visible preview if it is disabled or has no override.
    pub fn edit_committed(
        &mut self,
        label: &str,
        edit: impl FnOnce(&mut Document) -> Result<()>,
    ) -> Result<()> {
        if self.pending.is_none() {
            return self.edit(label, edit);
        }
        let mut updated = self.committed_document().clone();
        edit(&mut updated)?;
        updated.validate()?;
        if let Some(pending) = &mut self.pending
            && updated != pending.document
        {
            self.past.push(HistoryEntry {
                label: label.into(),
                document: std::mem::replace(&mut pending.document, updated),
                revision: std::mem::replace(&mut self.revision, Uuid::new_v4()),
            });
            pending.revision = self.revision;
            self.future.clear();
            self.trim_history();
        }
        Ok(())
    }

    pub fn undo(&mut self) {
        self.cancel();
        self.undo_committed();
    }
    pub fn redo(&mut self) {
        self.cancel();
        self.redo_committed();
    }

    /// Navigate committed history without consuming a temporary preview transaction.
    /// The caller rebuilds the displayed document from the new baseline and preview.
    pub fn undo_committed(&mut self) {
        if let Some(entry) = self.past.pop() {
            let current = self
                .pending
                .as_mut()
                .map_or(&mut self.document, |pending| &mut pending.document);
            self.future.push(HistoryEntry {
                label: entry.label,
                document: std::mem::replace(current, entry.document),
                revision: std::mem::replace(&mut self.revision, entry.revision),
            });
            if let Some(pending) = &mut self.pending {
                pending.revision = self.revision;
            }
            self.trim_history();
        }
    }
    pub fn redo_committed(&mut self) {
        if let Some(entry) = self.future.pop() {
            let current = self
                .pending
                .as_mut()
                .map_or(&mut self.document, |pending| &mut pending.document);
            self.past.push(HistoryEntry {
                label: entry.label,
                document: std::mem::replace(current, entry.document),
                revision: std::mem::replace(&mut self.revision, entry.revision),
            });
            if let Some(pending) = &mut self.pending {
                pending.revision = self.revision;
            }
            self.trim_history();
        }
    }
    pub fn duplicate(&mut self) -> Result<()> {
        self.edit("Duplicate Layers", |doc| {
            if doc.selected.len() == 1
                && let Some(index) = doc
                    .layers
                    .iter()
                    .position(|layer| Some(layer.id) == doc.active && !layer.is_group())
            {
                let mut copy = doc.layers[index].clone();
                copy.id = Uuid::new_v4();
                copy.name.push_str(" copy");
                let id = copy.id;
                doc.layers.insert(index + 1, copy);
                doc.select(id, false);
                return Ok(());
            }
            let mut selected = HashSet::new();
            for id in &doc.selected {
                selected.extend(doc.descendants(*id));
            }
            let mapping: std::collections::HashMap<_, _> =
                selected.iter().map(|id| (*id, Uuid::new_v4())).collect();
            let mut copies = Vec::new();
            for layer in &doc.layers {
                if let Some(id) = mapping.get(&layer.id) {
                    let mut copy = layer.clone();
                    copy.id = *id;
                    copy.name.push_str(" copy");
                    copy.parent = layer
                        .parent
                        .map(|id| mapping.get(&id).copied().unwrap_or(id));
                    copy.clip_source = layer
                        .clip_source
                        .map(|id| mapping.get(&id).copied().unwrap_or(id));
                    copies.push(copy);
                }
            }
            doc.active = doc.active.and_then(|id| mapping.get(&id).copied());
            doc.selected = mapping.values().copied().collect();
            doc.layers.extend(copies);
            Ok(())
        })
    }

    pub fn delete_layers(&mut self) -> Result<()> {
        self.edit(crate::clipping::deletion_label(&self.document), |doc| {
            crate::clipping::delete_selected(doc, crate::clipping::DeleteMode::Unlink)
        })
    }

    pub fn group(&mut self) -> Result<()> {
        self.edit("Group Layers", crate::layer_ops::group)
    }

    pub fn ungroup(&mut self) -> Result<()> {
        self.edit("Ungroup", |doc| {
            let group = doc
                .active_layer()
                .filter(|l| l.is_group())
                .cloned()
                .ok_or_else(|| invalid("Select a group to ungroup."))?;
            if group.mask.is_some() {
                return Err(invalid("Apply or remove the group mask before ungrouping."));
            }
            for layer in &mut doc.layers {
                if layer.parent == Some(group.id) {
                    layer.parent = group.parent;
                }
            }
            doc.layers.retain(|l| l.id != group.id);
            doc.active = doc.layers.last().map(|l| l.id);
            doc.selected = doc.active.into_iter().collect();
            Ok(())
        })
    }

    pub fn reorder(&mut self, upward: bool) -> Result<()> {
        self.edit("Reorder Layers", |doc| {
            let index = doc
                .layers
                .iter()
                .position(|l| Some(l.id) == doc.active)
                .ok_or_else(|| invalid("Select a layer to reorder."))?;
            let parent = doc.layers[index].parent;
            let other = if upward {
                (index + 1..doc.layers.len()).find(|i| doc.layers[*i].parent == parent)
            } else {
                (0..index).rev().find(|i| doc.layers[*i].parent == parent)
            };
            if let Some(other) = other {
                doc.layers.swap(index, other);
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_edits_survive_preview_cancellation_and_keep_separate_history() {
        let doc = Document::new(2, 2).unwrap();
        let mut session = Session::new(doc.clone(), None);
        session.begin("Preview").unwrap();
        session.document.layers[0].name = "Preview layer".into();
        let preview = session.document.clone();
        session
            .edit_committed("Rename underneath", |doc| {
                doc.layers[0].name = "Committed layer".into();
                Ok(())
            })
            .unwrap();
        assert_eq!(session.document, preview);
        assert_eq!(
            session.committed_document().layers[0].name,
            "Committed layer"
        );
        assert_eq!(session.undo_label(), Some("Rename underneath"));
        session.cancel();
        assert_eq!(session.document.layers[0].name, "Committed layer");
        session.undo();
        assert_eq!(session.document, doc);
        session.redo();
        assert_eq!(session.document.layers[0].name, "Committed layer");
    }

    #[test]
    fn rejected_independent_edits_preserve_preview_baseline_and_history() {
        let doc = Document::new(2, 2).unwrap();
        let mut session = Session::new(doc.clone(), None);
        session.begin("Preview").unwrap();
        session.document.layers[0].name = "Preview layer".into();
        let preview = session.document.clone();
        assert!(
            session
                .edit_committed("Invalid", |doc| {
                    doc.width = 0;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(session.document, preview);
        assert_eq!(session.committed_document(), &doc);
        assert!(session.undo_label().is_none());
        session.cancel();
        assert_eq!(session.document, doc);
    }

    #[test]
    fn failed_edits_rollback_and_do_not_enter_history() {
        let mut session = Session::new(Document::new(10, 10).unwrap(), None);
        let before = session.document.clone();
        assert!(
            session
                .edit("Invalid", |doc| {
                    doc.width = 0;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(session.document, before);
        assert_eq!(session.undo_label(), None);
    }
    #[test]
    fn undo_restores_saved_state_and_new_edits_clear_redo() {
        let mut session = Session::new(Document::new(10, 10).unwrap(), Some("test.comp".into()));
        session
            .edit("Move", |doc| {
                doc.layers[0].transform.origin[0] = 3.;
                Ok(())
            })
            .unwrap();
        assert!(session.dirty());
        session.undo();
        assert!(!session.dirty());
        session.redo();
        assert!(session.dirty());
        session.undo();
        session
            .edit("Rename", |doc| {
                doc.layers[0].name = "New".into();
                Ok(())
            })
            .unwrap();
        assert_eq!(session.redo_label(), None);
    }
    #[test]
    fn duplicating_group_remaps_children() {
        let mut session = Session::new(Document::new(10, 10).unwrap(), None);
        session.group().unwrap();
        session.duplicate().unwrap();
        let group = session.document.active.unwrap();
        assert_eq!(session.document.descendants(group).len(), 2);
        assert_eq!(session.document.layers.len(), 4);
        session.document.validate().unwrap();
    }

    #[test]
    fn duplicating_one_layer_inserts_above_its_source_and_keeps_clipping_and_undo() {
        let mut s = Session::new(Document::new(4, 4).unwrap(), None);
        let base = s.document.layers[0].id;
        let mut clipped = crate::document::Layer::blank("Clipped", 4, 4);
        clipped.clip_source = Some(base);
        let id = clipped.id;
        s.document.add(clipped).unwrap();
        s.document
            .add(crate::document::Layer::blank("Top", 4, 4))
            .unwrap();
        s.document.select(id, false);
        let original = s.document.clone();
        s.duplicate().unwrap();
        assert_eq!(s.document.layers[2].name, "Clipped copy");
        assert_eq!(s.document.layers[3].name, "Top");
        assert_eq!(s.document.layers[2].clip_source, Some(base));
        assert_eq!(s.document.active, Some(s.document.layers[2].id));
        assert_eq!(s.document.layers[1], original.layers[1]);
        s.undo();
        assert_eq!(s.document, original);
    }

    #[test]
    fn zoom_preserves_anchor_and_monitor_changes_preserve_center_pixel() {
        let mut s = Session::new(Document::new(1920, 1080).unwrap(), None);
        s.set_backing_scale(2.);
        s.pan = [60., -35.];
        let point = |s: &Session, anchor: Point| {
            [0, 1].map(|axis| (anchor[axis] - s.pan[axis]) / (s.zoom / s.backing_scale))
        };
        let anchor = [-343., -129.];
        let before = point(&s, anchor);
        s.zoom_at(4., anchor);
        assert_eq!(point(&s, anchor), before);
        assert!(!s.fit);
        let center = point(&s, [0., 0.]);
        s.set_backing_scale(1.);
        assert_eq!(point(&s, [0., 0.]), center);
        assert_eq!(s.zoom, 4.);
        s.zoom_at(1000., [0., 0.]);
        assert_eq!(s.zoom, 32.);
        s.zoom_at(0., [0., 0.]);
        assert_eq!(s.zoom, 0.001);
        s.zoom_at(f64::NAN, [0., 0.]);
        assert_eq!(s.zoom, 0.001);
        assert!(s.undo_label().is_none());
    }
    #[test]
    fn preview_does_not_mark_saved_project_dirty_until_committed() {
        let doc = Document::new(8, 8).unwrap();
        let mut session = Session::new(doc, Some("Saved.comp".into()));
        session.begin("Preview").unwrap();
        session.document.layers[0].name = "Preview layer name".into();
        assert!(!session.dirty());
        session.mark_saved("Saved.comp".into());
        assert!(!session.dirty());
        session.commit().unwrap();
        assert!(session.dirty());
        session.undo();
        assert!(!session.dirty());
        session.redo();
        assert!(session.dirty());
    }
}

#[cfg(test)]
mod revision_tests;
