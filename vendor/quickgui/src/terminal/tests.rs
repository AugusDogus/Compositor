use super::*;

#[test]
fn options_reject_unbounded_process_declarations() {
    let options = TerminalOptions {
        arguments: vec![OsString::from("x"); MAX_TERMINAL_ARGUMENTS + 1],
        ..TerminalOptions::default()
    };
    assert!(matches!(
        options.validate(),
        Err(TerminalError::TooManyArguments)
    ));
}

#[test]
fn ghostty_character_mapping_covers_terminal_control_keys() {
    assert_eq!(ghostty_character_key('c'), GhosttyKey::C);
    assert_eq!(ghostty_character_key('C'), GhosttyKey::C);
    assert_eq!(ghostty_character_key('!'), GhosttyKey::Digit1);
    assert_eq!(unshift_character('?'), '/');
}

#[test]
fn composed_key_text_overrides_a_synthetic_physical_key() {
    let physical = Key::Character("a".to_owned());
    let (key, text, unshifted) =
        ghostty_key(&physical, Some(&physical), Some("Z")).expect("mapped key");
    assert_eq!(key, GhosttyKey::Z);
    assert_eq!(text.as_deref(), Some("Z"));
    assert_eq!(unshifted, Some('a'));
}

#[test]
fn terminal_rows_drop_invisible_grid_padding_and_clip_highlights() {
    let mut content = String::from("prompt    ");
    let mut highlights = vec![(4..10, HighlightStyle::default())];

    trim_terminal_row(&mut content, &mut highlights, 6);

    assert_eq!(content, "prompt");
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0].0, 4..6);
}

#[cfg(not(quickgui_terminal_extension))]
#[test]
fn terminal_pointer_cells_clamp_to_the_complete_grid() {
    assert_eq!(terminal_pointer_cell(-12.0, 8.0, 10), 0);
    assert_eq!(terminal_pointer_cell(16.1, 8.0, 10), 2);
    assert_eq!(terminal_pointer_cell(10_000.0, 8.0, 10), 9);
}

#[test]
fn terminal_cursor_and_blank_cell_backgrounds_remain_visible() {
    assert!(!CellVisual::default().has_visible_blank_paint());
    assert!(
        CellVisual {
            background: Some(Color::WHITE),
            ..CellVisual::default()
        }
        .has_visible_blank_paint()
    );
}

#[cfg(not(quickgui_terminal_extension))]
#[test]
fn terminal_cursor_highlight_overrides_and_splits_ansi_runs() {
    let ansi = HighlightStyle::default().color(Color::rgb8(255, 0, 0));
    let cursor = HighlightStyle::default()
        .color(Color::BLACK)
        .background(Color::WHITE);
    let highlights = terminal_highlights_with_cursor(&[(0..10, ansi.clone())], 4..5, cursor);

    assert_eq!(highlights.len(), 3);
    assert_eq!(highlights[0], (0..4, ansi.clone()));
    assert_eq!(highlights[1].0, 4..5);
    assert_eq!(highlights[2], (5..10, ansi));
}

#[cfg(not(quickgui_terminal_extension))]
#[test]
fn terminal_cursor_blink_restarts_immediately_on_each_focus_gain() {
    let start = Instant::now();
    let mut blink = TerminalCursorBlink::new(start);
    let first_focus = start + Duration::from_millis(750);

    assert_eq!(blink.update_focus(true, first_focus), first_focus);
    assert_eq!(
        blink.update_focus(true, first_focus + Duration::from_millis(250)),
        first_focus,
        "ordinary focused redraws must not restart the blink cycle"
    );
    assert_eq!(
        blink.update_focus(false, first_focus + Duration::from_secs(1)),
        first_focus
    );

    let second_focus = first_focus + Duration::from_secs(2);
    assert_eq!(blink.update_focus(true, second_focus), second_focus);
}

#[cfg(not(quickgui_terminal_extension))]
#[test]
fn terminal_wheel_follows_platform_content_motion_and_retains_fractional_rows() {
    let mut remainder = 0.0;
    assert_eq!(accumulate_terminal_rows(&mut remainder, -0.4), 0);
    assert_eq!(accumulate_terminal_rows(&mut remainder, -0.7), -1);
    assert!((-0.11..=-0.09).contains(&remainder));
    assert_eq!(accumulate_terminal_rows(&mut remainder, 1.1), 1);

    let mut wheel = 0.0;
    let line_height = 18.0;
    assert_eq!(
        accumulate_terminal_rows(&mut wheel, -line_height / line_height),
        -1,
        "positive platform content motion must move the terminal viewport into history"
    );
    assert_eq!(
        accumulate_terminal_rows(&mut wheel, line_height / line_height),
        1,
        "negative platform content motion must move the terminal viewport toward the prompt"
    );
}

