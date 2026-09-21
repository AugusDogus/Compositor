//! Consequential alerts corresponding to the source app's NSAlert confirmations.
use super::*;
use quickgui::Dialog;

impl Editor {
    fn close_confirmation_title(&self) -> String {
        let filename = self
            .session()
            .path
            .as_deref()
            .and_then(std::path::Path::file_name)
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| "Untitled".into());
        format!("Save changes to {filename}?")
    }

    fn delete_confirmation_title(&self) -> &'static str {
        if self.session().document.selected.len() == 1 {
            "This layer supplies a live mask"
        } else {
            "These layers supply live masks"
        }
    }

    pub(super) fn confirmation_view(
        &mut self,
        cx: &mut ViewContext<'_, Self>,
        form: &Form,
    ) -> Element {
        let closing = matches!(form, Form::Close);
        let dialog = Dialog::alert("editor-dialog", true)
            .initial_focus(if closing {
                "save-close".into()
            } else {
                quickgui::ElementId::from(70_001_u64)
            })
            .restore_focus_to("workspace");
        let title = if closing {
            self.close_confirmation_title()
        } else {
            self.delete_confirmation_title().into()
        };
        let description = if closing {
            "Your changes will be lost if you don’t save them."
        } else {
            "Bake keeps the current masked appearance in the dependent layers’ pixels. Remove Links reveals their pixels. You can undo either choice."
        };
        let contents = Self::alert_contents(dialog, title, description, cx.size().height);
        let cancel = Self::alert_button("Cancel")
            .on_click(cx.listener("form-cancel", |this, cx| this.cancel_form(cx)));
        let buttons = div().w_full().flex_col().gap(8.);
        let buttons = if closing {
            buttons
                .child(
                    Self::alert_button("Save")
                        .bg(Color::rgb8(0, 122, 255))
                        .hover(|s| s.bg(Color::rgb8(24, 137, 255)))
                        .on_click(cx.listener("save-close", |this, cx| {
                            this.modal = None;
                            this.blend_picker.close();
                            this.file_action(Action::Save, cx);
                        })),
                )
                .child(cancel)
                .child(Self::alert_button("Don’t Save").on_click(cx.listener(
                    "discard-close",
                    |this, cx| {
                        if let Some(intent) = this.close_intent.take() {
                            this.finish_close(intent, cx);
                        }
                        this.changed(cx);
                    },
                )))
        } else {
            buttons
                .child(
                    Self::alert_button("Bake and Delete")
                        .bg(Color::rgb8(0, 122, 255))
                        .hover(|s| s.bg(Color::rgb8(24, 137, 255)))
                        .on_click(cx.listener(70_001_u64, |this, cx| {
                            this.action(Action::DeleteLayerBaked, cx);
                        })),
                )
                .child(cancel)
                .child(
                    Self::alert_button("Remove Links and Delete").on_click(cx.listener(
                        70_002_u64,
                        |this, cx| {
                            this.action(Action::DeleteLayerUnlinked, cx);
                        },
                    )),
                )
        };
        self.mount_form(
            cx,
            dialog,
            contents.child(buttons),
            340.,
            "Confirmation",
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn close_alert_names_the_full_project_and_defaults_to_save() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = Editor::with_test_document();
        assert_eq!(
            editor.close_confirmation_title(),
            "Save changes to Untitled?"
        );
        editor.session_mut().path = Some(directory.path().join("Portrait.Study.comp"));
        assert_eq!(
            editor.close_confirmation_title(),
            "Save changes to Portrait.Study.comp?"
        );
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Save confirmation").size(1280., 850.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        cx.update(view, |e, cx| {
            e.request_close(CloseIntent::Tab(e.tabs[e.current].id), cx);
        })
        .unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("save-close".into()));
        let save = cx.element_bounds(window, "save-close").unwrap();
        let cancel = cx.element_bounds(window, "form-cancel").unwrap();
        let discard = cx.element_bounds(window, "discard-close").unwrap();
        assert!(save.y + save.height <= cancel.y);
        assert!(cancel.y + cancel.height <= discard.y);
        cx.simulate_keystrokes(window, "enter").unwrap();
        cx.read(view, |e| {
            // The test backend cannot start the save worker. Return must still
            // attempt Save, keep the project open, and preserve its dirty state.
            assert!(e.status.starts_with("Could not start"), "{}", e.status);
            assert!(e.close_intent.is_none());
            assert!(e.modal.is_none());
            assert_eq!(e.session().document, original);
            assert!(e.session().dirty());
        })
        .unwrap();
    }

    #[test]
    fn close_alert_escape_and_focused_cancel_preserve_the_project() {
        for keys in ["escape", "tab enter"] {
            let editor = Editor::with_test_document();
            let original = editor.session().document.clone();
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Cancel confirmation").size(1280., 850.),
                    editor,
                )
                .unwrap();
            let window = view.window_handle();
            cx.update(view, |e, cx| {
                e.request_close(CloseIntent::Tab(e.tabs[e.current].id), cx);
            })
            .unwrap();
            cx.simulate_keystrokes(window, keys).unwrap();
            cx.read(view, |e| {
                assert!(e.modal.is_none());
                assert!(e.close_intent.is_none());
                assert_eq!(e.session().document, original);
                assert!(e.session().dirty());
            })
            .unwrap();
        }
    }

    #[test]
    fn live_mask_title_counts_requested_layers_not_folder_descendants() {
        let mut editor = Editor::with_test_document();
        compositor::layer_ops::add_blank(&mut editor.session_mut().document).unwrap();
        let ids = editor
            .session()
            .document
            .layers
            .iter()
            .map(|layer| layer.id)
            .collect();
        editor.session_mut().document.selected = ids;
        assert_eq!(
            editor.delete_confirmation_title(),
            "These layers supply live masks"
        );
        compositor::layer_ops::group(&mut editor.session_mut().document).unwrap();
        assert!(compositor::transform::selected_ids(&editor.session().document).len() > 1);
        assert_eq!(
            editor.delete_confirmation_title(),
            "This layer supplies a live mask"
        );
    }
}
