use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct CellVisual {
    pub(super) foreground: Option<Color>,
    pub(super) background: Option<Color>,
    pub(super) bold: bool,
    pub(super) italic: bool,
    pub(super) underline: Option<Underline>,
    pub(super) strikethrough: bool,
}

impl CellVisual {
    pub(super) fn is_empty(self) -> bool {
        self == Self::default()
    }

    pub(super) fn highlight(self) -> HighlightStyle {
        let mut style = HighlightStyle::default();
        if let Some(color) = self.foreground {
            style = style.color(color);
        }
        if let Some(color) = self.background {
            style = style.background(color);
        }
        if self.bold {
            style = style.font_bold();
        }
        if self.italic {
            style = style.italic();
        }
        style = match self.underline {
            Some(Underline::Double) => style.double_underline(),
            Some(Underline::Curly) => style.text_decoration_wavy(),
            Some(Underline::Single | Underline::Dotted | Underline::Dashed) => style.underline(),
            Some(Underline::None) | None => style,
            Some(_) => style.underline(),
        };
        if self.strikethrough {
            style = style.strikethrough();
        }
        style
    }
}

pub(super) struct TerminalSnapshotMetadata {
    revision: u64,
    status: TerminalStatus,
    detected_agent: Option<DetectedAgentProcess>,
    recently_active: bool,
}

impl TerminalSnapshotMetadata {
    pub(super) fn next(
        revision: &mut u64,
        status: TerminalStatus,
        detected_agent: Option<DetectedAgentProcess>,
        recently_active: bool,
    ) -> Self {
        *revision = revision.wrapping_add(1);
        Self {
            revision: *revision,
            status,
            detected_agent,
            recently_active,
        }
    }

    #[cfg(test)]
    pub(super) fn running(revision: u64) -> Self {
        Self {
            revision,
            status: TerminalStatus::Running { process_id: None },
            detected_agent: None,
            recently_active: false,
        }
    }
}

pub(super) fn publish_terminal_snapshot<'alloc: 'cb, 'cb>(
    terminal: &mut GhosttyTerminal<'alloc, 'cb>,
    render_state: &mut RenderState<'alloc>,
    rows: &mut RowIterator<'alloc>,
    cells: &mut CellIterator<'alloc>,
    shared: &RwLock<Arc<TerminalSnapshot>>,
    invalidator: &WindowInvalidator,
    metadata: TerminalSnapshotMetadata,
) {
    match build_snapshot(terminal, render_state, rows, cells, metadata) {
        Ok(snapshot) => publish(shared, invalidator, snapshot),
        Err(error) => publish_failure(
            shared,
            invalidator,
            TerminalSize {
                cols: DEFAULT_COLS,
                rows: DEFAULT_ROWS,
                cell_width_px: 0,
                cell_height_px: 0,
            },
            format!("could not render libghostty-vt state: {error}"),
        ),
    }
}

