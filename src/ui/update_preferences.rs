use compositor::{Result, invalid};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(super) fn path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .map(|p| p.join("compositor/automatic-updates"))
}

pub(super) fn read(path: &Path) -> Result<bool> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let mut value = String::new();
    file.take(32).read_to_string(&mut value)?;
    match value.as_str() {
        "enabled\n" => Ok(true),
        "disabled\n" => Ok(false),
        _ => Err(invalid(
            "The automatic-update preference is invalid. Toggle automatic checks to replace it; no check has started.",
        )),
    }
}

pub(super) fn save(path: &Path, enabled: bool) -> Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| invalid("The update preference has no parent directory."))?;
    std::fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(if enabled { b"enabled\n" } else { b"disabled\n" })?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_require_explicit_persisted_opt_in_and_reject_invalid_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("updates");
        assert!(!read(&path).unwrap());
        save(&path, true).unwrap();
        assert!(read(&path).unwrap());
        save(&path, false).unwrap();
        assert!(!read(&path).unwrap());
        std::fs::write(&path, "enabled\nmalformed").unwrap();
        assert!(read(&path).is_err());
    }
}
