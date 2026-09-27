//! Persisted application appearance, independent of project content.
use super::*;
use std::{
    io::{Read, Write},
    path::Path,
    time::{Duration, Instant},
};
mod omarchy;
pub(super) mod palette;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum Choice {
    #[default]
    Dark,
    Light,
    Omarchy,
}
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Changed(pub palette::Palette);
impl Choice {
    fn text(self) -> &'static str {
        match self {
            Self::Dark => "dark\n",
            Self::Light => "light\n",
            Self::Omarchy => "omarchy\n",
        }
    }
    fn read(path: &Path) -> Result<Self> {
        let value = match read_text(path, 32) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Self::Dark),
            Err(error) => return Err(error.into()),
        };
        match value.as_str() {
            "dark\n" => Ok(Self::Dark),
            "light\n" => Ok(Self::Light),
            "omarchy\n" => Ok(Self::Omarchy),
            _ => Err(compositor::invalid(
                "The appearance preference is invalid. Choose a theme from View > Theme to replace it.",
            )),
        }
    }
    fn save(self, path: &Path) -> Result<()> {
        let dir = path.parent().ok_or_else(|| {
            compositor::invalid("The appearance preference has no parent directory.")
        })?;
        std::fs::create_dir_all(dir)?;
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        file.write_all(self.text().as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
}
// Configuration files can be symlinks, but must never block the UI/background
// pool on a FIFO or stream arbitrary data from a device.
fn read_text(path: &Path, limit: u64) -> std::io::Result<String> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "The appearance configuration is not a regular file.",
        ));
    }
    let mut text = String::new();
    file.take(limit + 1).read_to_string(&mut text)?;
    if text.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("The appearance configuration exceeds {limit} bytes."),
        ));
    }
    Ok(text)
}

