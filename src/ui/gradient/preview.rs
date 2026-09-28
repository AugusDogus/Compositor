//! One full-resolution worker at a time; pointer events only replace the requested endpoints.
use super::*;

struct Request {
    id: Uuid,
    revision: u64,
    document: Document,
    start: Point,
    end: Point,
    gradient: compositor::gradient::Gradient,
    colors: [[u8; 4]; 2],
    mask: bool,
}

#[cfg(test)]
mod tests;

impl Request {
    fn render(mut self) -> Result<Document> {
        self.gradient.apply(
            &mut self.document,
            self.start,
            self.end,
            self.colors[0],
            self.colors[1],
            self.mask,
        )?;
        Ok(self.document)
    }
}

impl Editor {
    fn take_gradient_request(&mut self) -> Option<Request> {
        if self.gradient_worker.is_some() {
            return None;
        }
        let edit = self.pending_gradient.as_ref()?;
        if edit.preview != Preview::Queued {
            return None;
        }
        let request = Request {
            id: edit.id,
            revision: edit.revision,
            document: edit.original.clone(),
            start: edit.start,
            end: edit.end,
            gradient: self.tools.gradient.clone(),
            colors: self.palette_colors(edit.mask),
            mask: edit.mask,
        };
        self.gradient_worker = Some(edit.id);
        Some(request)
    }

    pub(in crate::ui) fn start_gradient_preview(&mut self, cx: &ViewContext<'_, Self>) {
        let Some(request) = self.take_gradient_request() else {
            return;
        };
        let (id, revision) = (request.id, request.revision);
        let launched = cx.spawn_background(
            move || request.render(),
            move |this, result, cx| {
                let result = result
                    .map_err(|error| compositor::invalid(format!("Gradient preview worker failed: {error}. Your original pixels are preserved.")))
                    .and_then(|result| result);
                let result = this.receive_gradient_preview(id, revision, result);
                this.operation_result(alerts::Operation::Paint, result, cx);
            },
        );
        if let Err(error) = launched {
            self.gradient_worker = None;
            if let Some(edit) = &mut self.pending_gradient {
                edit.preview = Preview::Failed;
            }
            self.status = format!(
                "Could not start the gradient preview: {error}. Your original pixels are preserved. Try drawing the gradient again."
            );
        }
    }

    fn receive_gradient_preview(
        &mut self,
        id: Uuid,
        revision: u64,
        result: Result<Document>,
    ) -> Result<()> {
        if self.gradient_worker != Some(id) {
            return Ok(());
        }
        self.gradient_worker = None;
        let Some(edit) = &mut self.pending_gradient else {
            return Ok(());
        };
        if edit.id != id || edit.preview == Preview::Ready {
            return Ok(());
        }
        let current = edit.revision == revision;
        let session_id = edit.session;
        let Some(session) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == session_id)
            .and_then(ProjectTab::session_mut)
        else {
            self.pending_gradient = None;
            return Ok(());
        };
        match result {
            Ok(document) => {
                // Display useful progress during continuous dragging, but only the
                // latest request may enable Apply. Cancelled edits never land here.
                session.document = document;
                if current {
                    edit.preview = Preview::Ready;
                }
                Ok(())
            }
            Err(error) if current => {
                session.cancel();
                self.pending_gradient = None;
                Err(error)
            }
            Err(_) => Ok(()), // A newer request supersedes this failed preview.
        }
    }

    #[cfg(test)]
    pub(in crate::ui) fn finish_gradient_preview_for_test(&mut self) -> Result<()> {
        // QuickGUI's deterministic test context has no worker pool. Resolve the
        // latest queued (or unavailable-pool) request through the real renderer.
        if let Some(edit) = &mut self.pending_gradient {
            if edit.preview == Preview::Ready {
                return Ok(());
            }
            edit.preview = Preview::Queued;
        }
        let request = self
            .take_gradient_request()
            .expect("queued gradient preview");
        let (id, revision) = (request.id, request.revision);
        self.receive_gradient_preview(id, revision, request.render())
    }
}
