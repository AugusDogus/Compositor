mod launch;
mod ui;
mod window_layout;

use quickgui::{AppInfo, AppRunStatus, Application};

const UI_FONT: &[u8] = include_bytes!("../assets/fonts/InterVariable.ttf");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let result = run();
    compositor::object_selection::shutdown();
    compositor::background::shutdown();
    result
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<_> = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .collect();
    let queue = launch::LaunchQueue::default();
    let incoming = queue.clone();
    let mut app = Application::new()
        .font(UI_FONT)
        .bind_keys(quickgui::select_key_bindings())
        .bind_keys(quickgui::menubar_key_bindings())
        .bind_keys(quickgui::popover_menu_key_bindings())
        .bind_keys(ui::menus::key_bindings())
        .app_info(AppInfo::new(
            "Compositor",
            env!("CARGO_PKG_VERSION"),
            "compositor",
        )?)
        .on_second_instance(move |event, cx| {
            if let Err(error) = incoming.push(launch::forwarded_paths(
                event.argv().iter().map(|arg| arg.as_ref()),
                event.cwd(),
            )) {
                eprintln!("{error}");
            }
            if let Some(window) = cx.windows().first().copied() {
                if let Err(error) = cx.restore_window_handle(window) {
                    eprintln!(
                        "Could not restore the editor window: {error}. Its edits remain open."
                    );
                }
                cx.focus_window(window);
                cx.invalidate_window(window);
            }
        })
        .into_runner()?;
    // QuickGUI's forwarding protocol represents arguments as UTF-8. Preserve
    // arbitrary Linux filenames by opening a separate window for such launches.
    let can_forward = paths.iter().all(|path| path.to_str().is_some())
        && std::env::current_dir()?.to_str().is_some();
    if can_forward && !app.request_single_instance_lock(launch::instance_identifier())? {
        return Ok(());
    }
    queue.push(paths)?;
    // Compile the compute shader while the native window and workspace initialize.
    if let Err(error) = std::thread::Builder::new()
        .name("graphics-init".into())
        .spawn(|| {
            // Each pipeline reports CPU fallback once if initialization fails.
            let _ = compositor::brush::initialize_gpu();
            let _ = compositor::render::initialize_gpu();
        })
    {
        eprintln!(
            "Could not start graphics warmup: {error}. Painting and previews will initialize on demand."
        );
    }
    let mut editor = ui::Editor::new(Vec::new())?;
    editor.set_launch_queue(queue);
    editor.restore_layout();
    editor.restore_tool_defaults();
    editor.restore_update_preferences();
    editor.restore_shortcuts();
    match compositor::native_clipboard::wayland::WaylandClipboard::new(app.owned_display_handle()) {
        Ok(Some(clipboard)) => {
            app.set_clipboard_provider(clipboard.clone());
            editor.set_wayland_clipboard(clipboard);
        }
        Ok(None) => {}
        Err(error) => editor.report_startup_error(format!(
            "Could not connect the window clipboard: {error} The fallback clipboard remains available where supported; files can still be imported."
        )),
    }
    let layout_path = window_layout::preference_path();
    let mut layout = None;
    if let Some(path) = &layout_path {
        match window_layout::read(path) {
            Ok(saved) => layout = saved,
            Err(error) => editor.report_startup_error(format!(
                "Could not restore window settings from {}: {error} Using the default window size; closing the editor will save new settings.", path.display()
            )),
        }
    }
    let window = app.open_window(
        window_layout::options(layout.as_ref(), &app.displays()),
        editor,
    )?;
    loop {
        // Capture before pumping: an accepted close removes the native window.
        if let Some(state) = app.window_state(window) {
            let current = state.restore_state(&app.displays());
            if current.is_valid() && !state.minimized {
                layout = Some(current);
            }
        }
        match app.pump(None)? {
            AppRunStatus::Continue => {}
            AppRunStatus::Exited(0) => break,
            AppRunStatus::Exited(code) => {
                return Err(std::io::Error::other(format!(
                    "The editor exited with status {code}."
                ))
                .into());
            }
        }
    }
    if let (Some(path), Some(state)) = (&layout_path, &layout)
        && let Err(error) = window_layout::save(path, state)
    {
        eprintln!(
            "Could not save window settings to {}: {error} Project saves are unaffected; check the configuration directory permissions.",
            path.display()
        );
    }
    Ok(())
}