#[cfg(not(quickgui_terminal_extension))]
#[test]
fn terminal_scrollbar_geometry_reaches_both_ends() {
    let mut state = TerminalScrollState {
        total_rows: 100,
        offset_rows: 0,
        viewport_rows: 20,
    };
    let top = terminal_scrollbar_geometry(state, 200.0).unwrap();
    assert_eq!(top.thumb_top, 0.0);
    assert_eq!(top.thumb_height, 40.0);
    assert_eq!(top.travel, 160.0);

    state.offset_rows = state.max_offset_rows();
    let bottom = terminal_scrollbar_geometry(state, 200.0).unwrap();
    assert_eq!(bottom.thumb_top, bottom.travel);
    assert!(terminal_scrollbar_geometry(TerminalScrollState::initial(24), 200.0).is_none());
}

#[cfg(not(quickgui_terminal_extension))]
#[test]
fn terminal_scrollbar_reveals_expands_and_auto_hides() {
    let start = Instant::now();
    let mut interaction = TerminalScrollbarInteraction::default();
    assert_eq!(
        interaction.presentation(start),
        TerminalScrollbarPresentation {
            visible: false,
            expanded: false,
            hide_at: None,
        }
    );

    interaction.reveal(start);
    let revealed = interaction.presentation(start);
    assert!(revealed.visible);
    assert!(!revealed.expanded);
    assert_eq!(
        revealed.hide_at,
        Some(start + TERMINAL_SCROLLBAR_HIDE_DELAY)
    );

    interaction.set_hovered(true, start + Duration::from_millis(100));
    let hovered = interaction.presentation(start + Duration::from_millis(100));
    assert!(hovered.visible);
    assert!(hovered.expanded);
    assert_eq!(hovered.hide_at, None);

    interaction.set_hovered(false, start + Duration::from_millis(200));
    assert!(
        interaction
            .presentation(start + Duration::from_millis(200))
            .visible
    );
    assert!(
        !interaction
            .presentation(start + Duration::from_millis(200) + TERMINAL_SCROLLBAR_HIDE_DELAY)
            .visible
    );
}

#[test]
fn every_encoded_keystroke_follows_the_live_prompt() {
    let terminal = GhosttyTerminal::new(GhosttyTerminalOptions {
        cols: 80,
        rows: 24,
        max_scrollback: 100,
    })
    .unwrap();
    let mut encoder = GhosttyKeyEncoder::new().unwrap();
    let mut writer: Box<dyn Write + Send> = Box::new(Vec::<u8>::new());
    let input = |key, text, action| TerminalKeyInput {
        key,
        key_char: None,
        text,
        modifiers: Modifiers::empty(),
        action,
    };

    assert!(write_key(
        &terminal,
        &mut encoder,
        &mut writer,
        input(
            Key::Character("a".to_owned()),
            Some("a".to_owned()),
            GhosttyKeyAction::Press,
        ),
    ));
    assert!(write_key(
        &terminal,
        &mut encoder,
        &mut writer,
        input(Key::Backspace, None, GhosttyKeyAction::Press),
    ));
    assert!(write_key(
        &terminal,
        &mut encoder,
        &mut writer,
        input(Key::ArrowUp, None, GhosttyKeyAction::Press),
    ));
    assert!(write_key(
        &terminal,
        &mut encoder,
        &mut writer,
        input(Key::Enter, None, GhosttyKeyAction::Repeat),
    ));
    assert!(!write_key(
        &terminal,
        &mut encoder,
        &mut writer,
        input(Key::Other, None, GhosttyKeyAction::Press),
    ));
}

#[cfg(not(quickgui_terminal_extension))]
#[test]
fn terminal_style_normalizes_content_padding() {
    let style = TerminalStyle {
        font_thicken: true,
        padding_top: 8.0,
        padding_right: 9.0,
        padding_bottom: f32::INFINITY,
        padding_left: -4.0,
        padding_color: TerminalPaddingColor::Extend,
        ..TerminalStyle::default()
    }
    .normalized();

    assert_eq!(style.padding_top, 8.0);
    assert_eq!(style.padding_right, 9.0);
    assert_eq!(style.padding_bottom, 0.0);
    assert_eq!(style.padding_left, 0.0);
    assert_eq!(style.padding_color, TerminalPaddingColor::Extend);
    assert!(style.font_thicken);
}

