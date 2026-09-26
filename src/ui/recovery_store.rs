//! Durable recovery copies are separate from user projects and locked per process.
use compositor::{Result, document::Document, invalid, project};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

#[derive(Clone)]
pub(super) struct Store {
    pub directory: PathBuf,
    _lock: Arc<File>,
}
#[derive(Serialize, Deserialize)]
struct Label {
    name: String,
}
pub(super) struct Recovered {
    pub id: Uuid,
    pub document: Document,
    pub name: String,
}
pub(super) struct Startup {
    pub store: Store,
    pub recovered: Vec<Recovered>,
    pub warnings: Vec<String>,
}

impl Store {
    pub fn initialize(root: &Path) -> Result<Startup> {
        fs::create_dir_all(root)?;
        let directory = root.join(Uuid::new_v4().to_string());
        fs::create_dir(&directory)?;
        let lock = File::create(directory.join("lock"))?;
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::LockExclusive)
            .map_err(std::io::Error::from)?;
        File::open(root)?.sync_all()?;
        let store = Self {
            directory,
            _lock: Arc::new(lock),
        };
        let mut recovered = Vec::new();
        let mut warnings = Vec::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir()
                || entry.path() == store.directory
                || Uuid::parse_str(&entry.file_name().to_string_lossy()).is_err()
            {
                continue;
            }
            let directory = entry.path();
            let lock = match fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(directory.join("lock"))
            {
                Ok(lock) => lock,
                Err(error) => {
                    warnings.push(format!(
                        "Could not inspect recovery folder {}: {error}",
                        directory.display()
                    ));
                    continue;
                }
            };
            match rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => {}
                Err(error) if error == rustix::io::Errno::WOULDBLOCK => continue,
                Err(error) => {
                    warnings.push(format!(
                        "Could not lock recovery folder {}: {error}",
                        directory.display()
                    ));
                    continue;
                }
            }
            for package in fs::read_dir(&directory)? {
                let package = package?;
                if !package.file_type()?.is_dir()
                    || package.path().extension().is_none_or(|e| e != "comp")
                {
                    continue;
                }
                let path = package.path();
                match project::load_verified(&path) {
                    Ok((document, _)) => {
                        let name = File::open(path.join("recovery.json"))
                            .ok()
                            .and_then(|file| {
                                let mut bytes = Vec::new();
                                file.take(65537).read_to_end(&mut bytes).ok().map(|_| bytes)
                            })
                            .filter(|bytes| bytes.len() <= 65536)
                            .and_then(|bytes| serde_json::from_slice::<Label>(&bytes).ok())
                            .map(|label| label.name)
                            .unwrap_or_else(|| "Untitled".into());
                        // Persist a new copy before retiring the old one. A crash at
                        // any point leaves at least one complete recoverable package.
                        let id = Uuid::new_v4();
                        match store.write(id, &name, &document) {
                            Ok(()) => {
                                recovered.push(Recovered { id, document, name });
                                if let Err(error) = fs::remove_dir_all(&path) { warnings.push(format!("Recovered the project, but could not remove old copy {}: {error}", path.display())); }
                            }
                            Err(error) => warnings.push(format!("Could not retain recovered project {}: {error}. Its previous recovery copy is preserved.", path.display())),
                        }
                    }
                    Err(error) => warnings.push(format!(
                        "Could not recover {}: {error}. Its recovery copy is preserved.",
                        path.display()
                    )),
                }
            }
        }
        Ok(Startup {
            store,
            recovered,
            warnings,
        })
    }
    pub fn path(&self, id: Uuid) -> PathBuf {
        self.directory.join(format!("{id}.comp"))
    }
    pub fn write(&self, id: Uuid, name: &str, document: &Document) -> Result<()> {
        let path = self.path(id);
        project::save(document, &path)?;
        let mut file = tempfile::NamedTempFile::new_in(&path)?;
        file.write_all(&serde_json::to_vec(&Label { name: name.into() })?)?;
        file.as_file().sync_all()?;
        file.persist(path.join("recovery.json"))
            .map_err(|e| e.error)?;
        File::open(&path)?.sync_all()?;
        Ok(())
    }
    pub fn remove(&self, id: Uuid) -> Result<()> {
        match fs::remove_dir_all(self.path(id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
    pub fn clear(&self) -> Result<()> {
        fs::remove_dir_all(&self.directory)?;
        Ok(())
    }
}
pub(super) fn state_root() -> Result<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state")))
        .map(|p| p.join("compositor/recovery"))
        .ok_or_else(|| {
            invalid("No state directory is available. Set XDG_STATE_HOME to enable crash recovery.")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recover_crash_without_reading_another_live_instances_work() {
        let root = tempfile::tempdir().unwrap();
        let first = Store::initialize(root.path()).unwrap();
        let id = Uuid::new_v4();
        first
            .store
            .write(id, "Painting", &Document::new(4, 3).unwrap())
            .unwrap();
        let second = Store::initialize(root.path()).unwrap();
        assert!(second.recovered.is_empty());
        drop(first);
        let third = Store::initialize(root.path()).unwrap();
        assert_eq!(third.recovered.len(), 1);
        assert_eq!(third.recovered[0].name, "Painting");
        assert_eq!(third.recovered[0].document.width, 4);
        assert!(
            third
                .store
                .path(third.recovered[0].id)
                .join("manifest.json")
                .exists()
        );
        third.store.clear().unwrap();
        drop(third);
        let fourth = Store::initialize(root.path()).unwrap();
        assert!(fourth.recovered.is_empty());
    }
    #[test]
    fn failed_recovery_keeps_the_only_copy_and_reports_the_problem() {
        let root = tempfile::tempdir().unwrap();
        let first = Store::initialize(root.path()).unwrap();
        let path = first.store.path(Uuid::new_v4());
        fs::create_dir(&path).unwrap();
        fs::write(path.join("manifest.json"), "invalid").unwrap();
        drop(first);
        let next = Store::initialize(root.path()).unwrap();
        assert!(path.exists());
        assert_eq!(next.warnings.len(), 1);
    }
}
