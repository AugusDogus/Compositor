use super::*;
use uuid::Uuid;

pub(super) enum OpenedProject {
    Existing(Uuid),
    Psd(compositor::psd::Imported),
    Loaded {
        document: Box<Document>,
        path: Option<PathBuf>,
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
            return Ok(Self::Loaded {
                document: Box::new(project::load(&path)?),
                path: Some(path),
            });
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
        Ok(Self::Loaded {
            document: Box::new(doc),
            path: None,
        })
    }
}

impl Editor {
    pub(super) fn show_opened_projects(&mut self, projects: Vec<OpenedProject>) -> Result<()> {
        for project in projects {
            let project = match project {
                OpenedProject::Psd(imported) => OpenedProject::Loaded {
                    document: Box::new(imported.document),
                    path: None,
                },
                project => project,
            };
            let index = match project {
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
                OpenedProject::Loaded { document, path } => {
                    // Two aliases in one request can load together. Resolve them
                    // against the tabs appended earlier in this same batch.
                    if let Some(index) = path.as_ref().and_then(|path| {
                        self.tabs.iter().position(|tab| {
                            tab.session()
                                .is_some_and(|session| session.path.as_ref() == Some(path))
                        })
                    }) {
                        index
                    } else {
                        if self.tabs.len() == 1 && !self.has_document() {
                            self.tabs[0] = Session::new(*document, path).into();
                            self.tools = project_tools::ProjectTools::default();
                            0
                        } else {
                            self.tabs.push(Session::new(*document, path).into());
                            self.tabs.len() - 1
                        }
                    }
                }
            };
            self.activate_tab(index);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
