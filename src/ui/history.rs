//! Document history follows EditorSession's pending-edit rules.
use super::*;

impl Editor {
    pub(super) fn history_preserves_preview(&self) -> bool {
        self.filter_edit.is_some()
            || (self
                .adjustment_edit
                .as_ref()
                .is_some_and(|edit| edit.settings.kind != Kind::Levels)
                && !self.editing_adjustment_layer())
    }

    pub(super) fn can_use_history(&self) -> bool {
        !self.pending
            && self.retained_panel.is_none()
            && self.gesture.is_none()
            && self.rename.is_none()
            && self.transform_edit.is_none()
            && self.pending_pixels.is_none()
            && (matches!(self.modal, None | Some(Form::Blend)) || self.history_preserves_preview())
            && (self.tabs[self.current]
                .session()
                .is_none_or(|session| !session.has_pending_edit())
                || self.pending_gradient.is_some()
                || self.history_preserves_preview())
    }

    pub(super) fn can_undo(&self) -> bool {
        self.can_use_history()
            && (self.pending_gradient.is_some() || self.tabs[self.current].undo_label().is_some())
    }

    pub(super) fn can_redo(&self) -> bool {
        self.can_use_history() && self.tabs[self.current].redo_label().is_some()
    }

    pub(super) fn undo_document(&mut self) {
        if self.pending_gradient.take().is_some() {
            self.session_mut().cancel();
        } else {
            let active = self.current_document().and_then(|doc| doc.active);
            if self.tabs[self.current].at_creation() || !self.history_preserves_preview() {
                self.tabs[self.current].undo();
            } else {
                self.session_mut().undo_committed();
            }
            self.refresh_history_document(active);
        }
    }

    pub(super) fn redo_document(&mut self) {
        if self.pending_gradient.take().is_some() {
            self.session_mut().cancel();
        }
        let active = self.current_document().and_then(|doc| doc.active);
        if !self.has_document() || !self.history_preserves_preview() {
            self.tabs[self.current].redo();
        } else {
            self.session_mut().redo_committed();
        }
        self.refresh_history_document(active);
    }

    fn refresh_history_document(&mut self, active: Option<uuid::Uuid>) {
        self.refresh_adjustment_document();
        self.refresh_filter_document();
        self.tools.pending_crop = None;
        if self.has_document() {
            self.retain_mask_target(active);
        } else {
            self.tools.mask_target = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    fn editor() -> Editor {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(8, 8).unwrap();
        compositor::edits::fill(&mut doc, [50, 120, 200, 255], false, false).unwrap();
        editor.tabs = vec![Session::new(doc, None).into()];
        editor
    }

    #[test]
    fn undo_discards_a_gradient_without_creating_redo_or_losing_existing_redo() {
        for existing_redo in [false, true] {
            let mut editor = editor();
            let mut redone = None;
            if existing_redo {
                editor
                    .session_mut()
                    .edit("Fill", |doc| {
                        compositor::edits::fill(doc, [200, 60, 80, 255], false, false)
                    })
                    .unwrap();
                redone = Some(editor.session().document.clone());
                editor.session_mut().undo();
            }
            let original = editor.session().document.clone();
            editor.begin_gradient([0., 0.]).unwrap();
            editor
                .move_gradient([7., 7.], gradient::Endpoint::End, false)
                .unwrap();
            assert!(editor.can_undo());
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Gradient history").size(1500., 900.),
                    editor,
                )
                .unwrap();
            cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
            cx.read(view, |e| {
                assert_eq!(e.session().document, original);
                assert!(e.pending_gradient.is_none());
                assert!(e.session().undo_label().is_none());
                assert_eq!(e.session().redo_label(), existing_redo.then_some("Fill"));
            })
            .unwrap();
            cx.update(view, |e, cx| e.action(Action::Redo, cx)).unwrap();
            cx.read(view, |e| {
                assert_eq!(e.session().document, redone.unwrap_or(original))
            })
            .unwrap();
        }
    }

    #[test]
    fn redo_discards_the_pending_gradient_before_restoring_history() {
        let mut editor = editor();
        editor
            .session_mut()
            .edit("Fill", |doc| {
                compositor::edits::fill(doc, [200, 60, 80, 255], false, false)
            })
            .unwrap();
        let redone = editor.session().document.clone();
        editor.session_mut().undo();
        editor.begin_gradient([0., 0.]).unwrap();
        editor
            .move_gradient([7., 7.], gradient::Endpoint::End, false)
            .unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Redo gradient").size(1500., 900.),
                editor,
            )
            .unwrap();
        cx.update(view, |e, cx| e.action(Action::Redo, cx)).unwrap();
        cx.read(view, |e| {
            assert_eq!(e.session().document, redone);
            assert!(e.pending_gradient.is_none());
            assert!(!e.session().has_pending_edit());
            assert_eq!(e.session().undo_label(), Some("Fill"));
            assert!(e.session().redo_label().is_none());
        })
        .unwrap();
    }

