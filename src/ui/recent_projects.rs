//! A bounded, atomic history of successfully opened or saved project packages.
use super::*;
use compositor::invalid;
use std::{
    io::{Read, Write},
    path::Path,
};
const LIMIT: usize = 12;
#[derive(Clone, PartialEq)]
pub(super) struct InvokeRecent(pub Option<PathBuf>);

#[derive(Default)]
pub(super) struct RecentProjects {
    paths: Vec<PathBuf>,
}
impl RecentProjects {
    pub fn load() -> Self {
        #[cfg(test)]
        {
            Self::default()
        }
        #[cfg(not(test))]
        {
            Self {
                paths: preference_path()
                    .and_then(|p| read(&p))
                    .unwrap_or_else(|error| {
                        eprintln!("Could not read recent projects: {error}");
                        Vec::new()
                    }),
            }
        }
    }
    pub fn existing(&self) -> Vec<PathBuf> {
        self.paths
            .iter()
            .filter(|path| path.join("manifest.json").is_file())
            .cloned()
            .collect()
    }
    pub fn remember(&mut self, path: PathBuf) -> Result<()> {
        self.paths.retain(|saved| saved != &path);
        self.paths.insert(0, path);
        self.paths.truncate(LIMIT);
        self.persist()
    }
    pub fn clear(&mut self) -> Result<()> {
        self.paths.clear();
        self.persist()
    }
    fn persist(&self) -> Result<()> {
        #[cfg(test)]
        {
            Ok(())
        }
        #[cfg(not(test))]
        {
            write(&preference_path()?, &self.paths)
        }
    }
}
#[cfg(not(test))]
fn preference_path() -> Result<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .map(|p| p.join("compositor/recent-projects.json"))
        .ok_or_else(|| invalid("No configuration directory is available. Set XDG_CONFIG_HOME to remember recent projects."))
}
fn read(path: &Path) -> Result<Vec<PathBuf>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut data = Vec::new();
    file.take(65_537).read_to_end(&mut data)?;
    if data.len() > 65_536 {
        return Err(invalid(
            "Recent project history exceeds 64 KiB. Clear Open Recent to reset it.",
        ));
    }
    let paths: Vec<PathBuf> = serde_json::from_slice(&data)?;
    Ok(paths
        .into_iter()
        .filter(|p| p.is_absolute())
        .take(LIMIT)
        .collect())
}
fn write(path: &Path, paths: &[PathBuf]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Recent project history has no parent directory."))?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&serde_json::to_vec(paths)?)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
impl Editor {
    pub(super) fn remember_project(&mut self, path: PathBuf) {
        if let Err(error) = self.recent_projects.remember(path) {
            self.status =
                format!("Project is open, but recent history could not be saved: {error}");
        }
    }
    pub(super) fn invoke_recent(&mut self, path: Option<PathBuf>, cx: &mut EventContext) {
        if !self.can_switch_projects() {
            return;
        }
        let result = match path {
            Some(path) => self.open_paths(vec![path], false),
            None => self.recent_projects.clear(),
        };
        self.operation_result(alerts::Operation::Open, result, cx);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_deduplicates_bounds_skips_missing_and_clears() {
        let directory = tempfile::tempdir().unwrap();
        let mut recent = RecentProjects::default();
        for i in 0..15 {
            recent
                .remember(directory.path().join(format!("{i}.comp")))
                .unwrap();
        }
        assert_eq!(recent.paths.len(), LIMIT);
        let path = recent.paths[3].clone();
        project::save(&Document::new(2, 2).unwrap(), &path).unwrap();
        recent.remember(path.clone()).unwrap();
        assert_eq!(recent.existing(), vec![path]);
        let history = directory.path().join("recent.json");
        write(&history, &recent.paths).unwrap();
        assert_eq!(read(&history).unwrap(), recent.paths);
        recent.clear().unwrap();
        assert!(recent.existing().is_empty());
    }
}