pub(super) struct Appearance {
    pub choice: Choice,
    preference: Option<PathBuf>,
    source: Option<PathBuf>,
    next_check: Instant,
    running: bool,
    generation: u64,
    last_error: Option<String>,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            choice: Choice::Dark,
            preference: None,
            source: None,
            next_check: Instant::now(),
            running: false,
            generation: 0,
            last_error: None,
        }
    }
}
impl Editor {
    pub(crate) fn restore_theme(&mut self) {
        self.appearance.preference =
            update_preferences::path().map(|p| p.with_file_name("appearance"));
        self.appearance.source = omarchy::path();
        if let Some(path) = &self.appearance.preference {
            match Choice::read(path) {
                Ok(choice) => self.apply_theme(choice),
                Err(error) => self.report_startup_error(format!(
                    "Could not restore appearance: {error} The dark theme remains active."
                )),
            }
        }
    }
    fn apply_theme(&mut self, choice: Choice) {
        self.appearance.choice = choice;
        self.appearance.generation = self.appearance.generation.wrapping_add(1);
        self.appearance.next_check = Instant::now();
        self.appearance.last_error = None;
        self.colors = match choice {
            Choice::Light => palette::Palette::LIGHT,
            _ => palette::Palette::default(),
        };
    }
    pub(super) fn choose_theme(&mut self, choice: Choice, cx: &mut EventContext) {
        self.apply_theme(choice);
        if let Some(path) = &self.appearance.preference
            && let Err(error) = choice.save(path)
        {
            self.status = format!(
                "Could not save appearance: {error}. This theme remains active for this session. Check configuration directory permissions and select the theme again to retry."
            );
        }
        self.notify_theme_window(cx);
        self.changed(cx);
    }
    fn notify_theme_window(&self, cx: &mut EventContext) {
        if let Some(window) = self.about_window {
            cx.dispatch_action_to_window(window, Changed(self.colors));
        }
    }
    pub(super) fn monitor_theme(&mut self, cx: &mut ViewContext<'_, Self>) {
        if self.appearance.choice != Choice::Omarchy || self.appearance.running {
            return;
        }
        if Instant::now() < self.appearance.next_check {
            cx.request_repaint_at(self.appearance.next_check);
            return;
        }
        self.appearance.next_check = Instant::now() + Duration::from_secs(2);
        cx.request_repaint_at(self.appearance.next_check);
        let Some(path) = self.appearance.source.clone() else {
            self.theme_error("The Omarchy palette path is unavailable. Set HOME or XDG_STATE_HOME, or choose Dark or Light from View > Theme.".into());
            return;
        };
        let generation = self.appearance.generation;
        self.appearance.running = true;
        let result = cx.spawn_background(move || omarchy::read(&path), move |this, result, cx| {
            this.appearance.running = false;
            if this.appearance.generation != generation || this.appearance.choice != Choice::Omarchy { cx.invalidate(); return; }
            let result = match result {
                Ok(result) => result.map_err(|error| format!("Could not load the Omarchy palette: {error}. The last valid theme remains active. Fix colors.toml or choose Dark or Light from View > Theme.")),
                Err(error) => Err(format!("Could not check the Omarchy palette: {error}. The last valid theme remains active; the next check will retry.")),
            };
            this.receive_theme(generation, result);
            this.notify_theme_window(cx);
            cx.invalidate();
        });
        if let Err(error) = result {
            self.appearance.running = false;
            self.theme_error(format!("Could not schedule the Omarchy palette check: {error}. The last valid theme remains active; the next check will retry."));
        }
    }
    fn receive_theme(
        &mut self,
        generation: u64,
        result: std::result::Result<palette::Palette, String>,
    ) {
        if self.appearance.generation != generation || self.appearance.choice != Choice::Omarchy {
            return;
        }
        match result {
            Ok(colors) => {
                self.colors = colors;
                if self.appearance.last_error.as_ref() == Some(&self.status) {
                    self.status = "Omarchy theme updated".into();
                }
                self.appearance.last_error = None;
            }
            Err(message) => self.theme_error(message),
        }
    }
    fn theme_error(&mut self, message: String) {
        if self.appearance.last_error.as_ref() != Some(&message) {
            self.status = message.clone();
            self.appearance.last_error = Some(message);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_updates_keep_last_palette_and_late_updates_cannot_replace_a_new_choice() {
        let mut editor = Editor::with_test_document();
        editor.apply_theme(Choice::Omarchy);
        let generation = editor.appearance.generation;
        let palette = palette::Palette {
            background: [10, 20, 30],
            foreground: [230, 220, 210],
            accent: [200, 160, 40],
        };
        editor.receive_theme(generation, Ok(palette));
        assert_eq!(editor.colors, palette);
        editor.receive_theme(generation, Err("Invalid theme".into()));
        assert_eq!(editor.colors, palette);
        assert_eq!(editor.status, "Invalid theme");
        editor.status = "Another operation completed".into();
        editor.receive_theme(generation, Err("Invalid theme".into()));
        assert_eq!(editor.status, "Another operation completed");
        editor.apply_theme(Choice::Light);
        editor.receive_theme(generation, Ok(palette));
        editor.receive_theme(generation, Err("Stale error".into()));
        assert_eq!(editor.colors, palette::Palette::LIGHT);
        assert_eq!(editor.status, "Another operation completed");
        assert_eq!(
            Editor::with_test_document().colors,
            palette::Palette::default()
        );
    }

    #[test]
    fn configuration_reads_reject_pipes_devices_and_oversized_preferences() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("appearance");
        rustix::fs::mkfifoat(
            rustix::fs::CWD,
            &path,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
        .unwrap();
        assert!(Choice::read(&path).is_err());
        assert!(omarchy::read(&path).is_err());
        assert!(read_text(Path::new("/dev/zero"), 32).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, [b' '; 33]).unwrap();
        assert!(Choice::read(&path).is_err());
    }

    #[test]
    fn appearance_preferences_round_trip_and_reject_trailing_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("appearance");
        assert_eq!(Choice::read(&path).unwrap(), Choice::Dark);
        for choice in [Choice::Dark, Choice::Light, Choice::Omarchy] {
            choice.save(&path).unwrap();
            assert_eq!(Choice::read(&path).unwrap(), choice);
        }
        std::fs::write(&path, "light\njunk").unwrap();
        assert!(Choice::read(&path).is_err());
    }
}

pub(super) fn control(colors: palette::Palette, label: impl Into<Arc<str>>) -> Element {
    button()
        .flex_row()
        .items_center()
        .text_size(13.)
        .line_height(16.)
        .h(24.)
        .px(11.)
        .rounded(12.)
        .border(1., Color::TRANSPARENT)
        .bg(colors.neutral(49))
        .hover(|s| s.bg(colors.neutral(68)))
        .focus(controls::focus_outline)
        .child(text(label))
}

#[cfg(test)]
mod view_tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn theme_menu_changes_chrome_and_persists_without_editing_the_project() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("appearance");
        let mut editor = Editor::with_test_document();
        editor.appearance.preference = Some(path.clone());
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [180, 120, 80, 255],
            false,
            false,
        )
        .unwrap();
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .bind_keys(quickgui::menubar_key_bindings())
            .bind_keys(quickgui::popover_menu_key_bindings())
            .bind_keys(menus::key_bindings())
            .into_test_context(WindowOptions::new("Appearance").size(1280., 900.), editor)
            .unwrap();
        let window = view.window_handle();
        let dark = cx.capture_screenshot(window).unwrap();
        cx.simulate_keystrokes(window, "alt-v end right down enter")
            .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.appearance.choice, Choice::Light);
            assert_eq!(e.colors, palette::Palette::LIGHT);
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
        assert_eq!(Choice::read(&path).unwrap(), Choice::Light);
        let light = cx.capture_screenshot(window).unwrap();
        let scale = light.width() / 1280;
        assert!(dark.pixel(800 * scale, 15 * scale).unwrap()[0] < 80);
        assert!(light.pixel(800 * scale, 15 * scale).unwrap()[0] > 200);
        if let Ok(dir) = std::env::var("COMPOSITOR_THEME_SCREENSHOTS") {
            std::fs::create_dir_all(&dir).unwrap();
            dark.write_png(Path::new(&dir).join("theme-dark.png"))
                .unwrap();
            light
                .write_png(Path::new(&dir).join("theme-light.png"))
                .unwrap();
            cx.update(view, |e, cx| {
                e.open_filter(compositor::filters::Filter::PhotoFilter(Default::default()))
                    .unwrap();
                cx.invalidate();
            })
            .unwrap();
            cx.capture_screenshot(window)
                .unwrap()
                .write_png(Path::new(&dir).join("theme-light-dialog.png"))
                .unwrap();
        }
    }
}
