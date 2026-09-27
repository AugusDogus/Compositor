//! Browse retained committed states without duplicating their document assets.
use super::*;

pub struct HistoryState<'a> {
    pub revision: Uuid,
    pub label: &'a str,
    pub current: bool,
    pub saved: bool,
}

impl Session {
    pub fn history(&self) -> Vec<HistoryState<'_>> {
        let mut states = Vec::with_capacity(self.past.len() + self.future.len() + 1);
        let mut label = "Earliest retained state";
        for entry in &self.past {
            states.push(HistoryState {
                revision: entry.revision,
                label,
                current: false,
                saved: self.saved_revision == Some(entry.revision),
            });
            label = &entry.label;
        }
        states.push(HistoryState {
            revision: self.revision,
            label,
            current: true,
            saved: self.saved_revision == Some(self.revision),
        });
        states.extend(self.future.iter().rev().map(|entry| HistoryState {
            revision: entry.revision,
            label: &entry.label,
            current: false,
            saved: self.saved_revision == Some(entry.revision),
        }));
        states
    }

    /// Revision IDs remain valid when retention evicts older states. Navigation
    /// trims once at the destination, so intermediate states cannot evict it.
    pub fn jump_history(&mut self, revision: Uuid) -> Result<()> {
        if self.has_pending_edit() {
            return Err(invalid(
                "Finish or cancel the current edit before choosing a history state. The document is unchanged.",
            ));
        }
        if revision == self.revision {
            return Ok(());
        }
        if let Some(index) = self
            .past
            .iter()
            .position(|entry| entry.revision == revision)
        {
            for entry in self.past.drain(index..).rev() {
                self.future.push(HistoryEntry {
                    label: entry.label,
                    document: std::mem::replace(&mut self.document, entry.document),
                    revision: std::mem::replace(&mut self.revision, entry.revision),
                });
            }
        } else if let Some(index) = self
            .future
            .iter()
            .position(|entry| entry.revision == revision)
        {
            for entry in self.future.drain(index..).rev() {
                self.past.push(HistoryEntry {
                    label: entry.label,
                    document: std::mem::replace(&mut self.document, entry.document),
                    revision: std::mem::replace(&mut self.revision, entry.revision),
                });
            }
        } else {
            return Err(invalid(
                "This history state is no longer retained. Choose an available state; the document is unchanged.",
            ));
        }
        self.trim_history();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browsing_keeps_saved_revision_and_branches_from_the_chosen_state() {
        let mut session = Session::new(Document::new(3, 2).unwrap(), Some("saved.comp".into()));
        let initial = session.revision();
        session
            .edit("First", |doc| {
                doc.resolution = 100.;
                Ok(())
            })
            .unwrap();
        let first = session.revision();
        session.mark_saved("saved.comp".into());
        session
            .edit("Second", |doc| {
                doc.resolution = 200.;
                Ok(())
            })
            .unwrap();
        let second = session.revision();
        session.jump_history(initial).unwrap();
        assert_eq!(
            session
                .history()
                .iter()
                .map(|state| state.label)
                .collect::<Vec<_>>(),
            ["Earliest retained state", "First", "Second"]
        );
        assert!(session.history()[0].current);
        assert!(session.history()[1].saved);
        assert!(session.dirty());
        session.jump_history(first).unwrap();
        assert_eq!(session.document.resolution, 100.);
        assert!(!session.dirty());
        session.jump_history(second).unwrap();
        assert_eq!(session.document.resolution, 200.);
        assert!(session.dirty());
        session.jump_history(first).unwrap();
        session
            .edit("Branch", |doc| {
                doc.resolution = 300.;
                Ok(())
            })
            .unwrap();
        assert!(session.redo_label().is_none());
        assert!(session.jump_history(second).is_err());
        assert_eq!(session.document.resolution, 300.);
        assert_eq!(session.history().last().unwrap().label, "Branch");
    }

    #[test]
    fn pending_edits_and_evicted_revisions_cannot_be_selected() {
        let mut session = Session::new(Document::new(3, 2).unwrap(), None);
        let initial = session.revision();
        session.begin("Pending").unwrap();
        session.document.resolution = 100.;
        assert!(session.jump_history(initial).is_err());
        assert!(session.has_pending_edit());
        assert_eq!(session.document.resolution, 100.);
        session.cancel();
        for resolution in 100..=205 {
            session
                .edit("Resolution", |doc| {
                    doc.resolution = resolution as f64;
                    Ok(())
                })
                .unwrap();
        }
        assert_eq!(session.history().len(), 101);
        assert!(session.jump_history(initial).is_err());
        assert_eq!(session.document.resolution, 205.);
        let oldest = session.history()[0].revision;
        session.jump_history(oldest).unwrap();
        assert_eq!(session.document.resolution, 105.);
        assert!(session.undo_label().is_none());
    }
}
