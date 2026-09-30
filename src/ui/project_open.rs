use super::*;
use uuid::Uuid;

pub(super) enum OpenedProject {
    Existing(Uuid),
    RenderedCopy(super::project_authoring::Request),
    Raw(PathBuf),
    Psd(compositor::psd::Imported),
    Image(Box<Document>),
    Loaded {
        document: Box<Document>,
        path: Option<PathBuf>,
        fingerprint: Option<project::Fingerprint>,
    },
}

impl OpenedProject {
    pub(super) fn load(path: PathBuf, open: &[(Uuid, PathBuf)]) -> Result<Self> {
        // Reopening a known path still selects its edits if the on-disk package
        // has since disappeared. Otherwise resolve aliases before loading.
        if let Some((id, _)) = open.iter().find(|(_, saved)| *saved == path) {
            return Ok(Self::Existing(*id));
        }
        let path = path.canonicalize()?;
        if let Some((id, _)) = open.iter().find(|(_, saved)| *saved == path) {
            return Ok(Self::Existing(*id));
        }
        if path.is_dir() {
            return Ok(match project::inspect_open(&path)? {
                project::OpenResult::Editable {
                    document,
                    fingerprint,
                } => Self::Loaded {
                    document: Box::new(document),
                    fingerprint: Some(fingerprint),
                    path: Some(path),
                },
                project::OpenResult::RenderedCopyAvailable {
                    reason,
                    fingerprint,
                } => Self::RenderedCopy(super::project_authoring::Request {
                    path,
                    reason,
                    fingerprint,
                }),
            });
        }
        if compositor::raw::is_raw(&path) {
            return Ok(Self::Raw(path));
        }
        if compositor::psd::is_psd(&path)? {
            return Ok(Self::Psd(compositor::psd::load(&path)?));
        }
        let layer = image_io::import(&path)?;
        let mut doc = Document::new(
            layer.transform.size[0] as u32,
            layer.transform.size[1] as u32,
        )?;
        doc.layers.clear();
        doc.add(layer)?;
        Ok(Self::Image(Box::new(doc)))
    }
}

pub(super) fn initial_tabs(
    paths: Vec<PathBuf>,
    launch_queue: &crate::launch::LaunchQueue,
) -> Result<Vec<ProjectTab>> {
    let mut tabs: Vec<ProjectTab> = Vec::new();
    for path in paths {
        if compositor::raw::is_raw(&path) || compositor::psd::is_psd(&path)? {
            launch_queue.push(vec![path])?;
            continue;
        }
        if path.is_dir() {
            let path = path.canonicalize()?;
            if !tabs.iter().any(|tab: &ProjectTab| {
                tab.session()
                    .is_some_and(|s| s.path.as_ref() == Some(&path))
            }) {
                match project::inspect_open(&path)? {
                    project::OpenResult::Editable {
                        document,
                        fingerprint,
                    } => {
                        let mut session = Session::new(document, Some(path));
                        session.disk_fingerprint = Some(fingerprint);
                        tabs.push(session.into());
                    }
                    project::OpenResult::RenderedCopyAvailable { .. } => {
                        launch_queue.push(vec![path])?
                    }
                }
            }
        } else {
            let layer = image_io::import(&path)?;
            let mut doc = Document::new(
                layer.transform.size[0] as u32,
                layer.transform.size[1] as u32,
            )?;
            doc.layers.clear();
            doc.add(layer)?;
            tabs.push(Session::from_image(doc).into());
        }
    }
    Ok(tabs)
}

impl Editor {
    pub(super) fn show_opened_projects(&mut self, projects: Vec<OpenedProject>) -> Result<()> {
        for project in projects {
            let project = match project {
                OpenedProject::Psd(imported) => OpenedProject::Loaded {
                    document: Box::new(imported.document),
                    path: None,
                    fingerprint: None,
                },
                project => project,
            };
            let index = match project {
                OpenedProject::Image(document) => {
                    self.attach_opened_session(Session::from_image(*document))
                }
                OpenedProject::RenderedCopy(request) => {
                    self.authoring_copies.push_back(request);
                    continue;
                }
                OpenedProject::Raw(path) => {
                    self.queue_raw(path, raw_develop::Target::New);
                    continue;
                }
                OpenedProject::Psd(_) => {
                    return Err(compositor::invalid("PSD conversion was not resolved."));
                }
                OpenedProject::Existing(id) => self
                    .tabs
                    .iter()
                    .position(|tab| tab.id == id)
                    .ok_or_else(|| {
                        compositor::invalid(
                            "The project tab closed while opening its file. Open the file again.",
                        )
                    })?,
                OpenedProject::Loaded {
                    document,
                    path,
                    fingerprint,
                } => {
                    let mut loaded = Session::new(*document, path);
                    loaded.disk_fingerprint = fingerprint;
                    self.attach_opened_session(loaded)
                }
            };
            self.activate_tab(index);
        }
        Ok(())
    }

