use super::*;

impl Editor {
    pub(super) fn has_document(&self) -> bool {
        self.tabs[self.current].session().is_some()
    }

    pub(super) fn current_document(&self) -> Option<&Document> {
        self.tabs[self.current]
            .session()
            .map(|session| &session.document)
    }

    pub(super) fn add_empty_tab(&mut self) {
        self.tabs.push(ProjectTab::empty(format!(
            "Untitled {}",
            self.next_tab_number
        )));
        self.next_tab_number += 1;
        self.activate_tab(self.tabs.len() - 1);
        self.preview = None;
        self.status = self.tool_hint().into();
    }

    pub(super) fn welcome_view(&mut self, cx: &mut ViewContext<'_, Self>) -> Element {
        // Release workspace builder temporaries before constructing a dialog.
        let root = self.welcome_workspace(cx);
        self.form_overlays(cx, root)
    }

    fn welcome_form(&mut self, cx: &mut ViewContext<'_, Self>) -> Element {
        let valid = self.tabs[self.current]
            .canvas_draft()
            .is_some_and(new_canvas::CanvasDraft::valid);
        let mut dimensions = div().flex_row().items_center().gap(16.);
        if let Some(draft) = self.tabs[self.current].canvas_draft() {
            for (index, label) in ["Width", "Height"].into_iter().enumerate() {
                let id = format!("welcome-dimension-{index}");
                let mut input = Self::text_field(draft.dimensions[index].clone());
                if index == 0 {
                    input = input.auto_focus();
                }
                dimensions = dimensions.child(
                    div()
                        .flex_col()
                        .gap(8.)
                        .flex_1()
                        .min_w(0.)
                        .child(text(label).text_size(12.).line_height(15.).font_medium())
                        .child(
                            div()
                                .flex_row()
                                .items_center()
                                .h(40.)
                                .px(12.)
                                .gap(8.)
                                .rounded(7.)
                                .bg(Color::rgb8(40, 40, 40))
                                .child(
                                    input
                                        .flex_1()
                                        .min_w(0.)
                                        .h(26.)
                                        .p(0.)
                                        .border(0., Color::TRANSPARENT)
                                        .text_input_padding(0.)
                                        .text_size(13.)
                                        .line_height(16.)
                                        .bg(Color::TRANSPARENT)
                                        .on_key_down(cx.key_down_listener(
                                            id.clone(),
                                            |_, event, cx| {
                                                if (event.modifiers - Modifiers::SHIFT)
                                                    == Modifiers::CONTROL
                                                    && let Key::Character(key) = &event.key
                                                    && ["n", "o", "w", "z", "y"].iter().any(
                                                        |command| key.eq_ignore_ascii_case(command),
                                                    )
                                                {
                                                    cx.propagate();
                                                } else {
                                                    cx.stop_propagation();
                                                }
                                            },
                                        ))
                                        .on_input(cx.input_listener(
                                            id.clone(),
                                            move |this, value, cx| {
                                                if let Some(draft) =
                                                    this.tabs[this.current].canvas_draft_mut()
                                                {
                                                    draft.dimensions[index] = value.to_string();
                                                    draft.edited = true;
                                                }
                                                cx.invalidate();
                                            },
                                        ))
                                        .on_submit(cx.submit_listener(id, |this, _, cx| {
                                            this.create_welcome_canvas(cx)
                                        })),
                                )
                                .child(
                                    text("px")
                                        .text_size(13.)
                                        .line_height(16.)
                                        .text_color(Color::rgb8(165, 165, 165)),
                                ),
                        ),
                );
                if index == 0 {
                    dimensions = dimensions.child(Icon::X.element(12.).mt(20.));
                }
            }
        }
        div()
            .w_full()
            .max_w(500.)
            .p(28.)
            .flex_col()
            .gap(24.)
            .child(
                div()
                    .flex_col()
                    .gap(6.)
                    .child(
                        text("New canvas")
                            .text_size(17.)
                            .line_height(22.)
                            .font_semibold(),
                    )
                    .child(
                        text("A blank space for your next composition.")
                            .text_size(13.)
                            .line_height(16.)
                            .text_color(Color::rgb8(165, 165, 165)),
                    ),
            )
            .child(dimensions)
            .child(
                text(if valid {
                    "Transparent canvas · sRGB"
                } else {
                    "Enter whole numbers from 1 to 30,000 pixels."
                })
                .text_size(12.)
                .line_height(15.)
                .text_color(if valid {
                    Color::rgb8(165, 165, 165)
                } else {
                    Color::rgb8(255, 159, 10)
                }),
            )
            .child(
                div()
                    .flex_row()
                    .items_center()
                    .gap(10.)
                    .whitespace_nowrap()
                    .child(
                        self.action_button(cx, 80_001, "Open project", Action::Open)
                            .px(8.),
                    )
                    .child(
                        self.action_button(cx, 80_002, "Import image", Action::Import)
                            .px(8.),
                    )
                    .child(div().flex_1())
                    .child(
                        Self::control("Create canvas")
                            .px(8.)
                            .bg(Color::rgb8(0, 122, 255))
                            .hover(|s| s.bg(Color::rgb8(24, 137, 255)))
                            .disabled(!valid)
                            .disabled_style(|s| {
                                s.bg(Color::rgb8(55, 62, 72))
                                    .text_color(Color::rgb8(139, 139, 139))
                            })
                            .on_click(cx.listener("welcome-create", |this, cx| {
                                this.create_welcome_canvas(cx)
                            })),
                    ),
            )
    }

