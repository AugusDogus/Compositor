use super::*;

pub(super) struct ProjectTab {
    pub id: uuid::Uuid,
    name: String,
    content: Content,
    // Swapped with Editor.tools while this project is active.
    pub(super) parked_tools: project_tools::ProjectTools,
}

enum Content {
    Empty {
        draft: new_canvas::CanvasDraft,
        path: Option<PathBuf>,
    },
    Document(Box<Session>),
    UndoneCreation {
        draft: new_canvas::CanvasDraft,
        session: Box<Session>,
    },
}

impl ProjectTab {
    pub fn empty(name: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            name,
            content: Content::Empty {
                draft: Default::default(),
                path: None,
            },
            parked_tools: project_tools::ProjectTools::default(),
        }
    }

    pub fn session(&self) -> Option<&Session> {
        match &self.content {
            Content::Empty { .. } | Content::UndoneCreation { .. } => None,
            Content::Document(session) => Some(session),
        }
    }

    pub fn session_mut(&mut self) -> Option<&mut Session> {
        match &mut self.content {
            Content::Empty { .. } | Content::UndoneCreation { .. } => None,
            Content::Document(session) => Some(session),
        }
    }

    pub fn canvas_draft(&self) -> Option<&new_canvas::CanvasDraft> {
        match &self.content {
            Content::Empty { draft, .. } | Content::UndoneCreation { draft, .. } => Some(draft),
            Content::Document(_) => None,
        }
    }

    pub fn canvas_draft_mut(&mut self) -> Option<&mut new_canvas::CanvasDraft> {
        match &mut self.content {
            Content::Empty { draft, .. } | Content::UndoneCreation { draft, .. } => Some(draft),
            Content::Document(_) => None,
        }
    }

    #[cfg(test)]
    pub fn set_document(&mut self, document: Document, path: Option<PathBuf>) {
        let mut session = Session::new(document, path);
        session.id = self.id;
        self.content = Content::Document(Box::new(session));
    }

    pub fn create_document(&mut self, document: Document) -> Result<()> {
        let mut session = Session::created(document, "New Canvas")?;
        session.id = self.id;
        self.content = Content::Document(Box::new(session));
        Ok(())
    }

    pub fn history_session_mut(&mut self) -> Option<&mut Session> {
        match &mut self.content {
            Content::Document(session) | Content::UndoneCreation { session, .. } => Some(session),
            Content::Empty { .. } => None,
        }
    }

    pub fn undo_label(&self) -> Option<&str> {
        let session = self.session()?;
        session.undo_label().or_else(|| session.creation_label())
    }

    pub fn redo_label(&self) -> Option<&str> {
        match &self.content {
            Content::Document(session) => session.redo_label(),
            Content::UndoneCreation { session, .. } => session.creation_label(),
            Content::Empty { .. } => None,
        }
    }

    pub fn at_creation(&self) -> bool {
        self.session().is_some_and(|session| {
            session.undo_label().is_none() && session.creation_label().is_some()
        })
    }

    pub fn undo(&mut self) {
        if self.at_creation() {
            let content = std::mem::replace(
                &mut self.content,
                Content::Empty {
                    draft: Default::default(),
                    path: None,
                },
            );
            self.content = match content {
                Content::Document(mut session) => {
                    if session.retain_creation_redo() {
                        Content::UndoneCreation {
                            draft: Default::default(),
                            session,
                        }
                    } else {
                        Content::Empty {
                            draft: Default::default(),
                            path: session.path.take(),
                        }
                    }
                }
                content => content,
            };
        } else if let Some(session) = self.session_mut() {
            session.undo();
        }
    }

    pub fn redo(&mut self) {
        if matches!(self.content, Content::UndoneCreation { .. }) {
            let content = std::mem::replace(
                &mut self.content,
                Content::Empty {
                    draft: Default::default(),
                    path: None,
                },
            );
            self.content = match content {
                Content::UndoneCreation { mut session, .. } => {
                    session.fit = true;
                    session.pan = [0., 0.];
                    Content::Document(session)
                }
                content => content,
            };
        } else if let Some(session) = self.session_mut() {
            session.redo();
        }
    }

    pub fn dirty(&self) -> bool {
        match &self.content {
            Content::Document(session) => session.dirty(),
            Content::UndoneCreation { session, .. } => session.path.is_some(),
            Content::Empty { path, .. } => path.is_some(),
        }
    }

    pub fn needs_save(&self) -> bool {
        self.session().is_some_and(Session::dirty)
    }

    /// Initialize an empty tab only after its first edit succeeds. Failed imports
    /// and clipboard decodes must leave the welcome tab intact.
    pub fn edit_or_create(
        &mut self,
        label: &str,
        create: impl FnOnce() -> Result<Document>,
        edit: impl FnOnce(&mut Document) -> Result<()>,
    ) -> Result<()> {
        if let Some(session) = self.session_mut() {
            return session.edit(label, edit);
        }
        let mut document = create()?;
        edit(&mut document)?;
        document.validate()?;
        if let Content::UndoneCreation { session, .. } = &mut self.content {
            session.replace_creation(document, label)?;
            self.redo();
            return Ok(());
        }
        let mut session = Session::created(document, label)?;
        if let Content::Empty { path, .. } = &self.content {
            session.path = path.clone();
        }
        session.id = self.id;
        self.content = Content::Document(Box::new(session));
        Ok(())
    }

    pub fn title(&self) -> String {
        let path = match &self.content {
            Content::Document(session) | Content::UndoneCreation { session, .. } => {
                session.path.as_ref()
            }
            Content::Empty { path, .. } => path.as_ref(),
        };
        path.and_then(|path| path.file_stem())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.name.clone())
    }
}

impl From<Session> for ProjectTab {
    fn from(session: Session) -> Self {
        Self {
            id: session.id,
            name: "Untitled".into(),
            content: Content::Document(Box::new(session)),
            parked_tools: project_tools::ProjectTools::default(),
        }
    }
}
