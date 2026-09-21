use super::*;

pub(super) fn terminal_cursor_presentation<V: 'static>(
    cursor: TerminalCursor,
    focused: bool,
    blink_epoch: Instant,
    now: Instant,
    cx: &mut ViewContext<'_, V>,
) -> Option<TerminalCursorStyle> {
    if !focused {
        return Some(TerminalCursorStyle::BlockHollow);
    }
    let elapsed = now.saturating_duration_since(blink_epoch);
    let half_periods = elapsed.as_millis() / TERMINAL_CURSOR_BLINK_HALF_PERIOD.as_millis();
    let phase_elapsed = elapsed.as_millis() % TERMINAL_CURSOR_BLINK_HALF_PERIOD.as_millis();
    let until_next = TERMINAL_CURSOR_BLINK_HALF_PERIOD
        - Duration::from_millis(phase_elapsed.try_into().unwrap_or(u64::MAX));
    cx.request_repaint_at(now + until_next);
    half_periods.is_multiple_of(2).then_some(cursor.style)
}

pub(super) fn terminal_cursor_element(
    cursor: TerminalCursor,
    style: TerminalCursorStyle,
    cell_width: f32,
    line_height: f32,
    color: Color,
) -> Element {
    let left = f32::from(cursor.column) * cell_width;
    let top = f32::from(cursor.row) * line_height;
    match style {
        TerminalCursorStyle::Bar => div()
            .absolute()
            .left(left)
            .top(top)
            .w((cell_width * 0.18).clamp(1.5, 2.5))
            .h(line_height)
            .bg(color),
        TerminalCursorStyle::Block => div()
            .absolute()
            .left(left)
            .top(top)
            .w(cell_width)
            .h(line_height)
            .bg(color),
        TerminalCursorStyle::Underline => div()
            .absolute()
            .left(left)
            .top(top + (line_height - 2.0).max(0.0))
            .w(cell_width)
            .h(2.0_f32.min(line_height))
            .bg(color),
        TerminalCursorStyle::BlockHollow => div()
            .absolute()
            .left(left)
            .top(top)
            .w(cell_width)
            .h(line_height)
            .border(1.0, color),
    }
}

