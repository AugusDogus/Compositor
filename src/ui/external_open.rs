use super::*;

impl Editor {
    pub fn set_launch_queue(&mut self, queue: crate::launch::LaunchQueue) {
        self.launch_queue = queue;
    }

    pub(super) fn start_external_open(&mut self) {
        if !self.can_switch_projects() {
            return;
        }
        if let Some(paths) = self.launch_queue.pop()
            && let Err(error) = self.open_paths(paths, false)
        {
            self.status = format!(
                "Could not open the requested files: {error}. Your existing edits are preserved. Finish the current edit and open the files again."
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_opens_wait_for_active_work_and_keep_their_order() {
        let mut e = Editor::with_test_document();
        let queue = crate::launch::LaunchQueue::default();
        e.set_launch_queue(queue.clone());
        let first = vec![PathBuf::from("/first.comp")];
        let second = vec![PathBuf::from("/second.png")];
        queue.push(first.clone()).unwrap();
        queue.push(second.clone()).unwrap();
        e.pending = true;
        e.start_external_open();
        assert!(e.file_job.is_none());
        e.pending = false;
        e.open_form(Action::CanvasSize);
        e.start_external_open();
        assert!(e.file_job.is_none());
        e.modal = None;
        e.start_external_open();
        let Some(file_jobs::FileJob::Open(paths)) = e.file_job.take() else {
            panic!("Expected first file request");
        };
        assert_eq!(paths, first);
        e.start_external_open();
        assert!(e.file_job.is_none());
        e.pending = false;
        e.start_external_open();
        let Some(file_jobs::FileJob::Open(paths)) = e.file_job.take() else {
            panic!("Expected second file request");
        };
        assert_eq!(paths, second);
    }
}
