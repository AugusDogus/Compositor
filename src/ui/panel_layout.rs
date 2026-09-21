use super::*;
use compositor::invalid;
use quickgui::{MouseButton, PointerPhase};
use std::{
    io::{Read, Write},
    path::Path,
};

pub(super) struct PanelLayout {
    pub width: f32,
    drag_start: Option<f32>,
    path: Option<PathBuf>,
}

impl Default for PanelLayout {
    fn default() -> Self {
        Self {
            width: 252.,
            drag_start: None,
            path: None,
        }
    }
}

fn preference_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .map(|p| p.join("compositor/layer-panel-width"))
}

fn read_width(path: &Path) -> Result<Option<f32>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut text = String::new();
    file.take(65).read_to_string(&mut text)?;
    let width = text
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|w| (202. ..=352.).contains(w));
    if text.len() > 64 || width.is_none() {
        return Err(invalid(
            "The saved panel width must be between 202 and 352 pixels.",
        ));
    }
    Ok(width)
}

fn save_width(path: &Path, width: f32) -> Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| invalid("The panel preference has no parent directory."))?;
    std::fs::create_dir_all(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    writeln!(file, "{width:.0}")?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

impl Editor {
    pub fn restore_layout(&mut self) {
        self.panel_layout.path = preference_path();
        if let Some(path) = &self.panel_layout.path {
            match read_width(path) {
                Ok(Some(width)) => self.panel_layout.width = width,
                Ok(None) => {}
                Err(error) => {
                    self.status = format!(
                        "Could not read panel width from {}: {error} Using 252 pixels; resize the panel to save a new width.",
                        path.display()
                    )
                }
            }
        }
    }

    pub(super) fn panel_resize_edge(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        div().id("panel-resize").absolute().left(-4.).top(0.).w(8.).h_full()
            .cursor(quickgui::CursorStyle::ResizeLeftRight)
            .on_pointer(cx.pointer_listener("panel-resize", |this, event, cx| {
                if this.pending || this.gesture.is_some() || this.modal.is_some() || event.button != MouseButton::Left { return; }
                match event.phase {
                    PointerPhase::Down => this.panel_layout.drag_start = Some(this.panel_layout.width),
                    PointerPhase::Move | PointerPhase::Up => {
                        if let Some(start) = this.panel_layout.drag_start {
                            this.panel_layout.width = (start - (event.position.x - event.origin.x)).round().clamp(202., 352.);
                            if event.phase == PointerPhase::Up {
                                this.panel_layout.drag_start = None;
                                if let Some(path) = &this.panel_layout.path && let Err(error) = save_width(path, this.panel_layout.width) {
                                    this.status = format!("Could not save panel width to {}: {error} The current layout and project are preserved; resize again to retry.", path.display());
                                }
                            }
                        }
                    }
                    PointerPhase::Cancel => {
                        if let Some(width) = this.panel_layout.drag_start.take() { this.panel_layout.width = width; }
                    }
                }
                cx.invalidate();
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_preferences_roundtrip_and_reject_invalid_or_oversized_values() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("compositor/layer-panel-width");
        assert_eq!(read_width(&path).unwrap(), None);
        save_width(&path, 310.).unwrap();
        assert_eq!(read_width(&path).unwrap(), Some(310.));
        for value in ["NaN", "inf", "201", "353", "broken", &"1".repeat(65)] {
            std::fs::write(&path, value).unwrap();
            assert!(read_width(&path).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), value);
        }
    }
}