pub(super) fn terminal_highlights_with_cursor(
    highlights: &[(Range<usize>, HighlightStyle)],
    cursor: Range<usize>,
    cursor_style: HighlightStyle,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut combined = Vec::with_capacity((highlights.len() + 2).min(MAX_TEXT_HIGHLIGHTS + 2));
    let mut inserted = false;
    for (range, style) in highlights {
        if range.end <= cursor.start {
            combined.push((range.clone(), style.clone(), false));
            continue;
        }
        if range.start >= cursor.end {
            if !inserted {
                combined.push((cursor.clone(), cursor_style.clone(), true));
                inserted = true;
            }
            combined.push((range.clone(), style.clone(), false));
            continue;
        }
        if range.start < cursor.start {
            combined.push((range.start..cursor.start, style.clone(), false));
        }
        if !inserted {
            combined.push((cursor.clone(), cursor_style.clone(), true));
            inserted = true;
        }
        if range.end > cursor.end {
            combined.push((cursor.end..range.end, style.clone(), false));
        }
    }
    if !inserted {
        combined.push((cursor, cursor_style, true));
    }
    combined.sort_unstable_by_key(|(range, _, _)| range.start);
    while combined.len() > MAX_TEXT_HIGHLIGHTS {
        let Some(index) = combined.iter().rposition(|(_, _, cursor)| !cursor) else {
            break;
        };
        combined.remove(index);
    }
    combined
        .into_iter()
        .map(|(range, style, _)| (range, style))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TerminalScrollbarGeometry {
    pub(super) thumb_top: f32,
    pub(super) thumb_height: f32,
    pub(super) travel: f32,
    pub(super) max_offset_rows: u64,
}

pub(super) fn terminal_scrollbar_geometry(
    scroll: TerminalScrollState,
    viewport_height: f32,
) -> Option<TerminalScrollbarGeometry> {
    let max_offset_rows = scroll.max_offset_rows();
    if max_offset_rows == 0
        || scroll.total_rows == 0
        || !viewport_height.is_finite()
        || viewport_height <= 0.0
    {
        return None;
    }
    let thumb_height = (viewport_height * scroll.viewport_rows as f32 / scroll.total_rows as f32)
        .max(TERMINAL_SCROLLBAR_MIN_THUMB)
        .min(viewport_height);
    let travel = viewport_height - thumb_height;
    let thumb_top =
        travel * (scroll.offset_rows.min(max_offset_rows) as f32 / max_offset_rows as f32);
    Some(TerminalScrollbarGeometry {
        thumb_top,
        thumb_height,
        travel,
        max_offset_rows,
    })
}

pub(super) fn accumulate_terminal_rows(remainder: &mut f32, rows: f32) -> isize {
    if !rows.is_finite() || rows == 0.0 {
        return 0;
    }
    *remainder = (*remainder + rows).clamp(-(MAX_ROWS as f32), MAX_ROWS as f32);
    let complete = remainder.trunc();
    *remainder -= complete;
    complete as isize
}

pub(super) fn derived_terminal_id(parent: ElementId, tag: u64) -> ElementId {
    let mut hash = parent.as_u64() ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == parent.as_u64() || hash == u64::MAX {
        hash ^= tag.rotate_left(17);
    }
    ElementId::new(hash)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn terminal_selection_input(
    event: &PointerEvent,
    bounds: Rect,
    cols: u16,
    rows: u16,
    cell_width: f32,
    line_height: f32,
    physical_cell_width: u32,
    scale_factor: f32,
    time: Duration,
) -> Option<TerminalSelectionInput> {
    if cols == 0
        || rows == 0
        || bounds.width <= 0.0
        || bounds.height <= 0.0
        || !bounds.x.is_finite()
        || !bounds.y.is_finite()
        || !bounds.width.is_finite()
        || !bounds.height.is_finite()
        || !event.local_position.x.is_finite()
        || !event.local_position.y.is_finite()
        || !cell_width.is_finite()
        || cell_width <= 0.0
        || !line_height.is_finite()
        || line_height <= 0.0
        || !scale_factor.is_finite()
        || scale_factor <= 0.0
    {
        return None;
    }
    let local_x = event.local_position.x;
    let local_y = event.local_position.y;
    let column = terminal_pointer_cell(local_x, cell_width, cols);
    let row = terminal_pointer_cell(local_y, line_height, rows);
    let surface_scale = f64::from(scale_factor);
    Some(TerminalSelectionInput {
        phase: match event.phase {
            PointerPhase::Down => TerminalSelectionPhase::Press,
            PointerPhase::Move => TerminalSelectionPhase::Drag,
            PointerPhase::Up => TerminalSelectionPhase::Release,
            PointerPhase::Cancel => TerminalSelectionPhase::Cancel,
        },
        column,
        row,
        surface_x: f64::from(local_x) * surface_scale,
        surface_y: f64::from(local_y) * surface_scale,
        geometry: GhosttySelectionGeometry {
            columns: u32::from(cols),
            cell_width: physical_cell_width.max(1),
            padding_left: 0,
            screen_height: (bounds.height * scale_factor).round().max(1.0) as u32,
        },
        time,
        repeat_distance: f64::from(TERMINAL_MULTI_CLICK_DISTANCE * scale_factor),
        rectangle: event.modifiers.contains(Modifiers::ALT),
    })
}

pub(super) fn terminal_pointer_cell(position: f32, cell_size: f32, count: u16) -> u16 {
    let maximum = i32::from(count.saturating_sub(1));
    ((position / cell_size).floor() as i32).clamp(0, maximum) as u16
}

#[cfg(not(quickgui_terminal_extension))]
pub(super) fn handle_terminal_copy(
    terminal: &Terminal,
    event: &KeyDownEvent,
    cx: &mut EventContext,
) -> bool {
    if !terminal_command_shortcut(event, "c") {
        return false;
    }
    if let Some(text) = terminal.snapshot().selected_text.clone()
        && let Ok(item) = ClipboardItem::new_string(text)
    {
        let _ = cx.write_to_clipboard(item);
    }
    cx.prevent_default();
    cx.stop_propagation();
    true
}

#[cfg(not(quickgui_terminal_extension))]
pub(super) fn handle_terminal_select_all(
    terminal: &Terminal,
    event: &KeyDownEvent,
    cx: &mut EventContext,
) -> bool {
    if !terminal_command_shortcut(event, "a") {
        return false;
    }
    let _ = terminal.try_send(WorkerMessage::SelectAll);
    cx.clear_text_selection();
    cx.prevent_default();
    cx.stop_propagation();
    cx.invalidate();
    true
}

#[cfg(not(quickgui_terminal_extension))]
pub(super) fn handle_terminal_paste(
    terminal: &Terminal,
    event: &KeyDownEvent,
    cx: &mut EventContext,
) -> bool {
    if !terminal_command_shortcut(event, "v") {
        return false;
    }
    if let Ok(Some(item)) = cx.read_from_clipboard()
        && let Some(value) = item.text()
    {
        let _ = terminal.paste(value);
    }
    cx.clear_text_selection();
    cx.prevent_default();
    cx.stop_propagation();
    true
}

#[cfg(not(quickgui_terminal_extension))]
pub(super) fn terminal_command_shortcut(event: &KeyDownEvent, key: &str) -> bool {
    key_character(&event.key).is_some_and(|value| value.eq_ignore_ascii_case(key))
        && (event.modifiers.contains(Modifiers::SUPER)
            || (cfg!(not(target_os = "macos"))
                && event
                    .modifiers
                    .contains(Modifiers::CONTROL | Modifiers::SHIFT)))
}
