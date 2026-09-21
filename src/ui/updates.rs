use super::*;
use compositor::update::{AvailableUpdate, Service, Staged};
use quickgui::UpdateCancellation;

pub(super) struct Updates {
    service: std::result::Result<Service, String>,
    phase: Phase,
    generation: u64,
    automatic: bool,
    preference: Option<PathBuf>,
}

enum Phase {
    Idle,
    Queued(Work),
    Running {
        cancellation: UpdateCancellation,
        activity: Activity,
    },
    Finished(Outcome),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Activity {
    Checking,
    Downloading,
    Installing,
}

enum Outcome {
    Current,
    Available(AvailableUpdate),
    Staged(Staged),
    Installed(String),
}

enum Work {
    Check,
    Download(AvailableUpdate),
    Install(Staged),
}

impl Work {
    fn run(self, service: &Service, cancellation: &UpdateCancellation) -> Result<Outcome> {
        match self {
            Self::Check => service
                .check(cancellation)
                .map(|update| update.map_or(Outcome::Current, Outcome::Available)),
            Self::Download(update) => service.stage(update, cancellation).map(Outcome::Staged),
            Self::Install(staged) => service
                .install(staged, cancellation)
                .map(Outcome::Installed),
        }
    }
}

impl Default for Updates {
    fn default() -> Self {
        let service = Service::configured().map_err(|e| e.to_string());
        Self {
            service,
            phase: Phase::Idle,
            generation: 0,
            automatic: false,
            preference: None,
        }
    }
}

impl Updates {
    fn cancel(&mut self) {
        if let Phase::Running {
            cancellation,
            activity: Activity::Checking | Activity::Downloading,
        } = &self.phase
        {
            cancellation.cancel();
        } else if matches!(self.phase, Phase::Queued(_)) {
            self.phase = Phase::Idle;
        }
    }

    fn advance(&mut self) {
        if self.service.is_err() || matches!(self.phase, Phase::Running { .. } | Phase::Queued(_)) {
            return;
        }
        let next = match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Finished(Outcome::Available(update)) => Work::Download(update),
            Phase::Finished(Outcome::Staged(staged)) => Work::Install(staged),
            _ => Work::Check,
        };
        self.phase = Phase::Queued(next);
    }
}

impl Drop for Updates {
    fn drop(&mut self) {
        if let Phase::Running { cancellation, .. } = &self.phase {
            cancellation.cancel();
        }
    }
}

impl Editor {
    pub fn restore_update_preferences(&mut self) {
        if compositor::update::is_appimage() {
            return;
        }
        self.updates.preference = update_preferences::path();
        if let Some(path) = &self.updates.preference {
            match update_preferences::read(path) {
                Ok(enabled) => {
                    self.updates.automatic = enabled;
                    if enabled && self.updates.service.is_ok() {
                        self.updates.advance();
                    }
                }
                Err(error) => self.updates.phase = Phase::Failed(error.to_string()),
            }
        }
    }

    pub(super) fn open_updates(&mut self, cx: &mut EventContext) {
        if self.pending || self.gesture.is_some() || self.modal.is_some() {
            return;
        }
        self.modal = Some(Form::Updates);
        cx.invalidate();
    }

    pub(super) fn cancel_update(&mut self) {
        self.updates.cancel();
    }

    pub(super) fn start_update_job(&mut self, cx: &ViewContext<'_, Self>) {
        if !matches!(self.updates.phase, Phase::Queued(_)) {
            return;
        }
        let Phase::Queued(work) = std::mem::replace(&mut self.updates.phase, Phase::Idle) else {
            return;
        };
        let Ok(service) = self.updates.service.clone() else {
            return;
        };
        let activity = match &work {
            Work::Check => Activity::Checking,
            Work::Download(_) => Activity::Downloading,
            Work::Install(_) => Activity::Installing,
        };
        let cancellation = UpdateCancellation::new();
        let token = cancellation.clone();
        self.updates.generation = self.updates.generation.wrapping_add(1);
        let generation = self.updates.generation;
        self.updates.phase = Phase::Running {
            cancellation,
            activity,
        };
        // Installation blocks closing until atomic replacement finishes. Checking
        // and downloading leave the editor usable when the dialog is dismissed.
        if activity == Activity::Installing {
            self.pending = true;
            self.status = "Working…".into();
        }
        let result = cx.spawn_background(move || work.run(&service, &token), move |this, result, cx| {
            if this.updates.generation != generation { return; }
            if activity == Activity::Installing { this.pending = false; }
            let cancelled = matches!(&this.updates.phase, Phase::Running { cancellation, .. } if cancellation.is_cancelled());
            this.updates.phase = if cancelled { Phase::Idle } else { match result {
                Ok(Ok(outcome)) => {
                    if let Outcome::Available(update) = &outcome {
                        this.status = format!("Compositor {} is available. Open Updates to review and download it.", update.version);
                    }
                    if let Outcome::Installed(version) = &outcome {
                        this.status = format!("Version {version} installed. Save your projects, then close and reopen Compositor.");
                    }
                    Phase::Finished(outcome)
                }
                Ok(Err(error)) => Phase::Failed(error.to_string()),
                Err(error) => Phase::Failed(format!("The update worker stopped: {error}. Retry the operation; open projects remain intact.")),
            }};
            cx.invalidate();
        });
        if let Err(error) = result {
            if activity == Activity::Installing {
                self.pending = false;
            }
            self.updates.phase = Phase::Failed(format!(
                "Could not start the update worker: {error}. Retry the operation."
            ));
        }
    }

