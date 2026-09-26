//! Observe saved packages and preserve edits when another process replaces them.
use super::*;
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};
use uuid::Uuid;

pub(super) struct ExternalProjects {
    next_check: Instant,
    running: bool,
    changes: VecDeque<Change>,
    ignored: HashMap<Uuid, project::Fingerprint>,
}
impl Default for ExternalProjects {
    fn default() -> Self {
        Self {
            next_check: Instant::now() + Duration::from_secs(2),
            running: false,
            changes: VecDeque::new(),
            ignored: HashMap::new(),
        }
    }
}
struct Change {
    id: Uuid,
    baseline: Option<project::Fingerprint>,
    path: PathBuf,
    fingerprint: project::Fingerprint,
    document: Document,
}

impl Editor {
    pub(super) fn monitor_projects(&mut self, cx: &mut ViewContext<'_, Self>) {
        if cfg!(test) {
            return;
        }
        {
            if self.external_projects.running {
                return;
            }
            if Instant::now() < self.external_projects.next_check {
                cx.request_repaint_at(self.external_projects.next_check);
                return;
            }
            self.external_projects.next_check = Instant::now() + Duration::from_secs(2);
            cx.request_repaint_at(self.external_projects.next_check);
            if self.saves.busy() || self.pending {
                return;
            }
            let projects: Vec<_> = self
                .tabs
                .iter()
                .filter_map(|tab| {
                    let session = tab.session()?;
                    Some((
                        tab.id,
                        session.path.clone()?,
                        session.disk_fingerprint.clone(),
                        self.external_projects.ignored.get(&tab.id).cloned(),
                    ))
                })
                .collect();
            if projects.is_empty() {
                return;
            }
            self.external_projects.running = true;
            let result = cx.spawn_background(move || {
                projects.into_iter().filter_map(|(id, path, baseline, ignored)| {
                    let fingerprint = match project::fingerprint(&path) {
                        Ok(Some(fingerprint)) => fingerprint,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                    if Some(&fingerprint) == baseline.as_ref() || Some(&fingerprint) == ignored.as_ref() { return None; }
                    Some(project::load_verified(&path).map(|(document, fingerprint)| Change { id, baseline, path, fingerprint, document }))
                }).collect::<Vec<_>>()
            }, |this, result, cx| {
                this.external_projects.running = false;
                let changed = result.as_ref().map_or(true, |results| !results.is_empty());
                match result {
                    Ok(results) => {
                    for result in results {
                        match result {
                            Ok(change) => this.receive_external_change(change),
                            Err(error) => this.status = format!("External project could not be reloaded: {error} Your open edits are preserved."),
                        }
                    }
                }
                    Err(error) => this.status = format!("External project check failed: {error}. Your open edits are preserved."),
                }
                if changed { this.changed(cx); }
            });
            if let Err(error) = result {
                self.external_projects.running = false;
                self.status = format!("Could not check external project changes: {error}");
            }
        }
    }
    fn receive_external_change(&mut self, change: Change) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == change.id) else {
            return;
        };
        let Some(session) = self.tabs[index].session() else {
            return;
        };
        if session.path.as_ref() != Some(&change.path)
            || session.disk_fingerprint != change.baseline
            || session.disk_fingerprint.as_ref() == Some(&change.fingerprint)
            || project::fingerprint(&change.path).ok().flatten().as_ref()
                != Some(&change.fingerprint)
        {
            return;
        }
        // A save, active gesture or preview can have started while the loader ran.
        let idle = !self.saves.busy()
            && !self.pending
            && self.can_switch_projects()
            && !session.dirty()
            && !session.has_pending_edit();
        if idle {
            self.reload_external(change);
        } else {
            self.external_projects
                .ignored
                .insert(change.id, change.fingerprint.clone());
            self.external_projects
                .changes
                .retain(|queued| queued.id != change.id);
            self.external_projects.changes.push_back(change);
        }
    }
    fn reload_external(&mut self, change: Change) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == change.id) else {
            return;
        };
        let Some(old) = self.tabs[index].session_mut() else {
            return;
        };
        let mut session = Session::new(change.document, Some(change.path));
        session.id = change.id;
        session.disk_fingerprint = Some(change.fingerprint);
        session.zoom = old.zoom;
        session.pan = old.pan;
        session.fit = old.fit;
        session.backing_scale = old.backing_scale;
        *old = session;
        if index == self.current {
            let toggles = tool_defaults::Toggles::capture(&self.tools);
            self.tools = project_tools::ProjectTools::default();
            toggles.apply(&mut self.tools);
        }
        self.status = "Reloaded changes made outside Compositor.".into();
    }
    pub(super) fn external_project_view(
        &mut self,
        cx: &mut ViewContext<'_, Self>,
    ) -> Option<Element> {
        if self.pending || self.saves.busy() || !self.can_switch_projects() {
            return None;
        }
        let change = self.external_projects.changes.front()?;
        let name = change
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let dialog = quickgui::Dialog::alert("external-project-dialog", true)
            .initial_focus("external-keep")
            .restore_focus_to("workspace");
        let contents = Self::alert_contents(
            dialog,
            format!("{name} changed outside Compositor"),
            "Your open edits are preserved. Open the external version in a separate unsaved tab to compare it. Use Save As to keep your edits without overwriting the external file.",
            cx.size().height,
        );
        let buttons = div()
            .flex_col()
            .gap(8.)
            .child(
                Self::alert_button("Open External Copy").on_click(cx.listener(
                    "external-copy",
                    |this, cx| {
                        if let Some(change) = this.external_projects.changes.pop_front() {
                            let result = this.show_opened_projects(vec![
                                project_open::OpenedProject::Loaded {
                                    document: Box::new(change.document),
                                    path: None,
                                    fingerprint: None,
                                },
                            ]);
                            this.operation_result(alerts::Operation::Open, result, cx);
                        }
                    },
                )),
            )
            .child(Self::alert_button("Keep Editing").on_click(cx.listener(
                "external-keep",
                |this, cx| {
                    this.external_projects.changes.pop_front();
                    cx.invalidate();
                },
            )));
        Some(self.mount_form(
            cx,
            dialog,
            contents.child(buttons),
            400.,
            "External project changes",
            None,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn change(e: &Editor, path: PathBuf, document: Document) -> Change {
        project::save(&document, &path).unwrap();
        Change {
            id: e.tabs[0].id,
            baseline: e.session().disk_fingerprint.clone(),
            fingerprint: project::fingerprint(&path).unwrap().unwrap(),
            path,
            document,
        }
    }
    #[test]
    fn external_reload_updates_clean_document_and_preserves_dirty_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("External.comp");
        project::save(&Document::new(3, 2).unwrap(), &path).unwrap();
        let mut e = Editor::new(vec![path.clone()]).unwrap();
        let mut doc = e.session().document.clone();
        doc.resolution = 144.;
        let update = change(&e, path.clone(), doc.clone());
        e.receive_external_change(update);
        assert_eq!(e.session().document, doc);
        assert!(!e.session().dirty());
        e.session_mut()
            .edit("Local", |doc| {
                doc.resolution = 300.;
                Ok(())
            })
            .unwrap();
        doc.resolution = 96.;
        let update = change(&e, path, doc);
        e.receive_external_change(update);
        assert_eq!(e.session().document.resolution, 300.);
        assert_eq!(e.external_projects.changes.len(), 1);
        assert!(e.session().dirty());
    }
    #[test]
    fn stale_external_load_cannot_replace_a_newly_saved_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Race.comp");
        project::save(&Document::new(3, 2).unwrap(), &path).unwrap();
        let mut e = Editor::new(vec![path.clone()]).unwrap();
        let mut external = e.session().document.clone();
        external.resolution = 144.;
        let update = change(&e, path.clone(), external);
        e.session_mut()
            .edit("Local", |doc| {
                doc.resolution = 300.;
                Ok(())
            })
            .unwrap();
        project::save(&e.session().document, &path).unwrap();
        e.session_mut().disk_fingerprint = project::fingerprint(&path).unwrap();
        e.session_mut().mark_saved(path);
        e.receive_external_change(update);
        assert_eq!(e.session().document.resolution, 300.);
        assert!(e.external_projects.changes.is_empty());
    }

    #[test]
    fn conflict_dialog_opens_an_external_copy_without_discarding_local_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Conflict.comp");
        project::save(&Document::new(3, 2).unwrap(), &path).unwrap();
        let mut e = Editor::new(vec![path.clone()]).unwrap();
        let mut external = e.session().document.clone();
        external.resolution = 144.;
        let update = change(&e, path, external);
        e.session_mut()
            .edit("Local", |doc| {
                doc.resolution = 300.;
                Ok(())
            })
            .unwrap();
        e.receive_external_change(update);
        let (mut cx, view) = quickgui::Application::new()
            .into_test_context(
                quickgui::WindowOptions::new("External conflict").size(1280., 850.),
                e,
            )
            .unwrap();
        cx.click(view.window_handle(), "external-copy").unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 2);
            assert_eq!(e.tabs[0].session().unwrap().document.resolution, 300.);
            assert_eq!(e.tabs[1].session().unwrap().document.resolution, 144.);
            assert!(e.tabs[1].session().unwrap().path.is_none());
            assert!(e.tabs[0].dirty());
            assert!(e.tabs[1].dirty());
        })
        .unwrap();
    }
}