#[cfg(not(quickgui_terminal_extension))]
#[test]
fn terminal_selection_uses_pointer_coordinates_local_to_the_terminal() {
    let event = PointerEvent {
        size: crate::Size::ZERO,
        phase: PointerPhase::Move,
        position: crate::Point::new(436.0, 158.0),
        origin: crate::Point::new(420.0, 158.0),
        local_position: crate::Point::new(36.0, 18.0),
        local_origin: crate::Point::new(20.0, 18.0),
        delta: crate::Vector::new(16.0, 0.0),
        button: MouseButton::Left,
        modifiers: Modifiers::empty(),
    };
    let input = terminal_selection_input(
        &event,
        Rect::new(0.0, 0.0, 800.0, 360.0),
        100,
        20,
        8.0,
        18.0,
        16,
        2.0,
        Duration::from_millis(100),
    )
    .unwrap();

    assert_eq!(input.column, 4);
    assert_eq!(input.row, 1);
    assert_eq!(input.surface_x, 72.0);
    assert_eq!(input.surface_y, 36.0);
}

#[test]
fn ghostty_snapshot_retains_complete_lines() {
    let mut terminal = GhosttyTerminal::new(GhosttyTerminalOptions {
        cols: 80,
        rows: 24,
        max_scrollback: 100,
    })
    .unwrap();
    terminal.vt_write(b"quickgui-pty-ok\r\n");
    let mut render_state = RenderState::new().unwrap();
    let mut rows = RowIterator::new().unwrap();
    let mut cells = CellIterator::new().unwrap();
    let snapshot = build_snapshot(
        &terminal,
        &mut render_state,
        &mut rows,
        &mut cells,
        TerminalSnapshotMetadata::running(1),
    )
    .unwrap();
    assert!(
        snapshot.content.contains("quickgui-pty-ok"),
        "snapshot: {snapshot:#?}"
    );
    assert_eq!(snapshot.scroll.viewport_rows, 24);
    assert_eq!(snapshot.scroll.total_rows, 24);
    assert_eq!(snapshot.scroll.offset_rows, 0);
}

#[test]
fn ghostty_selection_follows_release_into_blank_cells_without_copy_padding() {
    let mut terminal = GhosttyTerminal::new(GhosttyTerminalOptions {
        cols: 8,
        rows: 2,
        max_scrollback: 100,
    })
    .unwrap();
    terminal.vt_write(b"hi");
    let geometry = GhosttySelectionGeometry {
        columns: 8,
        cell_width: 8,
        padding_left: 0,
        screen_height: 32,
    };
    let input = |phase, column, surface_x| TerminalSelectionInput {
        phase,
        column,
        row: 0,
        surface_x,
        surface_y: 8.0,
        geometry,
        time: Duration::from_millis(100),
        repeat_distance: 4.0,
        rectangle: false,
    };
    let mut selection = TerminalSelectionState::new().unwrap();
    selection
        .apply(&terminal, input(TerminalSelectionPhase::Press, 1, 12.0))
        .unwrap();
    selection
        .apply(&terminal, input(TerminalSelectionPhase::Release, 6, 53.0))
        .unwrap();

    let mut render_state = RenderState::new().unwrap();
    let mut rows = RowIterator::new().unwrap();
    let mut cells = CellIterator::new().unwrap();
    let snapshot = build_snapshot(
        &terminal,
        &mut render_state,
        &mut rows,
        &mut cells,
        TerminalSnapshotMetadata::running(1),
    )
    .unwrap();

    assert_eq!(snapshot.content.as_ref(), "hi     ");
    assert_eq!(snapshot.selected_text.as_deref(), Some("i"));
    assert!(snapshot.highlights.iter().any(|(range, style)| {
        range.start <= 1 && range.end >= 7 && style.background == Some(static_selection_color())
    }));
    selection.clear(&terminal);
}

