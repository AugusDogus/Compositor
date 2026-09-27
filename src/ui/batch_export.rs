//! Folder exports capture one committed document before opening the chooser.
use super::*;
use quickgui::PathPromptOptions;
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Layers,
    Artboards,
}
impl Kind {
    fn operation(self) -> alerts::Operation {
        match self {
            Self::Layers => alerts::Operation::ExportLayers,
            Self::Artboards => alerts::Operation::ExportArtboards,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Layers => "Layers",
            Self::Artboards => "Artboards",
        }
    }
    fn job(self, document: Document, parent: PathBuf, title: String) -> file_jobs::FileJob {
        match self {
            Self::Layers => file_jobs::FileJob::ExportLayers {
                document,
                parent,
                title,
            },
            Self::Artboards => file_jobs::FileJob::ExportArtboards {
                document,
                parent,
                title,
            },
        }
    }
}
impl Editor {
    pub(super) fn prompt_batch_export(&mut self, kind: Kind, cx: &mut EventContext) {
        let document = self.session().committed_document().clone();
        let title = self.session().title();
        let operation = kind.operation();
        let label = kind.label();
        let options = PathPromptOptions::new()
            .files(false)
            .directories(true)
            .title(format!("Export {label}: Choose Destination"));
        match cx.prompt_for_paths(options) {
            Ok(response) => self.await_response(cx, operation, response, move |this, result, _| {
                match result {
                    Ok(Some(paths)) => {
                        if let Some(parent) = paths.into_iter().next() {
                            this.queue_file(kind.job(document, parent, title));
                        }
                    }
                    Ok(None) => this.status = "Export cancelled. Your project is unchanged.".into(),
                    Err(error) => this.show_error(operation, format!(
                        "Could not choose an export destination: {error}. Choose Export {label} again to retry."
                    )),
                }
            }),
            Err(error) => self.show_error(operation, format!(
                "Could not open the export destination dialog: {error}. Choose Export {label} again to retry."
            )),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layer_export_uses_snapshot_without_changing_editor_save_or_history() {
        let directory = tempfile::tempdir().unwrap();
        let mut document = Document::new(3, 3).unwrap();
        compositor::edits::fill(&mut document, [30, 80, 120, 255], false, false).unwrap();
        document.active_layer_mut().unwrap().name = "Blue".into();
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(document.clone(), None).into()];
        assert!(editor.action_available(Action::ExportLayers));
        let job = file_jobs::FileJob::ExportLayers {
            document,
            parent: directory.path().into(),
            title: "Snapshot".into(),
        };
        editor
            .session_mut()
            .edit("Fill", |doc| {
                compositor::edits::fill(doc, [200, 20, 0, 255], false, false)
            })
            .unwrap();
        let original = editor.session().document.clone();
        let revision = editor.session().revision();
        let file_jobs::Completed::Exported { path, .. } = job.run(&[]).unwrap() else {
            panic!("Expected layer export");
        };
        let files: Vec<_> = std::fs::read_dir(path)
            .unwrap()
            .map(|p| p.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1);
        let pixels = image_io::read_image(&files[0]).unwrap();
        assert!(pixels.pixels().all(|p| p.0 == [30, 80, 120, 255]));
        assert_eq!(editor.session().revision(), revision);
        assert_eq!(editor.session().document, original);
        assert!(editor.session().path.is_none());
        assert_eq!(editor.session().undo_label(), Some("Fill"));
    }
    #[test]
    fn layer_export_requires_visible_content_and_respects_busy_state() {
        let mut e = Editor::with_test_document();
        e.session_mut().document.layers.clear();
        e.session_mut().document.active = None;
        e.session_mut().document.selected.clear();
        assert!(!e.action_available(Action::ExportLayers));
        let mut doc = Document::new(4, 4).unwrap();
        compositor::edits::fill(&mut doc, [0, 0, 0, 255], false, false).unwrap();
        e.session_mut().document = doc;
        assert!(e.action_available(Action::ExportLayers));
        e.pending = true;
        assert!(!e.action_available(Action::ExportLayers));
        e.pending = false;
        e.session_mut().document.layers[0].visible = false;
        assert!(!e.action_available(Action::ExportLayers));
    }
}