    #[test]
    fn undo_keeps_an_existing_mask_target_and_clears_a_removed_one() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Mask history").size(1500., 900.),
                editor(),
            )
            .unwrap();
        cx.update(view, |e, cx| {
            e.action(Action::AddMask, cx);
            let original_mask = e.session().document.active_layer().unwrap().mask.clone();
            e.action(Action::Fill, cx);
            e.action(Action::Undo, cx);
            assert!(e.tools.mask_target);
            assert_eq!(
                e.session().document.active_layer().unwrap().mask,
                original_mask
            );
            e.action(Action::Undo, cx);
            assert!(!e.tools.mask_target);
            assert!(e.session().document.active_layer().unwrap().mask.is_none());
        })
        .unwrap();
    }

    #[test]
    fn persistent_layer_and_pixel_transforms_keep_history_unavailable() {
        for floating in [false, true] {
            let mut editor = editor();
            editor
                .session_mut()
                .edit("Fill", |doc| {
                    compositor::edits::fill(doc, [200, 60, 80, 255], false, false)
                })
                .unwrap();
            if floating {
                editor.session_mut().document.selection = Some(
                    compositor::selection::Selection::rectangle(8, 8, [1., 1.], [5., 5.], false),
                );
                editor.begin_pixel_transform().unwrap();
            } else {
                editor.start_toolbar_transform().unwrap();
            }
            let before = editor.session().document.clone();
            assert!(!editor.can_undo());
            assert!(!editor.can_redo());
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Transform history").size(1500., 900.),
                    editor,
                )
                .unwrap();
            cx.update(view, |e, cx| {
                e.action(Action::Undo, cx);
                e.action(Action::Redo, cx);
            })
            .unwrap();
            cx.read(view, |e| {
                assert_eq!(e.session().document, before);
                assert!(e.transform_edit.is_some() || e.pending_pixels.is_some());
                assert_eq!(e.session().undo_label(), Some("Fill"));
            })
            .unwrap();
        }
    }
    #[test]
    fn hue_history_preserves_panel_and_cached_pixels_across_removed_target() {
        let mut e = editor();
        let original = e.session().document.clone();
        e.session_mut()
            .edit("New Layer", |doc| {
                compositor::layer_ops::add_blank(doc)?;
                compositor::edits::fill(doc, [180, 60, 20, 255], false, false)
            })
            .unwrap();
        let added = e.session().document.clone();
        e.open_pixel_adjustment(Kind::HueSaturation).unwrap();
        if let Some(Form::Edit { fields, .. }) = &mut e.modal {
            fields[0].1 = "90".into();
        }
        e.preview_adjustment().unwrap();
        let preview = e.session().document.clone();
        assert_ne!(preview, added);
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Hue history").size(1500., 900.), e)
            .unwrap();
        cx.update(view, |e, cx| {
            assert!(e.can_undo());
            e.action(Action::Undo, cx);
            assert_eq!(e.session().document, original);
            assert!(e.adjustment_edit.is_some());
            assert!(e.modal.is_some());
            e.action(Action::Redo, cx);
            assert_eq!(e.session().document, preview);
        })
        .unwrap();
        cx.click(view.window_handle(), "adjustment-preview")
            .unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            added
        );
        cx.click(view.window_handle(), "adjustment-preview")
            .unwrap();
        cx.update(view, |e, _| {
            assert_eq!(e.session().document, preview);
            e.finish_adjustment().unwrap();
            e.session_mut().undo();
            assert_eq!(e.session().document, added);
            e.session_mut().undo();
            assert_eq!(e.session().document, original);
        })
        .unwrap();
    }

    #[test]
    fn levels_and_adjustment_layer_transactions_still_block_history() {
        for layer in [false, true] {
            let mut e = editor();
            e.session_mut()
                .edit("Fill", |doc| {
                    compositor::edits::fill(doc, [200, 20, 30, 255], false, false)
                })
                .unwrap();
            if layer {
                e.open_adjustment(Some(Kind::HueSaturation)).unwrap();
            } else {
                e.open_pixel_adjustment(Kind::Levels).unwrap();
            }
            assert!(!e.can_undo());
            assert!(!e.can_redo());
        }
    }

    #[test]
    fn panel_history_shortcuts_preserve_hue_and_filter_panels() {
        use quickgui::Keystroke;
        for filter in [false, true] {
            let mut e = editor();
            let original = e.session().document.clone();
            e.session_mut()
                .edit("Fill", |doc| {
                    compositor::edits::fill(doc, [200, 20, 30, 255], false, false)
                })
                .unwrap();
            let filled = e.session().document.clone();
            if filter {
                e.open_filter(compositor::filters::Filter::Gaussian { radius: 1. })
                    .unwrap();
            } else {
                e.open_pixel_adjustment(Kind::HueSaturation).unwrap();
            }
            let (mut cx, view) = Application::new()
                .into_test_context(WindowOptions::new("Panel shortcuts").size(1500., 900.), e)
                .unwrap();
            let window = view.window_handle();
            // Focus a control without text-editing history.
            cx.focus(window, "form-apply").unwrap();
            cx.simulate_keystroke(
                window,
                Keystroke::new(Key::Character("z".into()), Modifiers::CONTROL),
            )
            .unwrap();
            cx.read(view, |e| {
                assert_eq!(e.session().committed_document(), &original);
                assert!(e.modal.is_some());
            })
            .unwrap();
            cx.simulate_keystroke(
                window,
                Keystroke::new(
                    Key::Character("z".into()),
                    Modifiers::CONTROL | Modifiers::SHIFT,
                ),
            )
            .unwrap();
            cx.read(view, |e| {
                assert_eq!(e.session().committed_document(), &filled);
                assert!(e.modal.is_some());
            })
            .unwrap();
        }
    }
}
