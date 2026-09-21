//! PSD conversion is a cancellable staging step, never an implicit file save.
use super::*;
use super::{file_jobs::Completed, project_open::OpenedProject};
use compositor::{
    document::{Layer, LayerContent},
    invalid,
    psd::{ConversionReport, Imported},
};
use quickgui::Dialog;
use uuid::Uuid;

pub(super) enum Conversion {
    Files {
        completed: Box<Completed>,
        report: ConversionReport,
    },
    Export {
        document: Document,
        report: ConversionReport,
    },
}
impl Completed {
    pub(super) fn psd_report(&self) -> Option<ConversionReport> {
        let (projects, imports) = match self {
            Self::Opened { projects, .. } => (projects.as_slice(), &[][..]),
            Self::Imported { projects, psds, .. } => (projects.as_slice(), psds.as_slice()),
            _ => return None,
        };
        let reports = projects
            .iter()
            .filter_map(|p| match p {
                OpenedProject::Psd(i) => Some(&i.report),
                _ => None,
            })
            .chain(imports.iter().map(|(_, i)| &i.report))
            .collect::<Vec<_>>();
        if reports.is_empty() {
            return None;
        }
        Some(ConversionReport {
            changes: reports
                .into_iter()
                .flat_map(|r| r.changes.clone())
                .collect(),
        })
    }
}
impl Editor {
    pub(super) fn prepare_psd_export(&mut self) {
        let document = self.session().committed_document().clone();
        let report = compositor::psd::export_report(&document);
        self.psd_conversion = Some(Conversion::Export { document, report });
    }
    fn cancel_psd_conversion(&mut self, cx: &mut EventContext) {
        self.psd_conversion = None;
        self.status = "PSD conversion cancelled. Your files and open edits are unchanged.".into();
        cx.invalidate();
    }
    fn confirm_psd_conversion(&mut self, cx: &mut EventContext) {
        let Some(conversion) = self.psd_conversion.take() else {
            return;
        };
        match conversion {
            Conversion::Files { completed, .. } => {
                let result = self.finish_approved_file_job(*completed, cx);
                self.operation_result(alerts::Operation::Import, result, cx);
            }
            Conversion::Export { document, .. } => self.prompt_psd_path(document, cx),
        }
    }
    pub(super) fn psd_conversion_view(
        &mut self,
        cx: &mut ViewContext<'_, Self>,
    ) -> Option<Element> {
        let conversion = self.psd_conversion.as_ref()?;
        let (title, report) = match conversion {
            Conversion::Files { report, .. } => ("Import Photoshop document", report),
            Conversion::Export { report, .. } => ("Export Photoshop document", report),
        };
        let dialog = Dialog::alert("psd-conversion", true).initial_focus("psd-cancel");
        let contents =
            Self::alert_contents(dialog, title, report.description(), cx.size().height)
                .child(Self::alert_button("Continue").on_click(
                    cx.listener("psd-continue", |this, cx| this.confirm_psd_conversion(cx)),
                ))
                .child(Self::alert_button("Cancel").on_click(
                    cx.listener("psd-cancel", |this, cx| this.cancel_psd_conversion(cx)),
                ));
        let dismiss = cx.dismiss_listener(dialog.popover_id(), |this, cx| {
            this.cancel_psd_conversion(cx)
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
    pub(super) fn apply_psd_imports(
        &mut self,
        session: Uuid,
        imports: Vec<(String, Imported)>,
        center: Option<[f64; 2]>,
    ) -> Result<()> {
        if imports.is_empty() {
            return Ok(());
        }
        let destination = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == session)
            .ok_or_else(|| invalid("The PSD destination tab closed. Import the file again."))?;
        for (name, imported) in imports {
            if destination.session().is_none() {
                destination.edit_or_create("Import PSD", || Ok(imported.document), |_| Ok(()))?;
                continue;
            }
            let session = destination
                .session_mut()
                .ok_or_else(|| invalid("The PSD destination has no document."))?;
            session.edit_committed("Import PSD", move |doc| {
                let center =
                    center.unwrap_or([f64::from(doc.width) / 2., f64::from(doc.height) / 2.]);
                let delta = [
                    center[0] - f64::from(imported.document.width) / 2.,
                    center[1] - f64::from(imported.document.height) / 2.,
                ];
                let mut folder =
                    Layer::blank(name, imported.document.width, imported.document.height);
                folder.content = LayerContent::Group;
                folder.transform.origin = delta;
                let id = folder.id;
                doc.add(folder)?;
                for mut layer in imported.document.layers {
                    if layer.parent.is_none() {
                        layer.parent = Some(id);
                    }
                    for (axis, offset) in delta.iter().enumerate() {
                        layer.transform.origin[axis] += offset;
                    }
                    if let Some(mask) = &mut layer.mask
                        && let Some(placement) = &mut mask.placement
                    {
                        for (axis, offset) in delta.iter().enumerate() {
                            placement.origin[axis] += offset;
                        }
                    }
                    doc.add(layer)?;
                }
                doc.select(id, false);
                Ok(())
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};
    fn imported() -> Imported {
        Imported {
            document: Document::new(8, 6).unwrap(),
            report: ConversionReport {
                changes: vec!["Editable text becomes pixels.".into()],
            },
        }
    }
    #[test]
    fn psd_conversion_cancel_keeps_tabs_pixels_history_and_blocks_underlying_commands() {
        let mut editor = Editor::with_test_document();
        let original = editor.session().document.clone();
        editor.psd_conversion = Some(Conversion::Files {
            completed: Box::new(Completed::Opened {
                projects: vec![OpenedProject::Psd(imported())],
                failures: vec![],
            }),
            report: ConversionReport::default(),
        });
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("PSD cancel").size(1280., 850.), editor)
            .unwrap();
        let window = view.window_handle();
        cx.read(view, |e| {
            assert!(!e.action_available(Action::Save));
            assert!(!e.can_switch_projects());
        })
        .unwrap();
        cx.simulate_keystrokes(window, "ctrl-z ctrl-n").unwrap();
        cx.click(window, "psd-cancel").unwrap();
        cx.read(view, |e| {
            assert!(e.psd_conversion.is_none());
            assert_eq!(e.tabs.len(), 1);
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
    }
    #[test]
    fn confirmed_psd_import_wraps_layers_and_is_undoable_without_changing_save_path() {
        let mut editor = Editor::with_test_document();
        let original = editor.session().document.clone();
        let id = editor.tabs[0].id;
        editor.psd_conversion = Some(Conversion::Files {
            completed: Box::new(Completed::Imported {
                session: id,
                layers: vec![],
                center: Some([20., 30.]),
                projects: vec![],
                psds: vec![("Imported PSD".into(), imported())],
            }),
            report: ConversionReport::default(),
        });
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("PSD confirm").size(1280., 850.), editor)
            .unwrap();
        cx.click(view.window_handle(), "psd-continue").unwrap();
        cx.read(view, |e| {
            let doc = &e.session().document;
            assert_eq!(doc.layers.len(), 3);
            assert!(doc.layers[1].is_group());
            assert_eq!(doc.layers[2].parent, Some(doc.layers[1].id));
            assert_eq!(doc.layers[2].transform.origin, [16., 27.]);
            assert!(e.session().path.is_none());
        })
        .unwrap();
        cx.update(view, |e, _| e.session_mut().undo()).unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
    }
    #[test]
    fn psd_export_requires_confirmation_and_startup_queues_psd() {
        let mut editor = Editor::with_test_document();
        editor.prepare_psd_export();
        assert!(matches!(
            editor.psd_conversion,
            Some(Conversion::Export { .. })
        ));
        assert!(editor.file_job.is_none());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("input.psd");
        std::fs::write(
            &path,
            compositor::psd::encode(&Document::new(2, 2).unwrap()).unwrap(),
        )
        .unwrap();
        let editor = Editor::new(vec![path]).unwrap();
        assert!(!editor.has_document());
        assert!(editor.launch_queue.pop().is_some());
    }
}