    pub(super) fn update_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let mut panel = div()
            .flex_col()
            .gap(12.)
            .child(text(format!("Compositor {}", env!("CARGO_PKG_VERSION"))));
        if compositor::update::is_appimage() {
            return panel
                .child(text("Download the latest AppImage from GitHub Releases. Save your projects and close Compositor before replacing the old AppImage.").wrap())
                .child(Self::control("Open GitHub Releases").on_click(cx.listener("update-appimage", |this, cx| {
                    let result = cx.open_url(compositor::update::RELEASES_URL)
                        .map_err(|error| compositor::invalid(format!("Could not open GitHub Releases: {error}. Visit {} in your browser.", compositor::update::RELEASES_URL)));
                    this.result(result, cx);
                })));
        }
        if let Err(message) = &self.updates.service {
            return panel.child(text(message.clone()).wrap());
        }
        panel = panel.child(Self::control(if self.updates.automatic {
            "Automatic checks on launch: on"
        } else { "Automatic checks on launch: off" }).on_click(cx.listener("update-automatic", |this, cx| {
            let enabled = !this.updates.automatic;
            let result = this.updates.preference.as_ref()
                .ok_or_else(|| compositor::invalid("No configuration directory is available. Set XDG_CONFIG_HOME to save update preferences."))
                .and_then(|path| update_preferences::save(path, enabled));
            match result {
                Ok(()) => this.updates.automatic = enabled,
                Err(error) => this.status = format!("Could not save update preferences: {error}"),
            }
            cx.invalidate();
        })));
        let (message, action) = match &self.updates.phase {
            Phase::Idle => ("Check for a Linux update.".into(), Some("Check now")),
            Phase::Queued(_) => ("Starting update operation...".into(), None),
            Phase::Running { cancellation, .. } if cancellation.is_cancelled() => {
                ("Cancelling update request...".into(), None)
            }
            Phase::Running {
                activity: Activity::Installing,
                ..
            } => ("Installing update...".into(), None),
            Phase::Running {
                activity: Activity::Checking,
                ..
            } => ("Checking for updates...".into(), None),
            Phase::Running {
                activity: Activity::Downloading,
                ..
            } => ("Downloading update...".into(), None),
            Phase::Failed(error) => (error.clone(), Some("Check again")),
            Phase::Finished(Outcome::Current) => {
                ("This version is up to date.".into(), Some("Check again"))
            }
            Phase::Finished(Outcome::Available(update)) => {
                if let Some(notes) = &update.notes {
                    panel = panel.child(text(notes.clone()).wrap());
                }
                (
                    format!("Version {} is available.", update.version),
                    Some("Download update"),
                )
            }
            Phase::Finished(Outcome::Staged(staged)) => (
                format!(
                    "Version {} is downloaded. Install replaces the application. Your open projects stay in this process until you close it.",
                    staged.version()
                ),
                Some("Install update"),
            ),
            Phase::Finished(Outcome::Installed(version)) => (
                format!(
                    "Version {version} is installed. Save your projects, then close and reopen Compositor to use it."
                ),
                None,
            ),
        };
        panel = panel.child(text(message).wrap());
        if let Some(label) = action {
            panel = panel.child(Self::control(label).on_click(cx.listener(
                "update-next",
                |this, cx| {
                    this.updates.advance();
                    cx.invalidate();
                },
            )));
        }
        panel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_dialog_opens_on_welcome_and_preserves_the_canvas_draft() {
        use quickgui::{Application, WindowOptions};
        let mut editor = Editor::new(Vec::new()).unwrap();
        editor.tabs[0].canvas_draft_mut().unwrap().dimensions = ["321".into(), "123".into()];
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Updates").size(900., 600.), editor)
            .unwrap();
        let window = view.window_handle();
        let last = cx.element_bounds(window, 123_u64).unwrap();
        assert!(
            last.x + last.width <= 900.,
            "Toolbar must fit the minimum window width"
        );
        cx.click(
            window,
            quickgui::Menubar::new("application-menu").item_id(7),
        )
        .unwrap();
        let update = cx
            .read(view, |e| e.menus.command_id("Check for Updates…"))
            .unwrap();
        cx.click(window, update).unwrap();
        assert!(
            cx.read(view, |e| matches!(e.modal, Some(Form::Updates)))
                .unwrap()
        );
        assert!(cx.contains_element(window, "update-next").unwrap());
        cx.click(window, "form-cancel").unwrap();
        cx.read(view, |e| {
            assert!(e.modal.is_none());
            assert!(!e.has_document());
            assert_eq!(e.tabs[0].canvas_draft().unwrap().dimensions, ["321", "123"]);
        })
        .unwrap();
    }

    #[test]
    fn cancelling_a_running_check_retains_its_worker_slot_until_completion() {
        let cancellation = UpdateCancellation::new();
        let mut updates = Updates::default();
        updates.phase = Phase::Running {
            cancellation: cancellation.clone(),
            activity: Activity::Checking,
        };
        updates.cancel();
        assert!(cancellation.is_cancelled());
        assert_eq!(updates.generation, 0);
        updates.advance();
        assert!(matches!(updates.phase, Phase::Running { .. }));
    }

    #[test]
    fn dismissing_install_keeps_its_completion_and_close_guard() {
        let cancellation = UpdateCancellation::new();
        let mut updates = Updates::default();
        updates.phase = Phase::Running {
            cancellation: cancellation.clone(),
            activity: Activity::Installing,
        };
        updates.cancel();
        assert!(!cancellation.is_cancelled());
        assert_eq!(updates.generation, 0);
        assert!(matches!(
            updates.phase,
            Phase::Running {
                activity: Activity::Installing,
                ..
            }
        ));
    }
}
