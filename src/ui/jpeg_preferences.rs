//! Persist only the quality of a successfully written JPEG export.
use compositor::{Result, invalid};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

fn path() -> Result<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .map(|p| p.join("compositor/jpeg-quality"))
        .ok_or_else(|| invalid("No configuration directory is available. Set XDG_CONFIG_HOME to remember JPEG quality."))
}
pub(super) fn load() -> Result<u8> {
    read(&path()?)
}
pub(super) fn remember(quality: u8) -> Result<()> {
    save(&path()?, quality)
}
fn read(path: &Path) -> Result<u8> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(85),
        Err(e) => return Err(e.into()),
    };
    let mut text = String::new();
    file.take(32).read_to_string(&mut text)?;
    text.trim()
        .parse::<u8>()
        .ok()
        .filter(|v| *v <= 100)
        .ok_or_else(|| {
            invalid("The saved JPEG quality is invalid. Change quality and export to replace it.")
        })
}
fn save(path: &Path, quality: u8) -> Result<()> {
    if quality > 100 {
        return Err(invalid("JPEG quality must be 0 to 100."));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("The JPEG preference has no parent directory."))?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    writeln!(file, "{quality}")?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quality_defaults_and_round_trips_the_full_range() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quality");
        assert_eq!(read(&path).unwrap(), 85);
        for quality in [0, 37, 100] {
            save(&path, quality).unwrap();
            assert_eq!(read(&path).unwrap(), quality);
        }
        assert!(save(&path, 101).is_err());
        std::fs::write(&path, "37\ninvalid").unwrap();
        assert!(read(&path).is_err());
    }
}
