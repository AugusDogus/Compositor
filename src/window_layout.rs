use compositor::{Result, invalid};
use quickgui::{Color, Displays, WindowOptions, WindowRestoreState};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

pub(crate) fn preference_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(".config"))
        })
        .map(|path| path.join("compositor/window.json"))
}

pub(crate) fn read(path: &Path) -> Result<Option<WindowRestoreState>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(invalid("The saved window settings exceed 4096 bytes."));
    }
    let state: WindowRestoreState = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("The saved window settings are invalid: {error}.")))?;
    if !state.is_valid() {
        return Err(invalid(
            "The saved window position, size, or display scale is invalid.",
        ));
    }
    Ok(Some(state))
}

pub(crate) fn options(state: Option<&WindowRestoreState>, displays: &Displays) -> WindowOptions {
    let options = WindowOptions::new("Compositor")
        .size(1180., 780.)
        .decorations(false)
        // ContentView's 800 × 520 content minimum, plus our in-window menu
        // (28 points) and title toolbar (46 points), which AppKit hosts outside it.
        .minimum_size(800., 520. + 28. + 46.)
        .maximized(true)
        .background(Color::rgb8(30, 30, 30));
    match state {
        Some(state) => options.restore(state, displays),
        None => options,
    }
}

pub(crate) fn save(path: &Path, state: &WindowRestoreState) -> Result<()> {
    if !state.is_valid() {
        return Err(invalid(
            "The current window geometry cannot be saved. The previous window settings are preserved.",
        ));
    }
    let directory = path
        .parent()
        .ok_or_else(|| invalid("The window preference has no parent directory."))?;
    std::fs::create_dir_all(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer(&mut file, state)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Display, DisplayId, Rect, WindowBounds};

    #[test]
    fn restores_windowed_geometry_and_display_modes_after_saving() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("compositor/window.json");
        assert!(read(&path).unwrap().is_none());
        let displays = Displays::new(
            vec![
                Display::new(
                    DisplayId::new(1),
                    "Screen",
                    Rect::new(0., 0., 1920., 1080.),
                    Rect::new(0., 0., 1920., 1040.),
                    1.,
                )
                .unwrap(),
            ],
            Some(DisplayId::new(1)),
        )
        .unwrap();
        assert!(matches!(
            options(None, &displays).window_bounds,
            Some(WindowBounds::Maximized(_))
        ));
        for mode in 0..3 {
            let mut state = WindowRestoreState::new(Rect::new(120., 80., 1100., 700.));
            state.maximized = mode == 1;
            state.fullscreen = mode == 2;
            save(&path, &state).unwrap();
            let restored = read(&path).unwrap().unwrap();
            assert_eq!(restored, state);
            let bounds = options(Some(&restored), &displays).window_bounds.unwrap();
            assert_eq!(bounds.bounds(), state.bounds());
            assert_eq!(matches!(bounds, WindowBounds::Maximized(_)), mode == 1);
            assert_eq!(matches!(bounds, WindowBounds::Fullscreen(_)), mode == 2);
        }
        let offscreen = WindowRestoreState::new(Rect::new(9000., 9000., 1100., 700.));
        let bounds = options(Some(&offscreen), &displays)
            .window_bounds
            .unwrap()
            .bounds();
        assert!(bounds.x >= 0. && bounds.x + bounds.width <= 1920.);
        assert!(bounds.y >= 0. && bounds.y + bounds.height <= 1040.);
    }

    #[test]
    fn invalid_settings_and_failed_writes_preserve_the_previous_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("window.json");
        let state = WindowRestoreState::new(Rect::new(10., 20., 1100., 700.));
        save(&path, &state).unwrap();
        let mut invalid_state = state;
        invalid_state.width = f32::NAN;
        assert!(save(&path, &invalid_state).is_err());
        assert_eq!(read(&path).unwrap(), Some(state));
        invalid_state.width = -1.;
        for bytes in [
            b"broken".to_vec(),
            vec![b' '; 4097],
            serde_json::to_vec(&invalid_state).unwrap(),
        ] {
            std::fs::write(&path, &bytes).unwrap();
            assert!(read(&path).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }
}