    fn attach_opened_session(&mut self, loaded: Session) -> usize {
        if let Some(path) = &loaded.path {
            self.remember_project(path.clone());
        }
        // Two aliases in one request can load together. Resolve them
        // against the tabs appended earlier in this same batch.
        if let Some(index) = loaded.path.as_ref().and_then(|path| {
            self.tabs.iter().position(|tab| {
                tab.session()
                    .is_some_and(|session| session.path.as_ref() == Some(path))
            })
        }) {
            index
        } else if self.tabs.len() == 1 && !self.has_document() {
            self.tabs[0] = loaded.into();
            let toggles = tool_defaults::Toggles::capture(&self.tools);
            self.tools = project_tools::ProjectTools::default();
            toggles.apply(&mut self.tools);
            0
        } else {
            self.tabs.push(loaded.into());
            self.tabs.len() - 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opened_images_only_prompt_on_close_after_edits() {
        use quickgui::{Application, WindowOptions};

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Image.png");
        image::RgbaImage::from_pixel(8, 6, image::Rgba([20, 40, 60, 255]))
            .save(&path)
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        for from_args in [true, false] {
            for history_steps in 0..4 {
                let mut e = if from_args {
                    Editor::new(vec![path.clone()]).unwrap()
                } else {
                    let mut e = Editor::new(Vec::new()).unwrap();
                    e.show_opened_projects(vec![OpenedProject::load(path.clone(), &[]).unwrap()])
                        .unwrap();
                    e
                };
                // Saving must still ask for a project destination, never overwrite the image.
                assert!(e.session().path.is_none());
                e.session_mut().keyboard_zoom(true);
                if history_steps > 0 {
                    e.session_mut()
                        .edit("Paint", |doc| {
                            compositor::edits::fill(doc, [100, 120, 140, 255], false, false)
                        })
                        .unwrap();
                }
                if history_steps > 1 {
                    e.session_mut().undo();
                }
                if history_steps > 2 {
                    e.session_mut().redo();
                }
                let modified = history_steps % 2 == 1;
                assert_eq!(e.session().dirty(), modified);
                let (mut cx, view) = Application::new()
                    .into_test_context(WindowOptions::new("Close image").size(1280., 850.), e)
                    .unwrap();
                let window = view.window_handle();
                cx.simulate_close_requested(window).unwrap();
                assert_eq!(cx.is_window_open(window), modified);
                if modified {
                    assert!(
                        cx.read(view, |e| matches!(e.modal, Some(Form::Close)))
                            .unwrap()
                    );
                }
                assert_eq!(std::fs::read(&path).unwrap(), original);
            }
        }
    }

    #[test]
    fn reopening_a_project_or_alias_selects_unsaved_edits_without_reloading() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Original.comp");
        let alias = directory.path().join("Alias.comp");
        project::save(&Document::new(4, 3).unwrap(), &path).unwrap();
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        let mut e = Editor::new(vec![path.clone()]).unwrap();
        e.session_mut()
            .edit("Unsaved paint", |doc| {
                compositor::edits::fill(doc, [80, 100, 120, 255], false, false)
            })
            .unwrap();
        let edited = e.session().document.clone();
        let original = e.tabs[0].id;
        let open = vec![(original, e.session().path.clone().unwrap())];
        e.add_empty_tab();
        let empty = e.tabs[1].id;
        e.show_opened_projects(vec![OpenedProject::load(alias, &open).unwrap()])
            .unwrap();
        assert_eq!(e.tabs.len(), 2);
        assert_eq!(e.tabs[e.current].id, original);
        assert_eq!(e.session().document, edited);
        assert!(e.tabs.iter().any(|tab| tab.id == empty));
        std::fs::remove_dir_all(&path).unwrap();
        e.activate_tab(1);
        e.show_opened_projects(vec![OpenedProject::load(path, &open).unwrap()])
            .unwrap();
        assert_eq!(e.tabs[e.current].id, original);
        assert_eq!(e.session().document, edited);
        assert_eq!(e.session().undo_label(), Some("Unsaved paint"));
    }

    #[test]
    fn aliases_in_one_open_request_or_at_startup_create_only_one_tab() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Original.comp");
        let alias = directory.path().join("Alias.comp");
        project::save(&Document::new(4, 3).unwrap(), &path).unwrap();
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        let mut e = Editor::new(Vec::new()).unwrap();
        e.show_opened_projects(vec![
            OpenedProject::load(path.clone(), &[]).unwrap(),
            OpenedProject::load(alias.clone(), &[]).unwrap(),
        ])
        .unwrap();
        assert_eq!(e.tabs.len(), 1);
        assert!(!e.tabs[0].dirty());
        let from_args = Editor::new(vec![path, alias]).unwrap();
        assert_eq!(from_args.tabs.len(), 1);
        assert_eq!(from_args.session().document, e.session().document);
    }
}
