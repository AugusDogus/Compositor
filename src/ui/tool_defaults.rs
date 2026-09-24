//! Application-wide toggles, distinct from each project's brush and tool drafts.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Toggles {
    auto_select: bool,
    transform_controls: bool,
    pixel_grid: bool,
    layout: compositor::guides::Settings,
}
impl Default for Toggles {
    fn default() -> Self {
        Self::capture(&project_tools::ProjectTools::default())
    }
}
impl Toggles {
    pub(super) fn capture(tools: &project_tools::ProjectTools) -> Self {
        Self {
            auto_select: tools.transform_auto_select,
            transform_controls: tools.show_transform_controls,
            pixel_grid: tools.pixel_grid,
            layout: tools.layout,
        }
    }
    pub(super) fn apply(self, tools: &mut project_tools::ProjectTools) {
        tools.transform_auto_select = self.auto_select;
        tools.show_transform_controls = self.transform_controls;
        tools.pixel_grid = self.pixel_grid;
        tools.layout = self.layout;
    }
    fn read(path: &Path) -> Result<Self> {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        Ok(serde_json::from_reader(file.take(16 * 1024))?)
    }
    fn save(self, path: &Path) -> Result<()> {
        let directory = path
            .parent()
            .ok_or_else(|| compositor::invalid("Tool preferences have no parent directory."))?;
        std::fs::create_dir_all(directory)?;
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer(&mut file, &self)?;
        file.write_all(b"\n")?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
}
#[derive(Default)]
pub(super) struct Preferences {
    previous: Toggles,
    path: Option<PathBuf>,
}
impl Editor {
    pub(crate) fn restore_tool_defaults(&mut self) {
        self.tool_defaults.path =
            update_preferences::path().map(|p| p.with_file_name("tool-defaults.json"));
        if let Some(path) = &self.tool_defaults.path {
            match Toggles::read(path) {
                Ok(toggles) => { toggles.apply(&mut self.tools); self.tool_defaults.previous = toggles; }
                Err(error) => self.report_startup_error(format!("Could not restore tool preferences: {error}. Default toggles remain active; changing a toggle will replace the invalid settings.")),
            }
        }
    }
    pub(super) fn save_tool_defaults(&mut self) {
        let current = Toggles::capture(&self.tools);
        if current == self.tool_defaults.previous {
            return;
        }
        self.tool_defaults.previous = current;
        if let Some(path) = &self.tool_defaults.path
            && let Err(error) = current.save(path)
        {
            self.status = format!(
                "Could not save tool preferences: {error}. Current settings remain active for this session. Check the configuration directory permissions and change a toggle to retry."
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toggles_persist_and_follow_tabs_without_moving_brush_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("toggles.json");
        let mut e = Editor::with_test_document();
        e.tool_defaults.path = Some(path.clone());
        e.tools.transform_auto_select = true;
        e.tools.pixel_grid = false;
        e.tools.layout.to_grid = true;
        e.tools.brush.diameter = 93.;
        e.save_tool_defaults();
        let saved = Toggles::read(&path).unwrap();
        assert_eq!(saved, Toggles::capture(&e.tools));
        e.add_empty_tab();
        assert_eq!(Toggles::capture(&e.tools), saved);
        assert_eq!(e.tools.brush.diameter, 40.);
        e.tools.show_transform_controls = false;
        e.activate_tab(0);
        assert!(!e.tools.show_transform_controls);
        assert_eq!(e.tools.brush.diameter, 93.);
        std::fs::write(&path, r#"{"pixel_grid":"no"}"#).unwrap();
        assert!(Toggles::read(&path).is_err());
    }
}