    fn welcome_canvas(&mut self, cx: &mut ViewContext<'_, Self>) -> Element {
        let panel = self.welcome_form(cx);
        div()
            .id("welcome-canvas")
            .flex_1()
            .min_w(0.)
            .h_full()
            .flex_col()
            .items_center()
            .justify_center()
            .child(panel)
            .on_drop(cx.drop_listener(
                "welcome-canvas",
                |this, drag: &layer_drag::LayerDrag, _, cx| {
                    if !this.pending && this.modal.is_none() {
                        let result = this.copy_drag_to_tab(drag, this.current);
                        this.result(result, cx);
                    }
                },
            ))
    }

    fn welcome_workspace(&mut self, cx: &mut ViewContext<'_, Self>) -> Element {
        let canvas = self.welcome_canvas(cx);
        let content = self.workspace_layout(cx, canvas);
        div()
            .id("workspace")
            .focusable()
            .size_full()
            .flex_col()
            .bg(Color::rgb8(26, 26, 26))
            .text_color(Color::rgb8(224, 224, 224))
            .on_key_down(cx.key_down_listener("workspace", |this, event, cx| {
                if this.picking_color() {
                    if matches!(event.key, Key::Enter | Key::Escape) {
                        let result = this.finish_color(event.key == Key::Enter);
                        this.result(result, cx);
                    } else {
                        cx.propagate();
                    }
                    return;
                }
                if this.floating_panel_kind().is_some() && !this.pending {
                    this.form_key(&event.key, event.modifiers, cx);
                    return;
                }
                if this.modal.is_some() || this.pending {
                    cx.propagate();
                    return;
                }
                this.key(&event.key, event.modifiers, cx);
            }))
            .on_drop(cx.drop_listener(
                "workspace",
                |this, files: &quickgui::DroppedFiles, event, cx| {
                    this.drop_files(files, Some(this.tabs[this.current].id), event, cx);
                },
            ))
            .child(content)
            .child(self.status_bar())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Keystroke, WindowOptions};

    #[test]
    fn welcome_controls_fit_the_minimum_window_with_a_wide_layers_panel() {
        for panel_width in [202., 252., 352.] {
            let mut editor = Editor::new(Vec::new()).unwrap();
            editor.panel_layout.width = panel_width;
            let (mut cx, view) = Application::new()
                .font(crate::UI_FONT)
                .into_test_context(
                    WindowOptions::new("Narrow welcome").size(800., 594.),
                    editor,
                )
                .unwrap();
            let window = view.window_handle();
            let canvas = cx.element_bounds(window, "welcome-canvas").unwrap();
            let frame = cx.capture_screenshot(window).unwrap();
            let scale = frame.width() as f32 / 800.;
            for id in [
                quickgui::ElementId::from("welcome-dimension-0"),
                "welcome-dimension-1".into(),
                "welcome-create".into(),
                80_001_u64.into(),
                80_002_u64.into(),
            ] {
                let bounds = cx.element_bounds(window, id).unwrap();
                assert!(
                    bounds.x >= canvas.x + 28. && bounds.right() <= canvas.right() - 28.,
                    "panel width {panel_width}: {id:?} at {bounds:?} exceeds welcome padding in {canvas:?}"
                );
                assert!(bounds.y >= canvas.y && bounds.bottom() <= canvas.bottom());
                if bounds.height == 24. {
                    let rows: Vec<_> = ((bounds.y * scale) as u32
                        ..(bounds.bottom() * scale) as u32)
                        .filter(|&y| {
                            ((bounds.x * scale) as u32..(bounds.right() * scale) as u32).any(|x| {
                                frame
                                    .pixel(x, y)
                                    .is_some_and(|pixel| pixel[..3].iter().all(|&v| v > 200))
                            })
                        })
                        .collect();
                    assert!(!rows.is_empty());
                    assert!(
                        (rows.last().unwrap() - rows[0] + 1) as f32 <= 16. * scale,
                        "panel width {panel_width}: button text must remain on one line"
                    );
                }
            }
            cx.click(window, "welcome-create").unwrap();
            cx.read(view, |e| {
                assert_eq!(e.session().document.width, 1920);
                assert_eq!(e.session().document.height, 1080);
            })
            .unwrap();
        }
    }

