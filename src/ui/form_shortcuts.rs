use super::*;

impl Editor {
    pub(super) fn form_key(&mut self, key: &Key, modifiers: Modifiers, cx: &mut EventContext) {
        if !self.errors.is_empty() || self.panel_applying() {
            return;
        }
        let panel_command = (*key == Key::Function(10) && modifiers.is_empty())
            || (modifiers == Modifiers::ALT
                && matches!(key, Key::Character(c) if ["f", "e", "i", "l", "s", "t", "v", "h"].contains(&c.to_lowercase().as_str())))
            || (modifiers == Modifiers::CONTROL
                && matches!(key, Key::Character(c) if ["s", "z", "y", "i", "h", "0", "1", "+", "=", "-"].contains(&c.to_lowercase().as_str())))
            || (modifiers == (Modifiers::CONTROL | Modifiers::SHIFT)
                && matches!(key, Key::Character(c) if ["z", "s", "e"].contains(&c.to_lowercase().as_str())))
            || (modifiers == (Modifiers::CONTROL | Modifiers::ALT)
                && matches!(key, Key::Character(c) if ["c", "i"].contains(&c.to_lowercase().as_str())))
            || (modifiers == (Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT)
                && matches!(key, Key::Character(c) if c.eq_ignore_ascii_case("s")));
        if self.floating_panel_kind().is_some() && panel_command {
            cx.prevent_default();
            cx.stop_propagation();
            self.key(key, modifiers, cx);
        } else if *key == Key::Enter
            && modifiers.is_empty()
            && matches!(self.modal, Some(Form::Edit { .. }))
        {
            cx.prevent_default();
            cx.stop_propagation();
            self.size_menus.close(cx);
            self.submit_form(cx);
            self.changed(cx);
        } else if modifiers == Modifiers::ALT
            && matches!(key, Key::Character(c) if c.eq_ignore_ascii_case("p"))
            && self
                .adjustment_edit
                .as_ref()
                .is_some_and(|edit| edit.settings.kind == Kind::Levels)
        {
            cx.prevent_default();
            cx.stop_propagation();
            if let Some(edit) = &mut self.adjustment_edit {
                edit.preview = !edit.preview;
            }
            self.refresh_adjustment();
            self.changed(cx);
        } else {
            cx.propagate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Keystroke, WindowOptions};

    #[test]
    fn pan_and_zoom_beneath_panels_preserve_the_edit_and_cancelled_zoom() {
        for panel in 0..3 {
            for tool in [Tool::Hand, Tool::Zoom] {
                let mut editor = Editor::with_test_document();
                let mut doc = Document::new(8, 8).unwrap();
                compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
                editor.tabs = vec![Session::new(doc.clone(), None).into()];
                editor.tools.tool = tool;
                editor.session_mut().fit = false;
                editor.session_mut().zoom = 1.;
                match panel {
                    0 => editor.open_pixel_adjustment(Kind::HueSaturation).unwrap(),
                    1 => editor
                        .open_filter(compositor::filters::Filter::Gaussian { radius: 1. })
                        .unwrap(),
                    _ => editor.open_pixel_adjustment(Kind::Levels).unwrap(),
                }
                let (mut cx, view) = Application::new()
                    .into_test_context(
                        WindowOptions::new("Panel navigation").size(1500., 900.),
                        editor,
                    )
                    .unwrap();
                let window = view.window_handle();
                let bounds = cx.element_bounds(window, "canvas").unwrap();
                let start = quickgui::Point::new(bounds.x + 20., bounds.y + 20.);
                let end = quickgui::Point::new(start.x + 100., start.y + 20.);
                let initial = cx
                    .read(view, |e| (e.session().zoom, e.session().pan))
                    .unwrap();
                cx.simulate_pointer_drag(window, "canvas", start, end)
                    .unwrap();
                cx.read(view, |e| {
                    if tool == Tool::Hand {
                        assert_eq!(e.session().pan, [initial.1[0] + 100., initial.1[1] + 20.]);
                    } else {
                        assert!((e.session().zoom - initial.0 * 2.).abs() < 0.0001);
                    }
                    assert!(e.floating_panel_kind().is_some());
                    assert!(e.session().has_pending_edit());
                    assert_eq!(e.session().committed_document(), &doc);
                    assert!(e.session().undo_label().is_none());
                })
                .unwrap();
                if tool == Tool::Zoom {
                    let zoom = cx.read(view, |e| e.session().zoom).unwrap();
                    cx.update(view, |e, _| {
                        let mut event = quickgui::PointerEvent {
                            phase: quickgui::PointerPhase::Down,
                            button: quickgui::MouseButton::Left,
                            position: start,
                            origin: start,
                            local_position: quickgui::Point::new(20., 20.),
                            local_origin: quickgui::Point::new(20., 20.),
                            delta: quickgui::Vector::ZERO,
                            size: quickgui::Size::new(bounds.width, bounds.height),
                            modifiers: Modifiers::empty(),
                        };
                        e.pointer(&event).unwrap();
                        event.phase = quickgui::PointerPhase::Move;
                        event.local_position.x += 100.;
                        e.pointer(&event).unwrap();
                        event.phase = quickgui::PointerPhase::Cancel;
                        e.pointer(&event).unwrap();
                        assert_eq!(e.session().zoom, zoom);
                        assert!(e.session().has_pending_edit());
                    })
                    .unwrap();
                }
            }
        }
    }

    #[test]
    fn canvas_tool_shortcuts_under_panels_preserve_text_focus_and_preview() {
        for panel in 0..3 {
            let mut editor = Editor::with_test_document();
            let mut doc = Document::new(8, 8).unwrap();
            compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
            editor.tabs = vec![Session::new(doc.clone(), None).into()];
            editor.tools.tool = Tool::Brush;
            match panel {
                0 => editor.open_pixel_adjustment(Kind::HueSaturation).unwrap(),
                1 => editor
                    .open_filter(compositor::filters::Filter::Gaussian { radius: 1. })
                    .unwrap(),
                _ => editor.open_pixel_adjustment(Kind::Levels).unwrap(),
            }
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Panel canvas focus").size(1500., 900.),
                    editor,
                )
                .unwrap();
            let window = view.window_handle();
            cx.focus(window, 50_000_u64).unwrap();
            cx.simulate_keystrokes(window, "z").unwrap();
            assert_eq!(cx.read(view, |e| e.tools.tool).unwrap(), Tool::Brush);
            let bounds = cx.element_bounds(window, "canvas").unwrap();
            let point = quickgui::Point::new(bounds.x + 20., bounds.y + 20.);
            cx.simulate_pointer_drag(window, "canvas", point, point)
                .unwrap();
            assert_eq!(cx.focused(window).unwrap(), Some("workspace".into()));
            cx.simulate_keystrokes(window, "z").unwrap();
            cx.read(view, |e| {
                assert_eq!(
                    e.tools.tool,
                    if panel == 2 { Tool::Brush } else { Tool::Zoom }
                );
                assert!(e.floating_panel_kind().is_some());
                assert_eq!(e.session().committed_document(), &doc);
            })
            .unwrap();
        }
    }