pub(super) fn build_snapshot<'alloc: 'cb, 'cb>(
    terminal: &GhosttyTerminal<'alloc, 'cb>,
    render_state: &mut RenderState<'alloc>,
    rows: &mut RowIterator<'alloc>,
    cells: &mut CellIterator<'alloc>,
    metadata: TerminalSnapshotMetadata,
) -> Result<TerminalSnapshot, libghostty_vt::Error> {
    let TerminalSnapshotMetadata {
        revision,
        status,
        detected_agent,
        recently_active,
    } = metadata;
    let snapshot = render_state.update(terminal)?;
    let cols = snapshot.cols()?;
    let row_count = snapshot.rows()?;
    let scrollbar = terminal.scrollbar()?;
    let colors = snapshot.colors()?;
    let cursor_position = if snapshot.cursor_visible()? {
        snapshot.cursor_viewport()?
    } else {
        None
    };
    let foreground = ghostty_color(colors.foreground);
    let background = ghostty_color(colors.background);
    let selected_text = terminal
        .format_selection_alloc(
            None,
            GhosttySelectionFormatOptions::new()
                .with_emit_format(GhosttyFormat::Plain)
                .with_unwrap(true)
                .with_trim(true),
        )
        .ok()
        .flatten()
        .map(|bytes| Arc::<str>::from(String::from_utf8_lossy(bytes.as_ref()).into_owned()));
    let cursor_style = snapshot.cursor_visual_style()?;
    let cursor_blinking = snapshot.cursor_blinking()?;
    let cursor = cursor_position.map(|position| TerminalCursor {
        column: position.x,
        row: position.y,
        style: match cursor_style {
            CursorVisualStyle::Bar => TerminalCursorStyle::Bar,
            CursorVisualStyle::Block => TerminalCursorStyle::Block,
            CursorVisualStyle::Underline => TerminalCursorStyle::Underline,
            CursorVisualStyle::BlockHollow => TerminalCursorStyle::BlockHollow,
            _ => TerminalCursorStyle::Block,
        },
        blinking: cursor_blinking,
        color: ghostty_color(colors.cursor.unwrap_or(colors.foreground)),
    });
    let mut cursor_range = None;
    let mut content =
        String::with_capacity(cols as usize * row_count as usize + row_count as usize);
    let mut highlights = Vec::new();
    let mut graphics = Vec::new();
    let mut top_backgrounds = vec![None; usize::from(cols)];
    let mut right_backgrounds = vec![None; usize::from(row_count)];
    let mut bottom_backgrounds = vec![None; usize::from(cols)];
    let mut left_backgrounds = vec![None; usize::from(row_count)];
    let mut graphemes = Vec::new();
    let mut active_run: Option<(usize, usize, CellVisual)> = None;
    let mut row_iterator = rows.update(&snapshot)?;
    let mut row_index = 0_u16;
    while let Some(row) = row_iterator.next() {
        let row_selection = row.selection()?;
        let row_start = content.len();
        let mut row_visible_end = row_start;
        let mut cell_iterator = cells.update(row)?;
        let mut column = 0_u16;
        let mut grid_column = 0_u16;
        while let Some(cell) = cell_iterator.next() {
            let raw = cell.raw_cell()?;
            let wide = raw.wide()?;
            let style = cell.style()?;
            let cell_start_column = grid_column;
            let cell_end_column = if wide == CellWide::Wide {
                grid_column.saturating_add(1)
            } else {
                grid_column
            };
            let selected = row_selection.is_some_and(|selection| {
                selection.start_x <= cell_end_column && selection.end_x >= cell_start_column
            });
            let mut resolved_foreground = cell.fg_color()?.map(ghostty_color).unwrap_or(foreground);
            let mut resolved_background = cell.bg_color()?.map(ghostty_color).unwrap_or(background);
            if style.inverse {
                std::mem::swap(&mut resolved_foreground, &mut resolved_background);
            }
            let edge_background =
                (resolved_background != background).then_some(resolved_background);
            let edge_column = usize::from(grid_column);
            let edge_row = usize::from(row_index);
            if edge_column < usize::from(cols) {
                if edge_row == 0 {
                    top_backgrounds[edge_column] = edge_background;
                }
                if edge_row + 1 == usize::from(row_count) {
                    bottom_backgrounds[edge_column] = edge_background;
                }
            }
            if edge_row < usize::from(row_count) {
                if edge_column == 0 {
                    left_backgrounds[edge_row] = edge_background;
                }
                if edge_column + 1 == usize::from(cols) {
                    right_backgrounds[edge_row] = edge_background;
                }
            }
            grid_column = grid_column.saturating_add(1);
            if wide == CellWide::SpacerTail {
                column = column.saturating_add(1);
                continue;
            }
            let start = content.len();
            let mut has_visible_text = false;
            let mut graphic_character = None;
            if raw.has_text()? {
                let grapheme_count = cell.graphemes_len()?;
                graphemes.resize(grapheme_count, '\0');
                cell.graphemes_buf(&mut graphemes)?;
                has_visible_text = graphemes.iter().any(|value| !value.is_whitespace());
                graphic_character = (graphemes.len() == 1)
                    .then_some(graphemes[0])
                    .filter(|character| is_block_element(*character));
                content.extend(graphemes.iter().copied());
            } else {
                content.push(' ');
                if wide == CellWide::Wide {
                    content.push(' ');
                }
            }
            let end = content.len();
            if style.invisible {
                resolved_foreground = resolved_background;
            } else if style.faint {
                resolved_foreground = resolved_foreground.with_alpha(0.65);
            }
            if selected {
                resolved_background = static_selection_color();
            }
            let cursor_here = cursor_position.is_some_and(|cursor| {
                cursor.y == row_index
                    && (cursor.x == column
                        || (cursor.at_wide_tail
                            && wide == CellWide::Wide
                            && cursor.x == column.saturating_add(1)))
            });
            if let Some(character) = graphic_character {
                graphics.push(TerminalCellGraphic {
                    column,
                    row: row_index,
                    character,
                    foreground: (resolved_foreground != foreground).then_some(resolved_foreground),
                });
            }
            let visual = CellVisual {
                foreground: if graphic_character.is_some() {
                    Some(Color::TRANSPARENT)
                } else {
                    (resolved_foreground != foreground).then_some(resolved_foreground)
                },
                background: (resolved_background != background).then_some(resolved_background),
                bold: style.bold,
                italic: style.italic,
                underline: (style.underline != Underline::None).then_some(style.underline),
                strikethrough: style.strikethrough,
            };
            push_visual_run(&mut active_run, &mut highlights, start, end, visual);
            if cursor_here {
                cursor_range = Some(start..end);
            }
            if has_visible_text || cursor_here || visual.has_visible_blank_paint() {
                row_visible_end = end;
            }
            column = column.saturating_add(match wide {
                CellWide::Wide => 2,
                _ => 1,
            });
        }
        flush_visual_run(&mut active_run, &mut highlights);
        trim_terminal_row(&mut content, &mut highlights, row_visible_end);
        row_index = row_index.saturating_add(1);
        if row_index < row_count {
            content.push('\n');
        }
    }
    flush_visual_run(&mut active_run, &mut highlights);
    while content.ends_with('\n') {
        content.pop();
    }
    trim_terminal_highlights(&mut highlights, content.len());
    let title = terminal.title().unwrap_or_default();
    let agent = detected_agent.map(|detected| TerminalAgent {
        kind: Arc::from(detected.kind),
        status: detect_agent_status(detected.kind, &content, title, recently_active),
        process_id: detected.process_id,
    });
    Ok(TerminalSnapshot {
        revision,
        cols,
        rows: row_count,
        content: Arc::from(content),
        highlights: highlights.into(),
        foreground,
        background,
        title: Arc::from(title),
        working_directory: Arc::from(normalize_terminal_working_directory(
            terminal.pwd().unwrap_or_default(),
        )),
        scroll: TerminalScrollState {
            total_rows: scrollbar.total,
            offset_rows: scrollbar.offset,
            viewport_rows: scrollbar.len,
        },
        cursor,
        selected_text,
        cursor_range,
        graphics: graphics.into(),
        edge_backgrounds: EdgeBackgrounds::new(
            top_backgrounds,
            right_backgrounds,
            bottom_backgrounds,
            left_backgrounds,
        ),
        status,
        agent,
    })
}

