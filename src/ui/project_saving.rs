//! Serialize saves without blocking edits. Each result acknowledges only its snapshot.
use super::{
    file_jobs::{Completed, FileJob},
    *,
};
use compositor::invalid;
use std::collections::VecDeque;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct Saves {
    pub(super) queue: VecDeque<FileJob>,
    running: bool,
    pub close: Option<CloseIntent>,
}
impl Saves {
    pub fn busy(&self) -> bool {
        self.running || !self.queue.is_empty()
    }
}
impl Editor {
    pub(super) fn queue_save(&mut self, job: FileJob) {
        // Repeated Ctrl+S retains the newest requested revision for this destination.
        if let FileJob::Save { session, path, .. } = &job {
            self.saves.queue.retain(|queued| {
                !matches!(queued,
                FileJob::Save { session: id, path: destination, .. }
                    if id == session && destination == path)
            });
        }
        self.saves.queue.push_back(job);
        self.status = "Saving… You can keep editing.".into();
    }

    pub(super) fn start_save_job(&mut self, cx: &ViewContext<'_, Self>) {
        if self.saves.running {
            return;
        }
        let Some(job) = self.saves.queue.pop_front() else {
            return;
        };
        self.saves.running = true;
        let launched = cx.spawn_background(move || job.run(&[]), |this, result, cx| {
            this.saves.running = false;
            let result = result.map_err(|e| invalid(format!("Save worker failed: {e}. Your edits are preserved. Save again to retry.")))
                .and_then(|result| result)
                .and_then(|completed| match completed {
                    Completed::Saved { session, revision, path } => this.finish_save(session, revision, path, cx),
                    _ => Err(invalid("The save worker returned an unexpected result. Your edits are preserved.")),
                });
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
        id: Uuid,
        revision: Uuid,
        path: PathBuf,
        cx: &mut EventContext,
    ) -> Result<()> {
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
            if let Some(progress) = self.close_intent.take() {
                if dirty {
                    // Editing can continue after clicking Save in a close prompt.
                    // Ask about those newer edits instead of silently discarding them.
                    self.close_intent = Some(progress);
                    self.modal = Some(Form::confirm_close());
                } else {
                    self.finish_close(progress, cx);
                }
            } else if let Some(intent) = self.saves.close.take() {
                self.request_close(intent, cx);
            }
        }
        Ok(())
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
            let Completed::Saved {
                session,
                revision,
                path,
            } = job.run(&[]).unwrap()
            else {
                panic!()
            };
            e.saves.running = false;
            e.finish_save(session, revision, path.clone(), cx).unwrap();
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
            e.queue_save(FileJob::Save {
                session: e.session().id,
                revision: e.session().revision(),
                document: e.session().document.clone(),
                path: destination,
            });
        }
        assert_eq!(e.saves.queue.len(), 2);
    }
}
