use super::*;
use std::collections::VecDeque;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Quit;

pub(super) enum CloseIntent {
    Tab(Uuid),
    Window,
}

pub(super) enum CloseProgress {
    Tab { target: Uuid, previous: Uuid },
    Window(VecDeque<Uuid>),
}

impl Editor {
    pub(super) fn request_close(&mut self, intent: CloseIntent, cx: &mut EventContext) {
        if self.develop.is_some() {
            self.show_error(alerts::Operation::RawDevelop, "Choose Develop to keep the RAW edit, or Cancel to discard it, before closing. Your existing project and camera file are unchanged.");
            cx.invalidate();
            return;
        }
        if !self.can_switch_projects() {
            return;
        }
        if let Err(error) = self.finish_pending_edits() {
            self.result(Err(error), cx);
            return;
        }
        self.cancel_adjustment();
        self.cancel_filter();
        self.jpeg_export = None;
        self.gesture = None;
        if let Some(session) = self.tabs[self.current].session_mut() {
            session.cancel();
        }
        match intent {
            CloseIntent::Tab(target) => {
                let Some(index) = self.tabs.iter().position(|tab| tab.id == target) else {
                    return;
                };
                let progress = CloseProgress::Tab {
                    target,
                    previous: self.tabs[self.current].id,
                };
                if self.tabs[index].needs_save() {
                    self.activate_tab(index);
                    self.close_intent = Some(progress);
                    self.modal = Some(Form::confirm_close());
                } else {
                    self.finish_close(progress, cx);
                }
            }
            CloseIntent::Window => {
                let current = self.tabs[self.current].id;
                let order = std::iter::once(current)
                    .chain(self.tabs.iter().filter(|s| s.id != current).map(|s| s.id))
                    .collect();
                self.confirm_next_close(order, cx);
            }
        }
        self.changed(cx);
    }

    pub(super) fn cancel_close(&mut self) {
        if let Some(CloseProgress::Tab { previous, .. }) = self.close_intent.take()
            && let Some(index) = self.tabs.iter().position(|tab| tab.id == previous)
        {
            self.activate_tab(index);
        }
    }

    fn confirm_next_close(&mut self, mut remaining: VecDeque<Uuid>, cx: &mut EventContext) {
        while let Some(id) = remaining.pop_front() {
            if let Some(index) = self.tabs.iter().position(|s| s.id == id && s.needs_save()) {
                self.activate_tab(index);
                self.close_intent = Some(CloseProgress::Window(remaining));
                self.modal = Some(Form::confirm_close());
                return;
            }
        }
        cx.exit();
    }