#[test]
fn ghostty_snapshot_retains_grid_edge_backgrounds() {
    let mut terminal = GhosttyTerminal::new(GhosttyTerminalOptions {
        cols: 4,
        rows: 3,
        max_scrollback: 100,
    })
    .unwrap();
    terminal.vt_write(b"\x1b[48;2;10;20;30m\x1b[2J\x1b[H");
    let mut render_state = RenderState::new().unwrap();
    let mut rows = RowIterator::new().unwrap();
    let mut cells = CellIterator::new().unwrap();
    let snapshot = build_snapshot(
        &terminal,
        &mut render_state,
        &mut rows,
        &mut cells,
        TerminalSnapshotMetadata::running(1),
    )
    .unwrap();
    let surface = Color::rgb8(10, 20, 30);

    assert!(
        snapshot
            .edge_backgrounds
            .top
            .iter()
            .chain(snapshot.edge_backgrounds.right.iter())
            .chain(snapshot.edge_backgrounds.bottom.iter())
            .chain(snapshot.edge_backgrounds.left.iter())
            .all(|color| *color == Some(surface)),
        "edge backgrounds: {:?}",
        snapshot.edge_backgrounds,
    );
}

#[test]
fn ghostty_snapshot_preserves_block_text_and_uses_cell_graphics() {
    let mut terminal = GhosttyTerminal::new(GhosttyTerminalOptions {
        cols: 20,
        rows: 4,
        max_scrollback: 100,
    })
    .unwrap();
    terminal.vt_write("█▀▄\r\n".as_bytes());
    let mut render_state = RenderState::new().unwrap();
    let mut rows = RowIterator::new().unwrap();
    let mut cells = CellIterator::new().unwrap();
    let snapshot = build_snapshot(
        &terminal,
        &mut render_state,
        &mut rows,
        &mut cells,
        TerminalSnapshotMetadata::running(1),
    )
    .unwrap();

    assert!(snapshot.content.starts_with("█▀▄"));
    assert_eq!(
        snapshot
            .graphics
            .iter()
            .map(|graphic| (graphic.column, graphic.row, graphic.character))
            .collect::<Vec<_>>(),
        [(0, 0, '█'), (1, 0, '▀'), (2, 0, '▄')]
    );
    assert_eq!(snapshot.highlights[0].0, 0.."█▀▄".len());
    assert_eq!(snapshot.highlights[0].1.color, Some(Color::TRANSPARENT));
}

#[test]
fn ghostty_snapshot_preserves_spinner_columns() {
    let mut terminal = GhosttyTerminal::new(GhosttyTerminalOptions {
        cols: 20,
        rows: 4,
        max_scrollback: 100,
    })
    .unwrap();
    terminal.vt_write("■■■■⬝⬝⬝⬝".as_bytes());
    let mut render_state = RenderState::new().unwrap();
    let mut rows = RowIterator::new().unwrap();
    let mut cells = CellIterator::new().unwrap();
    let snapshot = build_snapshot(
        &terminal,
        &mut render_state,
        &mut rows,
        &mut cells,
        TerminalSnapshotMetadata::running(1),
    )
    .unwrap();

    assert!(
        snapshot.content.starts_with("■■■■⬝⬝⬝⬝"),
        "snapshot content: {:?}",
        snapshot.content,
    );
    assert_eq!(snapshot.cursor.unwrap().column, 8);
}

#[test]
fn terminal_theme_updates_libghostty_defaults_and_ansi_palette() {
    let mut terminal = GhosttyTerminal::new(GhosttyTerminalOptions {
        cols: 20,
        rows: 4,
        max_scrollback: 100,
    })
    .unwrap();
    let ansi = std::array::from_fn(|index| {
        Color::rgb8(index as u8, (index as u8).saturating_add(16), 240)
    });
    let theme = TerminalTheme::new(Color::rgb8(31, 35, 40), Color::WHITE, ansi)
        .cursor(Color::rgb8(9, 105, 218));
    apply_terminal_theme(&mut terminal, Some(&theme)).unwrap();
    terminal.vt_write(b"\x1b[31mred");

    let colors = terminal.default_color_palette().unwrap();
    assert_eq!(colors.0[1], terminal_rgb(theme.ansi[1]));
    assert_eq!(
        terminal.default_fg_color().unwrap(),
        Some(terminal_rgb(theme.foreground))
    );
    assert_eq!(
        terminal.default_bg_color().unwrap(),
        Some(terminal_rgb(theme.background))
    );
    assert_eq!(
        terminal.default_cursor_color().unwrap(),
        Some(terminal_rgb(theme.cursor))
    );

    let mut render_state = RenderState::new().unwrap();
    let mut rows = RowIterator::new().unwrap();
    let mut cells = CellIterator::new().unwrap();
    let snapshot = build_snapshot(
        &terminal,
        &mut render_state,
        &mut rows,
        &mut cells,
        TerminalSnapshotMetadata::running(1),
    )
    .unwrap();
    assert_eq!(snapshot.foreground, theme.foreground);
    assert_eq!(snapshot.background, theme.background);
    assert_eq!(snapshot.highlights[0].1.color, Some(theme.ansi[1]));
}

