//! Publish complete export batches without replacing existing user files.
use crate::{Result, invalid};
use std::{
    fs::File,
    path::{Path, PathBuf},
};

/// `name` is an internal filename assembled from `safe_stem` and a fixed suffix.
pub(crate) fn publish(
    parent: &Path,
    name: &str,
    write: impl FnOnce(&Path) -> Result<()>,
) -> Result<PathBuf> {
    let staged = tempfile::Builder::new()
        .prefix(".compositor-export-")
        .tempdir_in(parent)?;
    write(staged.path())?;
    File::open(staged.path())?.sync_all()?;
    for suffix in 0..1000 {
        let name = if suffix == 0 {
            name.to_owned()
        } else {
            format!("{name}-{}", suffix + 1)
        };
        let destination = parent.join(name);
        match rustix::fs::renameat_with(
            rustix::fs::CWD,
            staged.path(),
            rustix::fs::CWD,
            &destination,
            rustix::fs::RenameFlags::NOREPLACE,
        ) {
            Ok(()) => {
                File::open(parent)?.sync_all().map_err(|error| invalid(format!("Exported to {}, but could not sync the containing folder: {error}. Verify these files before removing the source project.", destination.display())))?;
                return Ok(destination);
            }
            Err(rustix::io::Errno::EXIST) => continue,
            Err(error) => return Err(std::io::Error::from(error).into()),
        }
    }
    Err(invalid(
        "No unused export folder name was available. Choose another destination folder. Existing files and your project are unchanged.",
    ))
}

pub(crate) fn safe_stem(title: &str) -> String {
    let mut bytes = 0;
    let stem: String = title
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || matches!(ch, '-' | '_' | ' ') {
                ch
            } else {
                '_'
            }
        })
        .take_while(|ch| {
            bytes += ch.len_utf8();
            bytes <= 128
        })
        .collect();
    let stem = stem.trim();
    if stem.is_empty() {
        "Untitled".into()
    } else {
        stem.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_batch_removes_staging_and_never_publishes_partial_files() {
        let root = tempfile::tempdir().unwrap();
        let result = publish(root.path(), "Failed-layers", |directory| {
            std::fs::write(directory.join("01-first.png"), b"partial")?;
            Err(invalid("second export failed"))
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