    #[test]
    fn project_shortcuts_work_while_a_welcome_dimension_is_focused() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Welcome project shortcuts").size(1500., 900.),
                Editor::new(Vec::new()).unwrap(),
            )
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, "welcome-dimension-0").unwrap();
        cx.simulate_keystrokes(window, "ctrl-n").unwrap();
        assert_eq!(cx.read(view, |e| e.tabs.len()).unwrap(), 2);
        cx.focus(window, "welcome-dimension-1").unwrap();
        cx.simulate_keystrokes(window, "ctrl-w").unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 1);
            assert!(!e.has_document());
            assert!(e.errors.is_empty());
        })
        .unwrap();
    }

    #[test]
    fn welcome_keeps_editor_chrome_and_tool_settings_without_creating_a_document() {
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .into_test_context(
                WindowOptions::new("Welcome chrome").size(1500., 900.),
                Editor::new(Vec::new()).unwrap(),
            )
            .unwrap();
        let window = view.window_handle();
        assert!(cx.contains_element(window, "brush-tool").unwrap());
        let canvas = cx.element_bounds(window, "welcome-canvas").unwrap();
        assert_eq!(canvas.x, 56.);
        assert_eq!(canvas.y, 116.);
        assert_eq!(canvas.width, 1192.);
        assert!(cx.click(window, 510_u64).is_err());
        assert!(cx.click(window, "layer-add-mask").is_err());
        assert!(cx.click(window, "layer-adjustment-menu").is_err());
        assert!(cx.click(window, "transform-apply").is_err());
        for (tool, _) in Tool::ALL {
            cx.update(view, |e, cx| e.select_tool(tool, cx)).unwrap();
            cx.read(view, |e| {
                assert_eq!(e.tools.tool, tool);
                assert!(!e.has_document());
                assert!(!e.tabs[e.current].dirty());
                assert!(e.tools.pending_crop.is_none());
            })
            .unwrap();
        }
        cx.focus(window, "workspace").unwrap();
        cx.simulate_keystrokes(window, "b").unwrap();
        assert_eq!(cx.read(view, |e| e.tools.tool).unwrap(), Tool::Brush);
        cx.focus(window, "brush-size").unwrap();
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
        )
        .unwrap();
        cx.simulate_input(window, "125").unwrap();
        assert_eq!(cx.read(view, |e| e.tools.brush.diameter).unwrap(), 125.);
        cx.click(window, 320_u64).unwrap();
        assert!(cx.read(view, Editor::picking_color).unwrap());
        cx.simulate_keystrokes(window, "escape").unwrap();
        assert!(!cx.read(view, Editor::picking_color).unwrap());
        assert!(!cx.read(view, Editor::has_document).unwrap());
        cx.click(window, "welcome-create").unwrap();
        assert!(cx.read(view, Editor::has_document).unwrap());
        assert_eq!(cx.read(view, |e| e.tools.brush.diameter).unwrap(), 125.);
    }

    #[test]
    fn empty_startup_can_create_cancel_close_and_keep_the_last_workspace_open() {
        let e = Editor::new(Vec::new()).unwrap();
        assert!(!e.has_document());
        assert!(!e.tabs[0].dirty());
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .into_test_context(WindowOptions::new("Empty workspace").size(1280., 850.), e)
            .unwrap();
        let window = view.window_handle();
        assert!(cx.contains_element(window, "welcome-create").unwrap());
        cx.focus(window, "welcome-dimension-0").unwrap();
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
        )
        .unwrap();
        cx.simulate_input(window, "0").unwrap();
        assert!(cx.click(window, "welcome-create").is_err());
        cx.simulate_keystrokes(window, "enter").unwrap();
        cx.read(view, |e| {
            assert!(e.errors.is_empty());
            assert_eq!(e.tabs[0].canvas_draft().unwrap().dimensions[0], "0");
        })
        .unwrap();
        assert!(!cx.read(view, Editor::has_document).unwrap());
        assert!(cx.read(view, |e| e.modal.is_none()).unwrap());
        cx.focus(window, "workspace").unwrap();
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("n".into()), Modifiers::CONTROL),
        )
        .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 2);
            assert_eq!(e.tabs[1].title(), "Untitled 2");
            assert!(!e.has_document());
            assert!(e.modal.is_none());
        })
        .unwrap();
        for (index, value) in ["32", "24"].into_iter().enumerate() {
            cx.focus(window, format!("welcome-dimension-{index}"))
                .unwrap();
            cx.simulate_keystroke(
                window,
                Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
            )
            .unwrap();
            let keys = value
                .chars()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            cx.simulate_keystrokes(window, &keys).unwrap();
        }
        cx.simulate_keystrokes(window, "enter").unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 2);
            assert_eq!(
                (e.session().document.width, e.session().document.height),
                (32, 24)
            );
        })
        .unwrap();
        cx.update(view, |e, cx| e.action(Action::CloseTab, cx))
            .unwrap();
        cx.click(window, "form-cancel").unwrap();
        assert!(cx.read(view, Editor::has_document).unwrap());
        cx.update(view, |e, cx| e.action(Action::CloseTab, cx))
            .unwrap();
        cx.click(window, "discard-close").unwrap();
        assert!(!cx.read(view, Editor::has_document).unwrap());
        cx.update(view, |e, cx| e.action(Action::CloseTab, cx))
            .unwrap();
        assert!(cx.is_window_open(window));
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 1);
            assert!(!e.has_document());
        })
        .unwrap();
        assert!(cx.simulate_close_requested(window).unwrap());
    }

    #[test]
    fn delayed_paste_initializes_only_its_empty_tab_and_failure_preserves_empty_state() {
        let mut e = Editor::new(Vec::new()).unwrap();
        let destination = e.tabs[0].id;
        e.add_empty_tab();
        let pixels = image::RgbaImage::from_pixel(7, 3, image::Rgba([80, 100, 120, 255]));
        e.paste_pixels(destination, pixels.clone()).unwrap();
        assert!(!e.has_document());
        let doc = &e.tabs[0].session().unwrap().document;
        assert_eq!((doc.width, doc.height), (7, 3));
        assert_eq!(doc.layers.len(), 1);
        assert_eq!(doc.layers[0].transform.origin, [0., 0.]);
        e.tabs.remove(0);
        // Removing a background fixture shifts the current index, not its tools.
        e.current = 0;
        assert!(e.paste_pixels(destination, pixels).is_err());
        assert!(!e.has_document());
        assert!(
            e.tabs[0]
                .edit_or_create(
                    "Failed import",
                    || Document::new(7, 3),
                    |_| Err(compositor::invalid("Import failed"))
                )
                .is_err()
        );
        assert!(!e.has_document());
        assert!(!e.tabs[0].dirty());
    }

    #[test]
    fn pasting_internal_pixels_into_an_empty_tab_fits_the_new_canvas() {
        let mut e = Editor::new(Vec::new()).unwrap();
        let pixels = image::RgbaImage::from_pixel(7, 3, image::Rgba([80, 100, 120, 255]));
        e.pixel_clipboard = Some(compositor::clipboard::PixelClipboard {
            pixels: Arc::new(pixels.clone()),
            origin: [400., -200.],
        });
        e.paste_pixels(e.tabs[0].id, pixels).unwrap();
        assert_eq!(e.session().document.layers[0].transform.origin, [0., 0.]);
    }

    #[test]
    fn new_layer_shortcut_does_not_create_a_project_in_an_empty_tab() {
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .into_test_context(
                WindowOptions::new("Empty shortcuts").size(1280., 850.),
                Editor::new(Vec::new()).unwrap(),
            )
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, "workspace").unwrap();
        cx.simulate_keystroke(
            window,
            Keystroke::new(
                Key::Character("n".into()),
                Modifiers::CONTROL | Modifiers::SHIFT,
            ),
        )
        .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 1);
            assert!(e.modal.is_none());
            assert!(!e.has_document());
        })
        .unwrap();
    }
}
