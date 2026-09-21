//! Project sheets coexist with the floating tool panel, as in ProjectController.swift.
use super::*;

impl Editor {
    pub(super) fn panel_applying(&self) -> bool {
        self.filter_applying() || self.adjustment_applying()
    }

    pub(super) fn panel_progress(&self, action: Action) -> Option<Element> {
        let applying = self.panel_applying();
        let busy = match action {
            Action::EditAdjustment => {
                let edit = self.adjustment_edit.as_ref()?;
                match edit.settings.kind {
                    Kind::HueSaturation => return None,
                    Kind::Levels => {
                        return applying.then(|| icons::progress("panel-progress", 14.));
                    }
                    _ => edit.preview_job.as_ref().is_some_and(|job| job.busy()),
                }
            }
            Action::Filter(_) | Action::RemoveBackground => self.filter_busy(),
            _ => return None,
        };
        (applying || busy).then(|| {
            div()
                .flex_row()
                .items_center()
                .gap(8.)
                .child(icons::progress("panel-progress", 14.))
                .child(
                    text(if applying {
                        "Applying…"
                    } else {
                        "Working…"
                    })
                    .text_size(12.)
                    .line_height(15.)
                    .text_color(Color::rgb8(181, 181, 181)),
                )
        })
    }

    pub(super) fn finish_panel_commit(&mut self, session: uuid::Uuid) {
        self.finish_filter_commit(session);
        self.finish_adjustment_commit(session);
    }

    pub(super) fn retained_panel_view(
        &mut self,
        cx: &mut ViewContext<'_, Self>,
    ) -> Option<Element> {
        let form = self.retained_panel.clone()?;
        let mut panel = self.form_view(cx, form);
        panel.disable_subtree();
        Some(panel)
    }

    pub(super) fn retain_tool_panel(&mut self) {
        if self.floating_panel_kind().is_some() {
            self.retained_panel = self.modal.take();
        }
    }

    pub(super) fn tool_form(&self) -> Option<&Form> {
        self.retained_panel.as_ref().or(self.modal.as_ref())
    }

    pub(super) fn tool_form_mut(&mut self) -> Option<&mut Form> {
        self.retained_panel.as_mut().or(self.modal.as_mut())
    }

    pub(super) fn finish_form(&mut self) {
        self.modal = self.retained_panel.take();
        self.dimension_link = None;
    }

    pub(super) fn cancel_form(&mut self, cx: &mut EventContext) {
        if self.panel_applying() {
            return;
        }
        self.size_menus.close(cx);
        self.jpeg_export = None;
        if self.retained_panel.is_none() {
            self.cancel_update();
            self.cancel_adjustment();
            self.cancel_filter();
            self.blend_picker.close();
            self.cancel_close();
        }
        self.finish_form();
        self.changed(cx);
    }