    pub(super) fn finish_close(&mut self, progress: CloseProgress, cx: &mut EventContext) {
        self.modal = None;
        self.close_intent = None;
        match progress {
            CloseProgress::Tab { target, previous } => {
                if let Some(index) = self.tabs.iter().position(|s| s.id == target) {
                    if self.tabs.len() == 1 {
                        // Activate the replacement before removing the last tab,
                        // so its parked controls never become another tab's state.
                        self.add_empty_tab();
                    } else {
                        let destination = self
                            .tabs
                            .iter()
                            .position(|tab| tab.id == previous && tab.id != target)
                            .unwrap_or_else(|| {
                                if index + 1 < self.tabs.len() {
                                    index + 1
                                } else {
                                    index - 1
                                }
                            });
                        self.activate_tab(destination);
                    }
                    self.tabs.remove(index);
                    if self.current > index {
                        self.current -= 1;
                    }
                }
            }
            // Keep every tab until all decisions are complete. Cancelling a later
            // prompt must retain earlier tabs and their unsaved edits.
            CloseProgress::Window(remaining) => self.confirm_next_close(remaining, cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn quit_shortcut_prompts_for_every_dirty_project_and_can_be_cancelled() {
        let mut editor = Editor::with_test_document();
        editor
            .tabs
            .push(Session::new(Document::new(4, 4).unwrap(), None).into());
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Quit").size(1280., 850.), editor)
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, "workspace").unwrap();
        let quit = quickgui::Keystroke::new(Key::Character("q".into()), Modifiers::CONTROL);
        cx.simulate_keystroke(window, quit.clone()).unwrap();
        assert!(
            cx.read(view, |e| matches!(
                e.close_intent,
                Some(CloseProgress::Window(_))
            ))
            .unwrap()
        );
        cx.click(window, "form-cancel").unwrap();
        assert!(cx.is_window_open(window));
        assert_eq!(cx.read(view, |e| e.tabs.len()).unwrap(), 2);
        cx.simulate_keystroke(window, quit).unwrap();
        cx.click(window, "discard-close").unwrap();
        assert!(cx.is_window_open(window));
        cx.click(window, "discard-close").unwrap();
        assert!(!cx.is_window_open(window));
    }

    #[test]
    fn quit_shortcut_closes_an_empty_workspace_with_a_dimension_field_focused() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Quit welcome").size(1280., 850.),
                Editor::new(Vec::new()).unwrap(),
            )
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, "welcome-dimension-0").unwrap();
        cx.simulate_keystroke(
            window,
            quickgui::Keystroke::new(Key::Character("q".into()), Modifiers::CONTROL),
        )
        .unwrap();
        assert!(!cx.is_window_open(window));
    }

    #[test]
    fn closing_another_tab_restores_the_selected_project_on_cancel_discard_and_save_failure() {
        for decision in ["form-cancel", "discard-close", "save-close"] {
            let directory = tempfile::tempdir().unwrap();
            let mut e = Editor::with_test_document();
            e.tabs = (0..3)
                .map(|_| Session::new(Document::new(4, 4).unwrap(), None).into())
                .collect();
            let target = e.tabs[0].id;
            let selected = e.tabs[2].id;
            if decision == "save-close" {
                e.tabs[0].session_mut().unwrap().path = Some(directory.path().join("Unsaved.comp"));
            }
            e.activate_tab(2);
            compositor::edits::add_mask(&mut e.session_mut().document, false).unwrap();
            e.tools.mask_target = true;
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Close background tab").size(1280., 850.),
                    e,
                )
                .unwrap();
            let window = view.window_handle();
            cx.click(window, format!("close-tab-{target}")).unwrap();
            cx.read(view, |e| {
                assert_eq!(e.tabs[e.current].id, target);
                assert!(matches!(e.modal, Some(Form::Close)));
            })
            .unwrap();
            cx.click(window, decision).unwrap();
            cx.read(view, |e| {
                assert_eq!(e.tabs[e.current].id, selected);
                assert!(e.tools.mask_target);
                assert!(e.close_intent.is_none());
                assert_eq!(
                    e.tabs.len(),
                    if decision == "discard-close" { 2 } else { 3 }
                );
                assert_eq!(
                    e.tabs.iter().any(|tab| tab.id == target),
                    decision != "discard-close"
                );
            })
            .unwrap();
        }
    }

    #[test]
    fn closing_a_clean_background_tab_keeps_the_selected_tab() {
        let mut e = Editor::new(Vec::new()).unwrap();
        let target = e.tabs[0].id;
        e.add_empty_tab();
        let selected = e.tabs[1].id;
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Close empty tab").size(1280., 850.), e)
            .unwrap();
        cx.click(view.window_handle(), format!("close-tab-{target}"))
            .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 1);
            assert_eq!(e.tabs[e.current].id, selected);
            assert!(e.modal.is_none());
        })
        .unwrap();
    }

    #[test]
    fn cancelling_window_close_retains_all_tabs_and_starts_with_the_current_project() {
        let mut e = Editor::with_test_document();
        e.tabs = (0..3)
            .map(|_| Session::new(Document::new(4, 4).unwrap(), None).into())
            .collect();
        e.activate_tab(2);
        let documents: Vec<_> = e
            .tabs
            .iter()
            .map(|s| s.session().unwrap().document.clone())
            .collect();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Close projects").size(1280., 850.), e)
            .unwrap();
        let window = view.window_handle();
        assert!(!cx.simulate_close_requested(window).unwrap());
        assert_eq!(cx.read(view, |e| e.current).unwrap(), 2);
        cx.click(window, "discard-close").unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 3);
            assert_eq!(e.current, 0);
        })
        .unwrap();
        cx.click(window, "form-cancel").unwrap();
        cx.read(view, |e| {
            assert!(e.close_intent.is_none());
            assert!(e.modal.is_none());
            assert_eq!(
                e.tabs
                    .iter()
                    .map(|s| s.session().unwrap().document.clone())
                    .collect::<Vec<_>>(),
                documents
            );
        })
        .unwrap();
        assert!(cx.is_window_open(window));
        assert!(!cx.simulate_close_requested(window).unwrap());
        for _ in 0..3 {
            cx.click(window, "discard-close").unwrap();
        }
        assert!(!cx.is_window_open(window));
    }
}