/// libghostty exposes OSC 7 working directories exactly as emitted by the child. Shells commonly
/// use a `file://host/path` URI there, while the public terminal status contract is a filesystem
/// path. Keep ordinary paths untouched and decode the path portion of local file URIs.
pub(super) fn normalize_terminal_working_directory(value: &str) -> String {
    let Some(uri) = value.strip_prefix("file://") else {
        return value.to_owned();
    };
    let path = if uri.starts_with('/') {
        uri
    } else {
        uri.find('/').map_or("", |separator| &uri[separator..])
    };
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2]))
        {
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

const fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

pub(super) fn detect_agent_status(
    kind: &str,
    content: &str,
    title: &str,
    recently_active: bool,
) -> TerminalAgentStatus {
    let tail = content
        .lines()
        .rev()
        .take(16)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    let title = title.to_ascii_lowercase();
    let blocked = [
        "action required",
        "permission required",
        "do you want to proceed?",
        "do you trust the contents of this directory?",
        "allow command?",
        "press enter to confirm or esc to cancel",
        "enter to submit answer",
        "waiting for permission",
        "do you want to allow this connection?",
    ]
    .iter()
    .any(|marker| tail.contains(marker) || title.contains(marker));
    if blocked {
        return TerminalAgentStatus::Blocked;
    }

    let working = [
        "esc to interrupt",
        "ctrl+c to interrupt",
        "press esc to interrupt",
        "shells ·",
        "mcp tasks still running",
        "waiting for background agent",
    ]
    .iter()
    .any(|marker| tail.contains(marker) || title.contains(marker))
        || title.chars().next().is_some_and(|character| {
            matches!(
                character,
                '⠋' | '⠙'
                    | '⠹'
                    | '⠸'
                    | '⠼'
                    | '⠴'
                    | '⠦'
                    | '⠧'
                    | '⠇'
                    | '⠏'
                    | '◐'
                    | '◓'
                    | '◑'
                    | '◒'
            )
        });
    if working || recently_active {
        TerminalAgentStatus::Working
    } else {
        let _ = kind;
        TerminalAgentStatus::Idle
    }
}

impl CellVisual {
    pub(super) fn has_visible_blank_paint(self) -> bool {
        self.background.is_some() || self.underline.is_some() || self.strikethrough
    }
}

pub(super) fn trim_terminal_row(
    content: &mut String,
    highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
    visible_end: usize,
) {
    content.truncate(visible_end);
    trim_terminal_highlights(highlights, visible_end);
}

pub(super) fn trim_terminal_highlights(
    highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
    content_len: usize,
) {
    while highlights
        .last()
        .is_some_and(|(range, _)| range.start >= content_len)
    {
        highlights.pop();
    }
    if let Some((range, _)) = highlights.last_mut() {
        range.end = range.end.min(content_len);
    }
}

pub(super) fn push_visual_run(
    active: &mut Option<(usize, usize, CellVisual)>,
    highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
    start: usize,
    end: usize,
    visual: CellVisual,
) {
    if start == end || visual.is_empty() {
        flush_visual_run(active, highlights);
        return;
    }
    if let Some((_, run_end, current)) = active
        && *run_end == start
        && *current == visual
    {
        *run_end = end;
        return;
    }
    flush_visual_run(active, highlights);
    *active = Some((start, end, visual));
}

pub(super) fn flush_visual_run(
    active: &mut Option<(usize, usize, CellVisual)>,
    highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
) {
    let Some((start, end, visual)) = active.take() else {
        return;
    };
    if highlights.len() < MAX_TEXT_HIGHLIGHTS {
        highlights.push((start..end, visual.highlight()));
    }
}

pub(super) fn publish(
    shared: &RwLock<Arc<TerminalSnapshot>>,
    invalidator: &WindowInvalidator,
    snapshot: TerminalSnapshot,
) {
    *shared
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(snapshot);
    let _ = invalidator.invalidate();
}

pub(super) fn publish_failure(
    shared: &RwLock<Arc<TerminalSnapshot>>,
    invalidator: &WindowInvalidator,
    size: TerminalSize,
    message: String,
) {
    let content = Arc::<str>::from(format!("QuickGUI terminal error\n\n{message}"));
    publish(
        shared,
        invalidator,
        TerminalSnapshot {
            revision: shared
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .revision
                .wrapping_add(1),
            cols: size.cols,
            rows: size.rows,
            content,
            highlights: Arc::from([]),
            foreground: Color::rgb8(248, 113, 113),
            background: Color::rgb8(20, 20, 20),
            title: Arc::from("Terminal error"),
            working_directory: Arc::from(""),
            scroll: TerminalScrollState::initial(size.rows),
            cursor: None,
            selected_text: None,
            cursor_range: None,
            graphics: Arc::from([]),
            edge_backgrounds: EdgeBackgrounds::empty(size.cols, size.rows),
            status: TerminalStatus::Failed {
                message: Arc::from(message),
            },
            agent: None,
        },
    );
}

pub(super) fn ghostty_color(color: RgbColor) -> Color {
    Color::rgb8(color.r, color.g, color.b)
}

pub(super) fn valid_os_string(value: &OsStr) -> bool {
    let value = value.to_string_lossy();
    value.len() <= MAX_TERMINAL_STRING_BYTES && !value.contains('\0')
}

pub(super) fn finite_clamp(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

pub(super) fn pack_size(size: TerminalSize) -> u64 {
    u64::from(size.cols)
        | (u64::from(size.rows) << 16)
        | (u64::from(size.cell_width_px.min(u16::MAX as u32)) << 32)
        | (u64::from(size.cell_height_px.min(u16::MAX as u32)) << 48)
}
