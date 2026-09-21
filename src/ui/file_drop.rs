use super::*;
use quickgui::{DropEvent, DroppedFiles};
use uuid::Uuid;

impl Editor {
    pub(super) fn drop_files(
        &mut self,
        files: &DroppedFiles,
        session: Option<Uuid>,
        event: &DropEvent,
        cx: &mut EventContext,
    ) {
        if !self.can_switch_projects() {
            return;
        }
        let result = self.prepare_file_drop(files, session, event.position);
        self.operation_result(alerts::Operation::Import, result, cx);
    }

    pub(super) fn prepare_file_drop(
        &mut self,
        files: &DroppedFiles,
        session: Option<Uuid>,
        position: quickgui::Point,
    ) -> Result<()> {
        if files.is_truncated() {
            return Err(compositor::invalid(
                "Too many files were dropped at once. Drop a smaller batch; no files have been imported.",
            ));
        }
        if files.is_empty() {
            return Ok(());
        }
        self.finish_pending_edits()?;
        let paths = files.paths().to_vec();
        let Some(session) = session else {
            self.queue_file(super::file_jobs::FileJob::Open(paths));
            return Ok(());
        };
        let (index, destination) = self
            .tabs
            .iter()
            .enumerate()
            .find(|(_, s)| s.id == session)
            .ok_or_else(|| {
                compositor::invalid(
                    "The destination tab has closed. Drop the files onto an open tab.",
                )
            })?;
        let mut center = destination
            .session()
            .map(|s| [s.document.width as f64 / 2., s.document.height as f64 / 2.]);
        if self.has_document()
            && self.tabs[self.current].id == session
            && let Some(bounds) = self
                .canvas_bounds
                .bounds()
                .filter(|bounds| bounds.contains(position))
        {
            let (zoom, offset) = self.viewport(bounds.width, bounds.height);
            center = Some([
                (f64::from(position.x - bounds.x) - offset[0]) / zoom,
                (f64::from(position.y - bounds.y) - offset[1]) / zoom,
            ]);
        }
        self.activate_tab(index);
        self.tools.pending_crop = None;
        self.queue_file(super::file_jobs::FileJob::Import {
            session,
            paths,
            center,
        });
        Ok(())
    }
}
