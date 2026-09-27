//! Recover a native rendered view only after an explicit user choice.
use super::*;

pub(super) struct Request {
    pub path: PathBuf,
    pub reason: String,
    pub fingerprint: project::Fingerprint,
}

impl Editor {
    fn cancel_authoring_copy(&mut self, cx: &mut EventContext) {
        self.authoring_copies.pop_front();
        self.status =
            "Opening the rendered copy was cancelled. Your files and open edits are unchanged."
                .into();
        cx.invalidate();
    }
    fn confirm_authoring_copy(&mut self, cx: &mut EventContext) {
        if let Some(request) = self.authoring_copies.pop_front() {
            self.queue_file(file_jobs::FileJob::OpenRenderedCopy(request));
            cx.invalidate();
        }
    }
    pub(super) fn authoring_copy_view(
        &mut self,
        cx: &mut ViewContext<'_, Self>,
    ) -> Option<Element> {
        if self.pending || self.psd_conversion.is_some() {
            return None;
        }
        let request = self.authoring_copies.front()?;
        let dialog = quickgui::Dialog::alert("authoring-copy", true)
            .initial_focus("authoring-cancel")
            .restore_focus_to("workspace");
        let contents = self.alert_contents(
            dialog,
            "Linux editing sources are unavailable",
            format!("{}\n\n{}\n\nOpen the rendered view as a separate unsaved document? Its Linux editing settings will not be editable. The original project and your open edits will be preserved.", request.path.display(), request.reason),
            cx.size().height,
        ).child(self.alert_button("Open Rendered Copy").on_click(cx.listener("authoring-open", |this, cx| {
            this.confirm_authoring_copy(cx);
        }))).child(self.alert_button("Cancel").on_click(cx.listener("authoring-cancel", |this, cx| this.cancel_authoring_copy(cx))));
        let dismiss = cx.dismiss_listener(dialog.popover_id(), |this, cx| {
            this.cancel_authoring_copy(cx)
        });
        Some(
            dialog
                .root()
                .flex_row()
                .items_center()
                .justify_center()
                .child(dialog.backdrop().bg(Color::TRANSPARENT))
                .child(dialog.popup_with(contents).on_dismiss(dismiss).on_key_down(
                    cx.key_down_listener(dialog.popover_id(), |_, _, cx| cx.stop_propagation()),
                )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::{
        adjustment::{ExtendedAdjustment, PhotoFilter},
        document::{Layer, LayerContent},
    };
    use quickgui::{Application, WindowOptions};

    fn damaged_project(path: &std::path::Path) {
        let mut document = Document::new(3, 2).unwrap();
        let mut layer = Layer::blank("Photo Filter", 3, 2);
        layer.content = LayerContent::ExtendedAdjustment(Box::new(
            ExtendedAdjustment::PhotoFilter(PhotoFilter::default()),
        ));
        document.add(layer).unwrap();
        project::save(&document, path).unwrap();
        std::fs::write(path.join("linux-editing.json"), b"{}").unwrap();
    }
    #[test]
    fn rendered_copy_requires_confirmation_and_has_no_save_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Damaged.comp");
        damaged_project(&path);
        let original = std::fs::read(path.join("linux-editing.json")).unwrap();
        let mut editor = Editor::with_test_document();
        let before = editor.session().document.clone();
        editor
            .show_opened_projects(vec![
                project_open::OpenedProject::load(path.clone(), &[]).unwrap(),
            ])
            .unwrap();
        assert_eq!(editor.tabs.len(), 1);
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Rendered copy").size(1280., 850.),
                editor,
            )
            .unwrap();
        cx.read(view, |editor| {
            assert!(!editor.action_available(Action::Save));
            assert!(!editor.can_switch_projects());
            assert!(!editor.can_edit_layers());
        })
        .unwrap();
        cx.simulate_keystrokes(view.window_handle(), "ctrl-z ctrl-n")
            .unwrap();
        let completed = cx
            .update(view, |editor, cx| {
                editor.confirm_authoring_copy(cx);
                editor.file_job.take().unwrap().run(&[]).unwrap()
            })
            .unwrap();
        cx.update(view, |editor, cx| {
            editor.pending = false;
            editor.finish_approved_file_job(completed, cx).unwrap();
        })
        .unwrap();
        cx.read(view, |editor| {
            assert_eq!(editor.tabs.len(), 2);
            assert_eq!(editor.tabs[0].session().unwrap().document, before);
            assert!(editor.session().dirty());
            assert!(editor.session().path.is_none());
            assert!(editor.session().disk_fingerprint.is_none());
        })
        .unwrap();
        assert_eq!(
            std::fs::read(path.join("linux-editing.json")).unwrap(),
            original
        );
    }
    #[test]
    fn escape_preserves_tabs_and_startup_queues_the_same_warning() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Damaged.comp");
        damaged_project(&path);
        let startup = Editor::new(vec![path.clone()]).unwrap();
        assert!(!startup.has_document());
        assert_eq!(startup.launch_queue.pop().unwrap(), vec![path.clone()]);
        let mut editor = Editor::with_test_document();
        let before = editor.session().document.clone();
        editor
            .show_opened_projects(vec![project_open::OpenedProject::load(path, &[]).unwrap()])
            .unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Cancel rendered copy").size(1280., 850.),
                editor,
            )
            .unwrap();
        cx.simulate_keystrokes(view.window_handle(), "escape")
            .unwrap();
        cx.read(view, |editor| {
            assert!(editor.authoring_copies.is_empty());
            assert_eq!(editor.tabs.len(), 1);
            assert_eq!(editor.session().document, before);
        })
        .unwrap();
    }
}