    #[test]
    fn transform_controls_shortcut_works_from_hue_and_filter_fields() {
        for filter in [false, true] {
            let mut editor = Editor::with_test_document();
            let mut doc = Document::new(8, 8).unwrap();
            compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
            editor.tabs = vec![Session::new(doc.clone(), None).into()];
            editor.tools.tool = Tool::Move;
            if filter {
                editor
                    .open_filter(compositor::filters::Filter::Gaussian { radius: 1. })
                    .unwrap();
            } else {
                editor.open_pixel_adjustment(Kind::HueSaturation).unwrap();
            }
            let initial = editor.tools.show_transform_controls;
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Tool panel view shortcut").size(1500., 900.),
                    editor,
                )
                .unwrap();
            let window = view.window_handle();
            cx.focus(window, 50_000_u64).unwrap();
            cx.simulate_keystrokes(window, "ctrl-h").unwrap();
            cx.read(view, |e| {
                assert_eq!(e.tools.show_transform_controls, !initial);
                assert!(e.floating_panel_kind().is_some());
                assert_eq!(e.session().committed_document(), &doc);
            })
            .unwrap();
            cx.simulate_keystrokes(window, "ctrl-h").unwrap();
            cx.read(view, |e| {
                assert_eq!(e.tools.show_transform_controls, initial)
            })
            .unwrap();
        }
    }

    #[test]
    fn enter_applies_a_valid_form_and_keeps_invalid_values_open() {
        let mut view = Editor::with_test_document();
        view.open_form(Action::New);
        if let Some(Form::Edit { fields, .. }) = &mut view.modal {
            fields[0].1 = "invalid".into();
            fields[1].1 = "24".into();
        }
        let (mut cx, editor) = Application::new()
            .into_test_context(WindowOptions::new("Form Enter").size(1280., 850.), view)
            .unwrap();
        let window = editor.window_handle();
        cx.focus(window, 50_000_u64).unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::Enter, Modifiers::empty()))
            .unwrap();
        cx.read(editor, |e| {
            assert!(matches!(&e.modal, Some(Form::Edit { error, .. }) if !error.is_empty()));
        })
        .unwrap();
        cx.update(editor, |e, cx| {
            if let Some(Form::Edit { fields, .. }) = &mut e.modal {
                fields[0].1 = "32".into();
            }
            cx.invalidate();
        })
        .unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::Enter, Modifiers::empty()))
            .unwrap();
        cx.read(editor, |e| {
            assert!(e.modal.is_none());
            assert_eq!(
                (e.session().document.width, e.session().document.height),
                (32, 24)
            );
        })
        .unwrap();
    }

    #[test]
    fn levels_alt_p_toggles_preview_and_escape_restores_the_original() {
        let mut view = Editor::with_test_document();
        let mut doc = Document::new(2, 2).unwrap();
        compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
        view.tabs = vec![Session::new(doc.clone(), None).into()];
        view.open_pixel_adjustment(Kind::Levels).unwrap();
        let (mut cx, editor) = Application::new()
            .into_test_context(WindowOptions::new("Levels preview").size(1280., 850.), view)
            .unwrap();
        let window = editor.window_handle();
        cx.focus(window, 50_000_u64).unwrap();
        for preview in [false, true] {
            cx.simulate_keystroke(
                window,
                Keystroke::new(Key::Character("p".into()), Modifiers::ALT),
            )
            .unwrap();
            assert_eq!(
                cx.read(editor, |e| e.adjustment_edit.as_ref().unwrap().preview)
                    .unwrap(),
                preview
            );
        }
        cx.simulate_keystroke(window, Keystroke::new(Key::Escape, Modifiers::empty()))
            .unwrap();
        cx.read(editor, |e| {
            assert!(e.modal.is_none());
            assert_eq!(e.session().document, doc);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
    }
    #[test]
    fn hue_text_undo_does_not_navigate_document_history() {
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(Document::new(8, 8).unwrap(), None).into()];
        e.session_mut()
            .edit("Fill", |doc| {
                compositor::edits::fill(doc, [80, 120, 160, 255], false, false)
            })
            .unwrap();
        let committed = e.session().document.clone();
        e.open_pixel_adjustment(Kind::HueSaturation).unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Text undo").size(1280., 900.), e)
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, 50_000_u64).unwrap();
        cx.simulate_keystrokes(window, "ctrl-a").unwrap();
        cx.simulate_input(window, "45").unwrap();
        cx.simulate_keystrokes(window, "ctrl-z").unwrap();
        cx.read(view, |e| {
            assert_eq!(e.session().committed_document(), &committed);
            assert_eq!(e.session().undo_label(), Some("Fill"));
            assert!(matches!(&e.modal, Some(Form::Edit { fields, .. }) if fields[0].1 == "0"));
        })
        .unwrap();
    }
}
