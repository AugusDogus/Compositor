use super::*;

#[allow(clippy::too_many_lines)]
pub(super) fn run_terminal_worker(
    options: TerminalOptions,
    mut size: TerminalSize,
    receiver: Receiver<WorkerMessage>,
    reader_messages: SyncSender<WorkerMessage>,
    shared: Arc<RwLock<Arc<TerminalSnapshot>>>,
    invalidator: WindowInvalidator,
    shutdown: Arc<AtomicBool>,
) {
    let pty_responses = Rc::new(RefCell::new(Vec::<u8>::new()));
    let mut terminal = match GhosttyTerminal::new(GhosttyTerminalOptions {
        cols: size.cols,
        rows: size.rows,
        max_scrollback: options.max_scrollback,
    }) {
        Ok(terminal) => terminal,
        Err(error) => {
            publish_failure(
                &shared,
                &invalidator,
                size,
                format!("libghostty-vt: {error}"),
            );
            return;
        }
    };
    let callback_responses = Rc::clone(&pty_responses);
    if let Err(error) = terminal.on_pty_write(move |_terminal, bytes| {
        callback_responses.borrow_mut().extend_from_slice(bytes);
    }) {
        publish_failure(
            &shared,
            &invalidator,
            size,
            format!("libghostty-vt: {error}"),
        );
        return;
    }
    let mut render_state = match RenderState::new() {
        Ok(state) => state,
        Err(error) => {
            publish_failure(
                &shared,
                &invalidator,
                size,
                format!("libghostty-vt: {error}"),
            );
            return;
        }
    };
    let mut rows = match RowIterator::new() {
        Ok(rows) => rows,
        Err(error) => {
            publish_failure(
                &shared,
                &invalidator,
                size,
                format!("libghostty-vt: {error}"),
            );
            return;
        }
    };
    let mut cells = match CellIterator::new() {
        Ok(cells) => cells,
        Err(error) => {
            publish_failure(
                &shared,
                &invalidator,
                size,
                format!("libghostty-vt: {error}"),
            );
            return;
        }
    };
    let mut key_encoder = match GhosttyKeyEncoder::new() {
        Ok(encoder) => encoder,
        Err(error) => {
            publish_failure(
                &shared,
                &invalidator,
                size,
                format!("libghostty-vt: {error}"),
            );
            return;
        }
    };
    let mut selection = match TerminalSelectionState::new() {
        Ok(selection) => selection,
        Err(error) => {
            publish_failure(
                &shared,
                &invalidator,
                size,
                format!("libghostty-vt: {error}"),
            );
            return;
        }
    };

    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(pty_size(size)) {
        Ok(pair) => pair,
        Err(error) => {
            publish_failure(
                &shared,
                &invalidator,
                size,
                format!("could not open PTY: {error}"),
            );
            return;
        }
    };
    let mut command = terminal_command(&options);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", TERM_PROGRAM_VALUE);
    command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    let mut child = match pair.slave.spawn_command(command) {
        Ok(child) => child,
        Err(error) => {
            publish_failure(
                &shared,
                &invalidator,
                size,
                format!("could not start terminal process: {error}"),
            );
            return;
        }
    };
    drop(pair.slave);
    let mut reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.kill();
            publish_failure(
                &shared,
                &invalidator,
                size,
                format!("could not read PTY: {error}"),
            );
            return;
        }
    };
    let mut writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(error) => {
            let _ = child.kill();
            publish_failure(
                &shared,
                &invalidator,
                size,
                format!("could not write PTY: {error}"),
            );
            return;
        }
    };
    let master = pair.master;
    thread::Builder::new()
        .name("quickgui-terminal-reader".to_owned())
        .spawn(move || {
            let mut buffer = vec![0; READ_CHUNK_BYTES];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(length) => {
                        if reader_messages
                            .send(WorkerMessage::Output(buffer[..length].to_vec()))
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            let _ = reader_messages.send(WorkerMessage::ReaderClosed);
        })
        .ok();

    let child_process_id = child.process_id();
    let mut revision = 0;
    let mut status = TerminalStatus::Running {
        process_id: child_process_id,
    };
    let mut detected_agent =
        child_process_id.and_then(crate::terminal_process::detect_agent_process);
    let mut last_process_probe = Instant::now();
    let mut last_output = Instant::now();
    let mut recently_active = detected_agent.is_some();
    publish_terminal_snapshot(
        &mut terminal,
        &mut render_state,
        &mut rows,
        &mut cells,
        &shared,
        &invalidator,
        TerminalSnapshotMetadata::next(
            &mut revision,
            status.clone(),
            detected_agent,
            recently_active,
        ),
    );

    let mut reader_closed = false;
    'worker: loop {
        if shutdown.load(Ordering::Acquire) {
            break;
        }
        let first = match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(message) => Some(message),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let mut redraw = false;
        let mut output_received = false;
        if let Some(message) = first {
            if !handle_worker_message(
                message,
                &mut terminal,
                &mut key_encoder,
                &mut selection,
                &pty_responses,
                &mut writer,
                master.as_ref(),
                &mut size,
                &mut child,
                &mut reader_closed,
                &mut redraw,
                &mut output_received,
            ) {
                break 'worker;
            }
            for _ in 1..WORKER_BATCH_LIMIT {
                match receiver.try_recv() {
                    Ok(message) => {
                        if !handle_worker_message(
                            message,
                            &mut terminal,
                            &mut key_encoder,
                            &mut selection,
                            &pty_responses,
                            &mut writer,
                            master.as_ref(),
                            &mut size,
                            &mut child,
                            &mut reader_closed,
                            &mut redraw,
                            &mut output_received,
                        ) {
                            break 'worker;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break 'worker,
                }
            }
        }

        let now = Instant::now();
        if output_received {
            last_output = now;
        }
        if now.duration_since(last_process_probe) >= AGENT_PROCESS_PROBE_INTERVAL {
            last_process_probe = now;
            let next = child_process_id.and_then(crate::terminal_process::detect_agent_process);
            if next != detected_agent {
                detected_agent = next;
                redraw = true;
            }
        }
        let next_recently_active =
            detected_agent.is_some() && now.duration_since(last_output) < AGENT_RECENT_ACTIVITY;
        if next_recently_active != recently_active {
            recently_active = next_recently_active;
            redraw = true;
        }
        if let Ok(Some(exit)) = child.try_wait() {
            status = TerminalStatus::Exited {
                exit_code: exit.exit_code(),
                signal: exit.signal().map(Arc::from),
            };
            redraw = true;
        }
        if redraw {
            publish_terminal_snapshot(
                &mut terminal,
                &mut render_state,
                &mut rows,
                &mut cells,
                &shared,
                &invalidator,
                TerminalSnapshotMetadata::next(
                    &mut revision,
                    status.clone(),
                    detected_agent,
                    recently_active,
                ),
            );
        }
        if reader_closed && matches!(status, TerminalStatus::Exited { .. }) {
            break;
        }
    }
    if !matches!(status, TerminalStatus::Exited { .. }) {
        let _ = child.kill();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_worker_message(
    message: WorkerMessage,
    terminal: &mut GhosttyTerminal<'_, '_>,
    key_encoder: &mut GhosttyKeyEncoder<'_>,
    selection: &mut TerminalSelectionState,
    pty_responses: &Rc<RefCell<Vec<u8>>>,
    writer: &mut Box<dyn Write + Send>,
    master: &dyn portable_pty::MasterPty,
    size: &mut TerminalSize,
    child: &mut Box<dyn portable_pty::Child + Send + Sync>,
    reader_closed: &mut bool,
    redraw: &mut bool,
    output_received: &mut bool,
) -> bool {
    match message {
        WorkerMessage::Output(bytes) => {
            terminal.vt_write(&bytes);
            let responses = std::mem::take(&mut *pty_responses.borrow_mut());
            if !responses.is_empty() {
                let _ = writer.write_all(&responses);
                let _ = writer.flush();
            }
            *redraw = true;
            *output_received = true;
        }
        WorkerMessage::ReaderClosed => *reader_closed = true,
        WorkerMessage::Input(bytes) => {
            selection.clear(terminal);
            let _ = writer.write_all(&bytes);
            let _ = writer.flush();
            *redraw = true;
        }
        WorkerMessage::Paste(value) => {
            if write_paste(terminal, writer, value) {
                selection.clear(terminal);
                terminal.scroll_viewport(ScrollViewport::Bottom);
                *redraw = true;
            }
        }
        WorkerMessage::Key(input) => {
            if write_key(terminal, key_encoder, writer, input) {
                selection.clear(terminal);
                terminal.scroll_viewport(ScrollViewport::Bottom);
                *redraw = true;
            }
        }
        WorkerMessage::Resize(next) => {
            if *size != next {
                *size = next;
                let _ = master.resize(pty_size(next));
                let _ = terminal.resize(
                    next.cols,
                    next.rows,
                    next.cell_width_px,
                    next.cell_height_px,
                );
                *redraw = true;
            }
        }
        WorkerMessage::Scroll(rows) => {
            terminal.scroll_viewport(ScrollViewport::Delta(rows));
            *redraw = true;
        }
        WorkerMessage::Selection(input) => {
            if selection.apply(terminal, input).is_ok() {
                *redraw = true;
            }
        }
        WorkerMessage::SelectAll => {
            if let Ok(selected) = terminal.select_all()
                && terminal.set_selection(selected.as_ref()).is_ok()
            {
                selection.gesture.reset(terminal);
                *redraw = true;
            }
        }
        WorkerMessage::Theme(theme) => {
            if apply_terminal_theme(terminal, theme.as_deref()).is_ok() {
                *redraw = true;
            }
        }
        WorkerMessage::Shutdown => {
            let _ = child.kill();
            return false;
        }
    }
    true
}

pub(super) fn apply_terminal_theme(
    terminal: &mut GhosttyTerminal<'_, '_>,
    theme: Option<&TerminalTheme>,
) -> Result<(), libghostty_vt::Error> {
    terminal
        .set_default_fg_color(theme.map(|theme| terminal_rgb(theme.foreground)))?
        .set_default_bg_color(theme.map(|theme| terminal_rgb(theme.background)))?
        .set_default_cursor_color(theme.map(|theme| terminal_rgb(theme.cursor)))?;
    if let Some(theme) = theme {
        let mut palette = terminal.default_color_palette()?;
        for (target, source) in palette.0.iter_mut().zip(theme.ansi) {
            *target = terminal_rgb(source);
        }
        terminal.set_default_color_palette(Some(palette))?;
    } else {
        terminal.set_default_color_palette(None)?;
    }
    Ok(())
}

pub(super) fn terminal_rgb(color: Color) -> RgbColor {
    let [r, g, b, _] = color.to_srgba8();
    RgbColor { r, g, b }
}

pub(super) fn terminal_command(options: &TerminalOptions) -> CommandBuilder {
    let mut command = options
        .program
        .as_ref()
        .map_or_else(CommandBuilder::new_default_prog, |program| {
            CommandBuilder::new(program)
        });
    command.args(&options.arguments);
    if let Some(directory) = &options.working_directory {
        command.cwd(directory);
    }
    for (key, value) in &options.environment {
        command.env(key, value);
    }
    command
}

pub(super) fn pty_size(size: TerminalSize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: size.cell_width_px.min(u16::MAX as u32) as u16,
        pixel_height: size.cell_height_px.min(u16::MAX as u32) as u16,
    }
}

pub(super) fn write_paste(
    terminal: &GhosttyTerminal<'_, '_>,
    writer: &mut Box<dyn Write + Send>,
    value: String,
) -> bool {
    let bracketed = terminal.mode(Mode::BRACKETED_PASTE).unwrap_or(false);
    let mut input = value.into_bytes();
    let mut encoded = vec![0; input.len().saturating_add(16)];
    let length = match libghostty_vt::paste::encode(&mut input, bracketed, &mut encoded) {
        Ok(length) => length,
        Err(libghostty_vt::Error::OutOfSpace { required })
            if required <= MAX_TERMINAL_INPUT_BYTES + 16 =>
        {
            encoded.resize(required, 0);
            match libghostty_vt::paste::encode(&mut input, bracketed, &mut encoded) {
                Ok(length) => length,
                Err(_) => return false,
            }
        }
        Err(_) => return false,
    };
    let _ = writer.write_all(&encoded[..length]);
    let _ = writer.flush();
    true
}

pub(super) fn write_key(
    terminal: &GhosttyTerminal<'_, '_>,
    encoder: &mut GhosttyKeyEncoder<'_>,
    writer: &mut Box<dyn Write + Send>,
    input: TerminalKeyInput,
) -> bool {
    let Some((key, text, unshifted)) =
        ghostty_key(&input.key, input.key_char.as_ref(), input.text.as_deref())
    else {
        return false;
    };
    let mut event = match GhosttyKeyEvent::new() {
        Ok(event) => event,
        Err(_) => return false,
    };
    event
        .set_action(input.action)
        .set_key(key)
        .set_mods(ghostty_modifiers(input.modifiers))
        .set_consumed_mods(GhosttyMods::empty())
        .set_composing(false)
        .set_utf8(text);
    if let Some(unshifted) = unshifted {
        event.set_unshifted_codepoint(unshifted);
    }
    encoder
        .set_options_from_terminal(terminal)
        .set_macos_option_as_alt(OptionAsAlt::True);
    let mut bytes = Vec::with_capacity(16);
    if encoder.encode_to_vec(&event, &mut bytes).is_ok() && !bytes.is_empty() {
        let _ = writer.write_all(&bytes);
        let _ = writer.flush();
        true
    } else {
        false
    }
}

pub(super) fn ghostty_modifiers(modifiers: Modifiers) -> GhosttyMods {
    let mut answer = GhosttyMods::empty();
    if modifiers.contains(Modifiers::SHIFT) {
        answer |= GhosttyMods::SHIFT;
    }
    if modifiers.contains(Modifiers::CONTROL) {
        answer |= GhosttyMods::CTRL;
    }
    if modifiers.contains(Modifiers::ALT) {
        answer |= GhosttyMods::ALT;
    }
    if modifiers.contains(Modifiers::SUPER) {
        answer |= GhosttyMods::SUPER;
    }
    answer
}

pub(super) fn ghostty_key(
    key: &Key,
    key_char: Option<&Key>,
    composed_text: Option<&str>,
) -> Option<(GhosttyKey, Option<String>, Option<char>)> {
    let physical_character = key_char
        .and_then(key_character)
        .or_else(|| key_character(key));
    let composed_text = composed_text
        .filter(|value| !value.is_empty())
        .filter(|value| value.chars().all(|character| !character.is_control()));
    let character = composed_text.or(physical_character);
    let answer = match key {
        Key::Character(_) => character
            .and_then(|value| value.chars().next())
            .map(ghostty_character_key)
            .unwrap_or(GhosttyKey::Unidentified),
        Key::ArrowUp => GhosttyKey::ArrowUp,
        Key::ArrowDown => GhosttyKey::ArrowDown,
        Key::ArrowLeft => GhosttyKey::ArrowLeft,
        Key::ArrowRight => GhosttyKey::ArrowRight,
        Key::PageUp => GhosttyKey::PageUp,
        Key::PageDown => GhosttyKey::PageDown,
        Key::Home => GhosttyKey::Home,
        Key::End => GhosttyKey::End,
        Key::Enter => GhosttyKey::Enter,
        Key::Escape => GhosttyKey::Escape,
        Key::Space => GhosttyKey::Space,
        Key::Tab => GhosttyKey::Tab,
        Key::Backspace => GhosttyKey::Backspace,
        Key::Delete => GhosttyKey::Delete,
        Key::Insert => GhosttyKey::Insert,
        Key::Function(value) => ghostty_function_key(*value),
        Key::Other => GhosttyKey::Unidentified,
    };
    let text = match key {
        Key::Enter
        | Key::Escape
        | Key::Tab
        | Key::Backspace
        | Key::Delete
        | Key::Insert
        | Key::ArrowUp
        | Key::ArrowDown
        | Key::ArrowLeft
        | Key::ArrowRight
        | Key::PageUp
        | Key::PageDown
        | Key::Home
        | Key::End
        | Key::Function(_)
        | Key::Other => None,
        Key::Space => Some(" ".to_owned()),
        Key::Character(_) => character.map(str::to_owned),
    };
    let unshifted = physical_character
        .and_then(|value| value.chars().next())
        .map(unshift_character);
    Some((answer, text, unshifted))
}

pub(super) fn ghostty_character_key(value: char) -> GhosttyKey {
    match value.to_ascii_lowercase() {
        'a' => GhosttyKey::A,
        'b' => GhosttyKey::B,
        'c' => GhosttyKey::C,
        'd' => GhosttyKey::D,
        'e' => GhosttyKey::E,
        'f' => GhosttyKey::F,
        'g' => GhosttyKey::G,
        'h' => GhosttyKey::H,
        'i' => GhosttyKey::I,
        'j' => GhosttyKey::J,
        'k' => GhosttyKey::K,
        'l' => GhosttyKey::L,
        'm' => GhosttyKey::M,
        'n' => GhosttyKey::N,
        'o' => GhosttyKey::O,
        'p' => GhosttyKey::P,
        'q' => GhosttyKey::Q,
        'r' => GhosttyKey::R,
        's' => GhosttyKey::S,
        't' => GhosttyKey::T,
        'u' => GhosttyKey::U,
        'v' => GhosttyKey::V,
        'w' => GhosttyKey::W,
        'x' => GhosttyKey::X,
        'y' => GhosttyKey::Y,
        'z' => GhosttyKey::Z,
        '0' | ')' => GhosttyKey::Digit0,
        '1' | '!' => GhosttyKey::Digit1,
        '2' | '@' => GhosttyKey::Digit2,
        '3' | '#' => GhosttyKey::Digit3,
        '4' | '$' => GhosttyKey::Digit4,
        '5' | '%' => GhosttyKey::Digit5,
        '6' | '^' => GhosttyKey::Digit6,
        '7' | '&' => GhosttyKey::Digit7,
        '8' | '*' => GhosttyKey::Digit8,
        '9' | '(' => GhosttyKey::Digit9,
        '`' | '~' => GhosttyKey::Backquote,
        '\\' | '|' => GhosttyKey::Backslash,
        '[' | '{' => GhosttyKey::BracketLeft,
        ']' | '}' => GhosttyKey::BracketRight,
        ',' | '<' => GhosttyKey::Comma,
        '=' | '+' => GhosttyKey::Equal,
        '-' | '_' => GhosttyKey::Minus,
        '.' | '>' => GhosttyKey::Period,
        '\'' | '"' => GhosttyKey::Quote,
        ';' | ':' => GhosttyKey::Semicolon,
        '/' | '?' => GhosttyKey::Slash,
        _ => GhosttyKey::Unidentified,
    }
}

pub(super) fn unshift_character(value: char) -> char {
    match value {
        ')' => '0',
        '!' => '1',
        '@' => '2',
        '#' => '3',
        '$' => '4',
        '%' => '5',
        '^' => '6',
        '&' => '7',
        '*' => '8',
        '(' => '9',
        '~' => '`',
        '|' => '\\',
        '{' => '[',
        '}' => ']',
        '<' => ',',
        '+' => '=',
        '_' => '-',
        '>' => '.',
        '"' => '\'',
        ':' => ';',
        '?' => '/',
        value => value.to_ascii_lowercase(),
    }
}

pub(super) fn ghostty_function_key(value: u8) -> GhosttyKey {
    match value {
        1 => GhosttyKey::F1,
        2 => GhosttyKey::F2,
        3 => GhosttyKey::F3,
        4 => GhosttyKey::F4,
        5 => GhosttyKey::F5,
        6 => GhosttyKey::F6,
        7 => GhosttyKey::F7,
        8 => GhosttyKey::F8,
        9 => GhosttyKey::F9,
        10 => GhosttyKey::F10,
        11 => GhosttyKey::F11,
        12 => GhosttyKey::F12,
        13 => GhosttyKey::F13,
        14 => GhosttyKey::F14,
        15 => GhosttyKey::F15,
        16 => GhosttyKey::F16,
        17 => GhosttyKey::F17,
        18 => GhosttyKey::F18,
        19 => GhosttyKey::F19,
        20 => GhosttyKey::F20,
        21 => GhosttyKey::F21,
        22 => GhosttyKey::F22,
        23 => GhosttyKey::F23,
        24 => GhosttyKey::F24,
        _ => GhosttyKey::Unidentified,
    }
}
