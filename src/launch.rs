use std::{
    cell::RefCell,
    collections::VecDeque,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone, Default)]
pub(crate) struct LaunchQueue(Rc<RefCell<VecDeque<Vec<PathBuf>>>>);

impl LaunchQueue {
    pub fn push(&self, paths: Vec<PathBuf>) -> compositor::Result<()> {
        let mut queue = self.0.borrow_mut();
        let bytes = queue
            .iter()
            .flatten()
            .chain(&paths)
            .fold(0_usize, |total, path| {
                total.saturating_add(path.as_os_str().len())
            });
        if queue.iter().map(Vec::len).sum::<usize>() + paths.len() > 4096 || bytes > 8 * 1024 * 1024
        {
            return Err(compositor::invalid(
                "Too many files are waiting to open. Finish the current operation, then open these files again.",
            ));
        }
        if !paths.is_empty() {
            queue.push_back(paths);
        }
        Ok(())
    }

    pub fn pop(&self) -> Option<Vec<PathBuf>> {
        self.0.borrow_mut().pop_front()
    }
}

pub(crate) fn forwarded_paths<'a>(
    args: impl IntoIterator<Item = &'a str>,
    cwd: &Path,
) -> Vec<PathBuf> {
    args.into_iter().map(|arg| cwd.join(arg)).collect()
}

/// Keep independent desktop sessions separate, including nested test displays.
pub(crate) fn instance_identifier() -> String {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    std::env::var_os("HOME").hash(&mut hash);
    std::env::var_os("XDG_RUNTIME_DIR").hash(&mut hash);
    match std::env::var_os("WAYLAND_DISPLAY") {
        Some(display) => ("wayland", display).hash(&mut hash),
        None => ("x11", std::env::var_os("DISPLAY")).hash(&mut hash),
    }
    format!("compositor.{:016x}", hash.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_relative_paths_keep_the_senders_directory_and_request_order() {
        let queue = LaunchQueue::default();
        queue
            .push(forwarded_paths(
                ["Photo with spaces.png", "/images/Other.comp"],
                Path::new("/work"),
            ))
            .unwrap();
        queue.push(vec![PathBuf::from("/third.png")]).unwrap();
        assert_eq!(
            queue.pop().unwrap(),
            [
                PathBuf::from("/work/Photo with spaces.png"),
                PathBuf::from("/images/Other.comp")
            ]
        );
        assert_eq!(queue.pop().unwrap(), [PathBuf::from("/third.png")]);
        assert!(queue.pop().is_none());
        queue.push(vec![PathBuf::from("/image.png"); 4096]).unwrap();
        assert!(queue.push(vec![PathBuf::from("/overflow.png")]).is_err());
        assert_eq!(queue.pop().unwrap().len(), 4096);
        assert!(
            queue
                .push(vec![PathBuf::from("x".repeat(8 * 1024 * 1024 + 1))])
                .is_err()
        );
        assert!(queue.pop().is_none());
    }
}