    pub(super) fn size_field_id(&self, index: usize) -> u64 {
        (if self.retained_panel.is_some() {
            60_000
        } else {
            50_000
        }) + index as u64
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use quickgui::{Application, Dialog, WindowOptions};

    #[test]
    fn resizing_beneath_tool_panels_records_separate_history_and_survives_cancel() {
        for filter in [false, true] {
            for action in [Action::CanvasSize, Action::ImageSize] {
                let mut e = Editor::with_test_document();
                let mut doc = Document::new(8, 8).unwrap();
                compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
                e.tabs = vec![Session::new(doc.clone(), None).into()];
                if filter {
                    e.open_filter(compositor::filters::Filter::Gaussian { radius: 1. })
                        .unwrap();
                } else {
                    e.open_pixel_adjustment(Kind::HueSaturation).unwrap();
                    e.update_form_field(0, "90");
                }
                let (mut cx, view) = Application::new()
                    .into_test_context(
                        WindowOptions::new("Resize beneath tool").size(1500., 900.),
                        e,
                    )
                    .unwrap();
                cx.update(view, |e, cx| {
                    e.action(action, cx);
                    e.update_form_field(0, "16");
                    e.submit_form(cx);
                    assert!(e.floating_panel_kind().is_some());
                    assert!(e.retained_panel.is_none());
                    let mut expected = doc.clone();
                    match action {
                        Action::CanvasSize => compositor::canvas_size::resize(
                            &mut expected,
                            [16, 8],
                            [0.5, 0.5],
                            None,
                        )
                        .unwrap(),
                        Action::ImageSize => compositor::image_resize::resize(
                            &mut expected,
                            16,
                            16,
                            doc.resolution,
                            compositor::geometry::Sampling::High,
                        )
                        .unwrap(),
                        _ => unreachable!(),
                    }
                    assert_eq!(e.session().committed_document(), &expected);
                    assert_eq!(
                        e.session().undo_label(),
                        Some(if matches!(action, Action::CanvasSize) {
                            "Canvas Size"
                        } else {
                            "Image Size"
                        })
                    );
                    e.cancel_form(cx);
                    assert_eq!(e.session().document, expected);
                    e.undo_document();
                    assert_eq!(e.session().document, doc);
                })
                .unwrap();
            }
        }
    }

    #[test]
    fn size_shortcut_and_escape_return_to_the_original_panel() {
        let mut e = Editor::with_test_document();
        compositor::edits::fill(
            &mut e.session_mut().document,
            [80, 120, 160, 255],
            false,
            false,
        )
        .unwrap();
        e.open_pixel_adjustment(Kind::HueSaturation).unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Size keyboard routing").size(1500., 900.),
                e,
            )
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, 50_000_u64).unwrap();
        cx.simulate_keystrokes(window, "ctrl-alt-c").unwrap();
        cx.read(view, |e| {
            assert!(matches!(
                e.modal,
                Some(Form::Edit {
                    action: Action::CanvasSize,
                    ..
                })
            ));
            assert!(!e.can_undo());
            assert!(!e.action_available(Action::Save));
        })
        .unwrap();
        cx.focus(window, 60_000_u64).unwrap();
        cx.simulate_keystrokes(window, "escape").unwrap();
        cx.read(view, |e| {
            assert!(e.adjustment_edit.is_some());
            assert!(e.retained_panel.is_none());
            assert!(matches!(
                e.modal,
                Some(Form::Edit {
                    action: Action::EditAdjustment,
                    ..
                })
            ));
        })
        .unwrap();
    }

    #[test]
    fn canvas_sheet_keeps_hue_visible_and_cancellation_preserves_its_fields_and_preview() {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(8, 8).unwrap();
        compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
        e.tabs = vec![Session::new(doc.clone(), None).into()];
        e.open_pixel_adjustment(Kind::HueSaturation).unwrap();
        e.update_form_field(0, "90");
        let preview = e.session().document.clone();
        assert_ne!(preview, doc);
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Project sheet over Hue").size(1500., 900.),
                e,
            )
            .unwrap();
        let window = view.window_handle();
        let panel_before = cx
            .element_bounds(window, Dialog::new("editor-dialog", true).popover_id())
            .unwrap();
        cx.update(view, |e, cx| e.action(Action::CanvasSize, cx))
            .unwrap();
        let panel_after = cx
            .element_bounds(window, Dialog::new("editor-dialog", true).popover_id())
            .unwrap();
        assert_eq!(
            panel_before, panel_after,
            "Opening a project sheet must not move or resize its retained tool panel"
        );
        cx.read(view, |e| {
            assert!(e.adjustment_edit.is_some());
            assert_eq!(e.session().document, preview);
        })
        .unwrap();
        assert!(cx.element_bounds(window, "floating-panel-close").is_ok());
        assert!(
            cx.element_bounds(window, Dialog::new("project-sheet", true).popover_id())
                .is_ok()
        );
        cx.update(view, |e, _| e.update_form_field(0, "16"))
            .unwrap();
        cx.read(view, |e| assert_eq!(e.session().document, preview))
            .unwrap();
        cx.click(window, "form-cancel").unwrap();
        cx.read(view, |e| {
            assert!(matches!(&e.modal, Some(Form::Edit { action: Action::EditAdjustment, fields, .. }) if fields[0].1 == "90"));
            assert_eq!(e.session().document, preview);
            assert_eq!(e.session().committed_document(), &doc);
            assert!(e.session().undo_label().is_none());
        }).unwrap();
    }
}
