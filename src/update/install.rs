use crate::{Result, invalid};
use quickgui::UpdateCancellation;
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::Path,
};

/// Copy and verify beside the installed executable, then exchange both names
/// atomically. Even a process kill cannot leave the launcher without a file.
pub(super) fn replace(
    source: &Path,
    target: &Path,
    cancellation: &UpdateCancellation,
    verify: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let metadata = fs::symlink_metadata(target)?;
    if !metadata.is_file() {
        return Err(invalid(
            "The update destination is not a regular executable. Update through your package manager.",
        ));
    }
    let parent = target
        .parent()
        .ok_or_else(|| invalid("The update destination has no directory."))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| invalid(format!("Cannot stage an update beside {}: {error}. Use a user-writable installation or the package manager; the installed editor is unchanged.", target.display())))?;
    let mut source = File::open(source)?;
    if source.metadata()?.len() > 512 * 1024 * 1024 {
        return Err(invalid(
            "The update exceeds 512 MiB. The installed editor is unchanged.",
        ));
    }
    let mut buffer = [0; 64 * 1024];
    let mut copied = 0_u64;
    loop {
        check_cancelled(cancellation)?;
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        copied += count as u64;
        if copied > 512 * 1024 * 1024 {
            return Err(invalid(
                "The update grew beyond 512 MiB during installation. The installed editor is unchanged. Download it again.",
            ));
        }
        temporary.write_all(&buffer[..count])?;
    }
    temporary
        .as_file()
        .set_permissions(metadata.permissions())?;
    temporary.as_file().sync_all()?;
    verify(temporary.path())?;
    check_cancelled(cancellation)?;
    let current = fs::symlink_metadata(target)?;
    if current.dev() != metadata.dev() || current.ino() != metadata.ino() {
        return Err(invalid(
            "Another process replaced the editor during this update. Reopen it before checking for updates again.",
        ));
    }
    rustix::fs::renameat_with(
        rustix::fs::CWD, temporary.path(), rustix::fs::CWD, target,
        rustix::fs::RenameFlags::EXCHANGE,
    ).map_err(|error| invalid(format!("Could not atomically replace {}: {error}. The installed editor is unchanged. Use the package manager or a filesystem supporting rename exchange.", target.display())))?;
    // The temporary path now owns the previous executable, which remains mapped
    // by the current process until exit. Drop removes only that old directory entry.
    File::open(parent)?.sync_all().map_err(|error| invalid(format!("The new editor is installed, but its directory could not be synced: {error}. Keep the application open and check the filesystem before restarting.")))?;
    Ok(())
}

fn check_cancelled(cancellation: &UpdateCancellation) -> Result<()> {
    if cancellation.is_cancelled() {
        Err(invalid(
            "Update cancelled. The installed editor is unchanged.",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn replacement_preserves_mode_and_running_file_and_rejects_failed_verification() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("compositor");
        let source = directory.path().join("download");
        fs::write(&target, b"old editor").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o751)).unwrap();
        fs::write(&source, b"new editor").unwrap();
        let mut running = File::open(&target).unwrap();
        let cancel = UpdateCancellation::new();
        assert!(
            replace(&source, &target, &cancel, |_| Err(invalid(
                "invalid executable"
            )))
            .is_err()
        );
        assert_eq!(fs::read(&target).unwrap(), b"old editor");
        replace(&source, &target, &cancel, |path| {
            assert_eq!(fs::read(path).unwrap(), b"new editor");
            Ok(())
        })
        .unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new editor");
        let mut bytes = Vec::new();
        running.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"old editor");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o751
        );
        cancel.cancel();
        assert!(replace(&source, &target, &cancel, |_| Ok(())).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"new editor");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }
}
