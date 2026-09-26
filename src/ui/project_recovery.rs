//! Periodic recovery saves never overwrite the user's chosen project package.
use super::recovery_store::Store;
use super::*;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub(super) struct Recovery {
    store: Option<Store>,
    saved: HashMap<Uuid, Uuid>,
    next_save: Instant,
    running: bool,
    pub close: Option<CloseIntent>,
}
impl Default for Recovery {
    fn default() -> Self {
        Self {
            store: None,
            saved: HashMap::new(),
            next_save: Instant::now() + Duration::from_secs(30),
            running: false,
            close: None,
        }
    }
}
impl Recovery {
    pub fn initialize(tabs: &mut Vec<ProjectTab>) -> (Self, String) {
        if cfg!(test) {
            return (Self::default(), String::new());
        }
        let startup = super::recovery_store::state_root().and_then(|path| Store::initialize(&path));
        match startup {
            Ok(startup) => {
                let count = startup.recovered.len();
                let mut recovery = Self {
                    store: Some(startup.store),
                    ..Self::default()
                };
                for recovered in startup.recovered {
                    let mut session = Session::new(recovered.document, None);
                    session.id = recovered.id;
                    recovery.saved.insert(session.id, session.revision());
                    tabs.push(ProjectTab::recovered(
                        session,
                        format!("Recovered {}", recovered.name),
                    ));
                }
                let mut status = if count == 0 {
                    String::new()
                } else {
                    format!(
                        "Recovered {count} unsaved project(s). Original files are unchanged. Use Save As to keep the recovered work."
                    )
                };
                if !startup.warnings.is_empty() {
                    status.push_str(&startup.warnings.join(" "));
                }
                (recovery, status)
            }
            Err(error) => (
                Self::default(),
                format!(
                    "Crash recovery is unavailable: {error}. Manual project saving still works."
                ),
            ),
        }
    }
    pub fn busy(&self) -> bool {
        self.running
    }
}
impl Editor {
    pub(super) fn autosave_projects(&mut self, cx: &mut ViewContext<'_, Self>) {
        let Some(store) = self.recovery.store.clone() else {
            return;
        };
        if self.recovery.running {
            return;
        }
        if Instant::now() < self.recovery.next_save {
            cx.request_repaint_at(self.recovery.next_save);
            return;
        }
        self.recovery.next_save = Instant::now() + Duration::from_secs(30);
        cx.request_repaint_at(self.recovery.next_save);
        let mut snapshots = Vec::new();
        let mut clean = Vec::new();
        for tab in &self.tabs {
            let Some(session) = tab.session() else {
                continue;
            };
            if !session.dirty() {
                clean.push(tab.id);
            } else if self.recovery.saved.get(&tab.id) != Some(&session.revision()) {
                snapshots.push((
                    tab.id,
                    session.revision(),
                    tab.title(),
                    session.committed_document().clone(),
                ));
            }
        }
        if snapshots.is_empty() && clean.iter().all(|id| !self.recovery.saved.contains_key(id)) {
            return;
        }
        self.recovery.running = true;
        let result = cx.spawn_background(move || {
            let mut results = Vec::new();
            for (id, revision, name, document) in snapshots {
                results.push((id, Some(revision), store.write(id, &name, &document)));
            }
            for id in clean { results.push((id, None, store.remove(id))); }
            results
        }, |this, result, cx| {
            this.recovery.running = false;
            match result {
                Ok(results) => for (id, revision, result) in results {
                    match result {
                        Ok(()) => match revision {
                            Some(revision) => { this.recovery.saved.insert(id, revision); },
                            None => { this.recovery.saved.remove(&id); },
                        },
                        Err(error) => this.status = format!("Could not update a recovery copy: {error}. Your open edits and original project are preserved. Save manually and check available disk space."),
                    }
                },
                Err(error) => this.status = format!("Recovery worker failed: {error}. Your open edits are preserved. Save manually."),
            }
            if let Some(intent) = this.recovery.close.take() { this.request_close(intent, cx); }
            cx.invalidate();
        });
        if let Err(error) = result {
            self.recovery.running = false;
            self.status = format!(
                "Could not start recovery save: {error}. Your open edits are preserved. Save manually."
            );
        }
    }
    pub(super) fn remove_recovery(&mut self, id: Uuid) {
        if let Some(store) = &self.recovery.store {
            if let Err(error) = store.remove(id) {
                self.status = format!(
                    "The project closed, but its recovery copy could not be removed: {error}"
                );
            } else {
                self.recovery.saved.remove(&id);
            }
        }
    }
    pub(super) fn finish_recovery(&mut self) -> Result<()> {
        if let Some(store) = &self.recovery.store {
            store.clear()?;
        }
        self.recovery.store = None;
        Ok(())
    }
}