#[test]
fn ghostty_snapshot_retains_the_cursor_cell_and_shape() {
    let mut terminal = GhosttyTerminal::new(GhosttyTerminalOptions {
        cols: 20,
        rows: 4,
        max_scrollback: 100,
    })
    .unwrap();
    terminal.vt_write(b"prompt");
    let mut render_state = RenderState::new().unwrap();
    let mut rows = RowIterator::new().unwrap();
    let mut cells = CellIterator::new().unwrap();
    let snapshot = build_snapshot(
        &terminal,
        &mut render_state,
        &mut rows,
        &mut cells,
        TerminalSnapshotMetadata::running(1),
    )
    .unwrap();

    let cursor = snapshot.cursor.expect("visible cursor metadata");
    assert_eq!((cursor.column, cursor.row), (6, 0));
    assert_eq!(cursor.style, TerminalCursorStyle::Block);
    assert_eq!(snapshot.cursor_range, Some(6..7));
    assert_eq!(snapshot.content.as_ref(), "prompt ");
}

#[test]
fn ghostty_snapshot_reports_real_scrollback_position() {
    let mut terminal = GhosttyTerminal::new(GhosttyTerminalOptions {
        cols: 40,
        rows: 10,
        max_scrollback: 100,
    })
    .unwrap();
    for line in 0..40 {
        terminal.vt_write(format!("line {line}\r\n").as_bytes());
    }
    let mut render_state = RenderState::new().unwrap();
    let mut rows = RowIterator::new().unwrap();
    let mut cells = CellIterator::new().unwrap();
    let bottom = build_snapshot(
        &terminal,
        &mut render_state,
        &mut rows,
        &mut cells,
        TerminalSnapshotMetadata::running(1),
    )
    .unwrap();
    assert!(bottom.scroll.total_rows > bottom.scroll.viewport_rows);
    assert_eq!(
        bottom.scroll.offset_rows,
        bottom.scroll.max_offset_rows(),
        "the live prompt is the bottom of libghostty's scroll range"
    );

    terminal.scroll_viewport(ScrollViewport::Top);
    let top = build_snapshot(
        &terminal,
        &mut render_state,
        &mut rows,
        &mut cells,
        TerminalSnapshotMetadata::running(2),
    )
    .unwrap();
    assert_eq!(top.scroll.offset_rows, 0);
}

#[test]
fn visual_runs_merge_and_never_exceed_styled_text_limit() {
    let visual = CellVisual {
        foreground: Some(Color::rgb8(255, 0, 0)),
        ..CellVisual::default()
    };
    let mut active = None;
    let mut highlights = Vec::new();
    push_visual_run(&mut active, &mut highlights, 0, 1, visual);
    push_visual_run(&mut active, &mut highlights, 1, 2, visual);
    flush_visual_run(&mut active, &mut highlights);
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0].0, 0..2);
}

#[test]
fn terminal_identity_matches_ghostty_compatibility_contract() {
    assert_eq!(TERM_PROGRAM_VALUE, "ghostty");
}

#[test]
fn terminal_working_directory_is_a_local_path_not_an_osc_7_uri() {
    assert_eq!(
        normalize_terminal_working_directory(
            "file://egoists-MacBook-Air.local/Users/egoist/My%20Project"
        ),
        "/Users/egoist/My Project"
    );
    assert_eq!(
        normalize_terminal_working_directory("file:///private/tmp/herdr"),
        "/private/tmp/herdr"
    );
    assert_eq!(
        normalize_terminal_working_directory("/Users/egoist/dev/quickgui"),
        "/Users/egoist/dev/quickgui"
    );
}

#[test]
fn detected_agent_status_uses_live_terminal_signals() {
    assert_eq!(
        detect_agent_status(
            "claude",
            "Bash command\nDo you want to proceed?\n1. Yes\n2. No",
            "Action Required",
            false,
        ),
        TerminalAgentStatus::Blocked
    );
    assert_eq!(
        detect_agent_status("codex", "Working (esc to interrupt)", "", false),
        TerminalAgentStatus::Working
    );
    assert_eq!(
        detect_agent_status("opencode", "ready", "", false),
        TerminalAgentStatus::Idle
    );
}
