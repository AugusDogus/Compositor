//! Browse this tab's retained committed states; edits keep the ordinary Undo branch rules.
use super::*;

impl Editor {
    pub(super) fn can_browse_history(&self) -> bool {
        self.has_document()
            && matches!(self.modal, None | Some(Form::History))
            && self.can_start_project_operation()
            && self.transform_edit.is_none()
            && self.pending_pixels.is_none()
            && self.pending_gradient.is_none()
            && self.filter_edit.is_none()
            && self.adjustment_edit.is_none()
            && self.jpeg_export.is_none()
            && self.tools.pending_crop.is_none()
            && self.tools.polygon.is_none()
            && !self.session().has_pending_edit()
    }

    fn choose_history(&mut self, session: uuid::Uuid, revision: uuid::Uuid) -> Result<()> {
        if !matches!(self.modal, Some(Form::History))
            || !self.can_browse_history()
            || self.session().id != session
        {
            return Err(compositor::invalid(
                "History navigation is unavailable during another edit. Finish or cancel that edit, then reopen History. The document is unchanged.",
            ));
        }
        let active = self.session().document.active;
        self.session_mut().jump_history(revision)?;
        self.retain_mask_target(active);
        Ok(())
    }

    pub(super) fn history_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let session = self.session().id;
        let mut rows = div()
            .id("history-states")
            .max_h(320.)
            .overflow_y_scroll()
            .flex_col()
            .gap(4.);
        for state in self.session().history() {
            let revision = state.revision;
            let label = format!(
                "{}{}{}",
                state.label,
                if state.current { " · Current" } else { "" },
                if state.saved { " · Saved" } else { "" }
            );
            rows = rows.child(
                self.control(label)
                    .flex_shrink_0()
                    .w_full()
                    .selected(state.current)
                    .bg(if state.current {
                        self.colors.neutral(76)
                    } else {
                        Color::TRANSPARENT
                    })
                    .on_click(
                        cx.listener(format!("history-state-{revision}"), move |this, cx| {
                            let result = this.choose_history(session, revision);
                            this.result(result, cx);
                        }),
                    ),
            );
        }
        div().flex_col().gap(12.)
            .child(text("Choose a state to restore it. A new edit replaces later redo states. History is kept only while this document is open.").wrap())
            .child(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn history_panel_selects_earlier_and_later_states() {
        let mut editor = Editor::with_test_document();
        let first = editor.session().revision();
        editor
            .session_mut()
            .edit("Resolution", |doc| {
                doc.resolution = 144.;
                Ok(())
            })
            .unwrap();
        let last = editor.session().revision();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("History").size(1280., 900.), editor)
            .unwrap();
        cx.update(view, |editor, cx| editor.action(Action::History, cx))
            .unwrap();
        let window = view.window_handle();
        cx.click(window, format!("history-state-{first}")).unwrap();
        cx.read(view, |editor| {
            assert_eq!(editor.session().revision(), first);
            assert!(matches!(editor.modal, Some(Form::History)));
            assert_eq!(editor.session().redo_label(), Some("Resolution"));
        })
        .unwrap();
        cx.click(window, format!("history-state-{last}")).unwrap();
        cx.read(view, |editor| {
            assert_eq!(editor.session().revision(), last);
            assert_eq!(editor.session().document.resolution, 144.);
        })
        .unwrap();
    }

    #[test]
    fn history_rejects_pending_work_and_stale_tab_callbacks() {
        let mut editor = Editor::with_test_document();
        let session = editor.session().id;
        let revision = editor.session().revision();
        editor.session_mut().begin("Preview").unwrap();
        assert!(!editor.action_available(Action::History));
        editor.session_mut().cancel();
        editor.pending = true;
        assert!(!editor.action_available(Action::History));
        editor.pending = false;
        editor.modal = Some(Form::Updates);
        assert!(!editor.action_available(Action::History));
        editor.modal = Some(Form::History);
        assert!(
            editor
                .choose_history(uuid::Uuid::new_v4(), revision)
                .is_err()
        );
        assert!(editor.choose_history(session, revision).is_ok());
    }

    #[test]
    fn long_history_scrolls_without_squeezing_rows() {
        let mut editor = Editor::with_test_document();
        let first = editor.session().revision();
        for resolution in 100..120 {
            editor
                .session_mut()
                .edit("Resolution", |doc| {
                    doc.resolution = f64::from(resolution);
                    Ok(())
                })
                .unwrap();
        }
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Long history").size(1280., 900.), editor)
            .unwrap();
        cx.update(view, |editor, cx| editor.action(Action::History, cx))
            .unwrap();
        let window = view.window_handle();
        let list = cx.element_bounds(window, "history-states").unwrap();
        let row = format!("history-state-{first}");
        let before = cx.element_bounds(window, row.clone()).unwrap();
        assert_eq!(before.height, 24.);
        assert!(list.height <= 320.);
        assert!(
            cx.simulate_retained_scroll(
                window,
                "history-states",
                quickgui::Vector::new(0., -1000.)
            )
            .unwrap()
        );
        assert!(cx.element_bounds(window, row).unwrap().y < before.y);
        cx.click(window, "form-cancel").unwrap();
        cx.read(view, |editor| {
            assert!(editor.modal.is_none());
            assert_eq!(editor.session().document.resolution, 119.);
        })
        .unwrap();
    }
}
