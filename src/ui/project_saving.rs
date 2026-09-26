//! Serialize saves without blocking edits. Each result acknowledges only its snapshot.
use super::*;
use compositor::invalid;
use std::collections::VecDeque;
use uuid::Uuid;

pub(super) struct SaveRequest {
    pub session: Uuid,
    pub revision: Uuid,
    pub expected: Option<project::Fingerprint>,
    pub document: Document,
    pub path: PathBuf,
}

pub(super) struct SavedSnapshot {
    session: Uuid,
    revision: Uuid,
    fingerprint: project::Fingerprint,
    path: PathBuf,
}

impl SaveRequest {
    pub(super) fn run(self) -> Result<SavedSnapshot> {
        let fingerprint =
            project::save_if_unchanged(&self.document, &self.path, self.expected.as_ref())?;
        Ok(SavedSnapshot {
            session: self.session,
            revision: self.revision,
            fingerprint,
            path: self.path.canonicalize()?,
        })
    }
}

#[derive(Default)]
pub(super) struct Saves {
    pub(super) queue: VecDeque<SaveRequest>,
    running: bool,
    pub close: Option<CloseIntent>,
    resume_close: Option<Uuid>,
}
impl Saves {
    pub fn busy(&self) -> bool {
        self.running || !self.queue.is_empty()
    }
}
impl Editor {
    pub(super) fn queue_save(&mut self, job: SaveRequest) {
        // Repeated Ctrl+S retains the newest requested revision for this destination.
        self.saves
            .queue
            .retain(|queued| queued.session != job.session || queued.path != job.path);
        self.saves.queue.push_back(job);
        self.status = "Saving… You can keep editing.".into();
    }

    pub(super) fn start_save_job(&mut self, cx: &ViewContext<'_, Self>) {
        if self.saves.running {
            return;
        }
        let Some(mut job) = self.saves.queue.pop_front() else {
            return;
        };
        if let Some(current) = self
            .tabs
            .iter()
            .find(|tab| tab.id == job.session)
            .and_then(ProjectTab::session)
            && current.path.as_ref() == Some(&job.path)
        {
            job.expected = current.disk_fingerprint.clone();
        }
        self.saves.running = true;
        let launched = cx.spawn_background(move || job.run(), |this, result, cx| {
            this.saves.running = false;
            let result = result.map_err(|e| invalid(format!("Save worker failed: {e}. Your edits are preserved. Save again to retry.")))
                .and_then(|result| result)
                .and_then(|saved| this.finish_save(saved, cx));
            if result.is_err() {
                this.saves.queue.clear();
                this.saves.close = None;
                this.cancel_close();
            }
            this.operation_result(alerts::Operation::Save, result, cx);
        });
        if let Err(error) = launched {
            self.saves.running = false;
            self.saves.queue.clear();
            self.saves.close = None;
            self.cancel_close();
            self.show_error(alerts::Operation::Save, format!("Could not start saving: {error}. Your edits are preserved. Save again to retry."));
        }
    }

    pub(super) fn finish_save(
        &mut self,
        saved: SavedSnapshot,
        cx: &mut EventContext,
    ) -> Result<()> {
        let SavedSnapshot {
            session: id,
            revision,
            fingerprint,
            path,
        } = saved;
        let session = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == id)
            .and_then(ProjectTab::history_session_mut)
            .ok_or_else(|| {
                invalid(format!(
                    "Saved {} successfully, but its tab has closed.",
                    path.display()
                ))
            })?;
        session.disk_fingerprint = Some(fingerprint);
        session.mark_saved_revision(path.clone(), revision);
        let dirty = session.dirty();
        self.remember_project(path);
        self.status = if dirty {
            "Saved. Newer edits are still unsaved."
        } else {
            "Project saved."
        }
        .into();
        if !self.saves.busy() {
            if self.close_intent.is_some() {
                self.saves.resume_close = Some(id);
                self.resume_saved_close(cx);
            } else if let Some(intent) = self.saves.close.take() {
                self.request_close(intent, cx);
            }
        }
        Ok(())
    }
    /// A completed snapshot cannot acknowledge a gesture or draft that has not
    /// entered history yet. Wait for its UI boundary before reconsidering close.
    pub(super) fn resume_saved_close(&mut self, cx: &mut EventContext) {
        let Some(id) = self.saves.resume_close else {
            return;
        };
        if self.close_intent.is_none() {
            self.saves.resume_close = None;
            return;
        }
        if self.saves.busy()
            || self.recovery.busy()
            || !self.project_transition_ready()
            || self.transform_edit.is_some()
            || self.tools.pending_crop.is_some()
            || self.slider_drag.is_some()
            || self.numeric_scrub.is_some()
        {
            return;
        }
        if let Err(error) = self.finish_pending_edits() {
            self.saves.resume_close = None;
            self.cancel_close();
            self.show_error(alerts::Operation::Save, format!("The saved snapshot is safe, but a newer edit could not be committed: {error}. Your project remains open. Resolve the edit before closing."));
            return;
        }
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            self.saves.resume_close = None;
            self.cancel_close();
            return;
        };
        if tab.session().is_some_and(Session::has_pending_edit) {
            return;
        }
        let needs_save = tab.needs_save();
        self.saves.resume_close = None;
        if needs_save {
            self.modal = Some(Form::confirm_close());
            cx.invalidate();
        } else if let Some(progress) = self.close_intent.take() {
            self.finish_close(progress, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn save_snapshot_allows_edits_and_close_prompts_for_newer_revision() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Snapshot.comp");
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Concurrent save").size(1280., 850.),
                Editor::with_test_document(),
            )
            .unwrap();
        cx.update(view, |e, cx| {
            e.save_to(path.clone(), cx);
            assert!(!e.pending);
            assert!(e.action_available(Action::Fill));
            let job = e.saves.queue.pop_front().unwrap();
            e.saves.running = true;
            e.session_mut()
                .edit("New edit", |doc| {
                    doc.resolution = 144.;
                    Ok(())
                })
                .unwrap();
            e.request_close(CloseIntent::Window, cx);
            assert!(e.saves.close.is_some());
            let saved = job.run().unwrap();
            e.saves.running = false;
            e.finish_save(saved, cx).unwrap();
            assert!(e.session().dirty());
            assert_eq!(project::load(&path).unwrap().resolution, 72.);
            assert!(matches!(e.modal, Some(Form::Close)));
            e.session_mut().undo();
            assert!(!e.session().dirty());
        })
        .unwrap();
        assert!(cx.is_window_open(view.window_handle()));
    }

    #[test]
    fn repeated_saves_coalesce_only_the_same_destination() {
        let mut e = Editor::with_test_document();
        let path = PathBuf::from("first.comp");
        for destination in [path.clone(), PathBuf::from("copy.comp"), path] {
            e.queue_save(SaveRequest {
                session: e.session().id,
                revision: e.session().revision(),
                expected: None,
                document: e.session().document.clone(),
                path: destination,
            });
        }
        assert_eq!(e.saves.queue.len(), 2);
    }
}

#[cfg(test)]
#[path = "project_saving_pending_tests.rs"]
mod pending_tests;
