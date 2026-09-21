//! The Linux menu row owns the window chrome. Move and resize gestures are
//! delegated to the compositor so snapping and desktop policies still apply.
use super::*;
use quickgui::{CursorStyle, MouseButton, ResizeDirection};

fn control(id: &'static str, label: &'static str, glyph: Element) -> Element {
    button()
        .id(id)
        .w(36.)
        .h(24.)
        .p(0.)
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(4.)
        .bg(Color::TRANSPARENT)
        .hover(|s| s.bg(Color::rgb8(62, 62, 62)))
        .focus(controls::focus_outline)
        .accessibility_label(label)
        .tooltip(label)
        .child(glyph)
}

fn maximize_icon(maximized: bool) -> Element {
    let color = Color::rgb8(210, 210, 210);
    let square = div().absolute().size(9., 9.).border(1., color);
    let mut icon = div().relative().size(12., 12.);
    if maximized {
        icon = icon.child(square.clone().left(3.).top(0.));
    }
    icon.child(square.left(0.).top(if maximized { 3. } else { 1. }))
}

impl Editor {
    pub(super) fn window_titlebar(&self, cx: &mut ViewContext<'_, Self>, menu: Element) -> Element {
        let maximized = cx.window_state().maximized;
        let drag = div()
            .id("window-titlebar-drag")
            .flex_1()
            .min_w(24.)
            .h_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.mouse_down_listener("window-titlebar-drag", |this, event, cx| {
                    if event.click_count != 2 {
                        return;
                    }
                    this.window_drag = None;
                    let result = cx.zoom_window();
                    this.result(
                        result.map_err(|error| {
                            compositor::invalid(format!(
                                "Could not move or maximize the window: {error}."
                            ))
                        }),
                        cx,
                    );
                    cx.prevent_default();
                }),
            )
            .on_pointer(
                cx.pointer_listener("window-titlebar-drag", |this, event, cx| {
                    if event.button != MouseButton::Left {
                        return;
                    }
                    match event.phase {
                        quickgui::PointerPhase::Down => this.window_drag = Some(event.position),
                        quickgui::PointerPhase::Up | quickgui::PointerPhase::Cancel => {
                            this.window_drag = None
                        }
                        quickgui::PointerPhase::Move => {
                            if let Some(origin) = this.window_drag
                                && (event.position.x - origin.x)
                                    .abs()
                                    .max((event.position.y - origin.y).abs())
                                    >= 3.
                            {
                                // A stationary click must stay in the app so a second
                                // click can maximize before the WM grabs the pointer.
                                this.window_drag = None;
                                let result = cx.begin_window_move();
                                this.result(
                                    result.map_err(|error| {
                                        compositor::invalid(format!(
                                            "Could not move the window: {error}."
                                        ))
                                    }),
                                    cx,
                                );
                            }
                        }
                    }
                }),
            );
        let minimize = control(
            "window-minimize",
            "Minimize",
            div().w(10.).h(1.).bg(Color::rgb8(210, 210, 210)),
        )
        .on_click(cx.listener("window-minimize", |this, cx| {
            let result = cx.minimize_window();
            this.result(
                result.map_err(|error| {
                    compositor::invalid(format!("Could not minimize the window: {error}."))
                }),
                cx,
            );
        }));
        let maximize = control(
            "window-maximize",
            if maximized {
                "Restore window"
            } else {
                "Maximize"
            },
            maximize_icon(maximized),
        )
        .on_click(cx.listener("window-maximize", |this, cx| {
            let result = cx.zoom_window();
            this.result(
                result.map_err(|error| {
                    compositor::invalid(format!(
                        "Could not maximize or restore the window: {error}."
                    ))
                }),
                cx,
            );
        }));
        let close = control("window-close", "Close window", Icon::X.element(14.))
            .hover(|s| s.bg(Color::rgb8(185, 48, 48)))
            .on_click(cx.listener("window-close", |this, cx| {
                this.request_close(CloseIntent::Window, cx);
                cx.invalidate();
            }));
        div()
            .id("window-titlebar")
            .relative()
            .h(28.)
            .flex_shrink_0()
            .flex_row()
            .items_center()
            .bg(Color::rgb8(36, 36, 36))
            // Center against the window, not the remaining space after the menus.
            // Omit the title in narrow windows where it would collide with the menus.
            .children((cx.size().width >= 960.).then(|| {
                div()
                    .absolute()
                    .inset_0()
                    .items_center()
                    .justify_center()
                    .child(
                        text("Compositor")
                            .id("window-titlebar-title")
                            .text_size(12.)
                            .text_color(Color::rgb8(155, 155, 155)),
                    )
            }))
            .child(menu)
            .child(drag)
            .child(minimize)
            .child(maximize)
            .child(close)
    }

    pub(super) fn window_resize_edges(&self, cx: &mut ViewContext<'_, Self>) -> Vec<Element> {
        let state = cx.window_state();
        if state.maximized || state.fullscreen || !state.resizable {
            return Vec::new();
        }
        let size = cx.size();
        let (w, h, edge, corner) = (size.width, size.height, 4., 8.);
        [
            (
                ResizeDirection::North,
                CursorStyle::ResizeUp,
                [corner, 0., w - 2. * corner, edge],
            ),
            (
                ResizeDirection::South,
                CursorStyle::ResizeDown,
                [corner, h - edge, w - 2. * corner, edge],
            ),
            (
                ResizeDirection::West,
                CursorStyle::ResizeLeft,
                [0., corner, edge, h - 2. * corner],
            ),
            (
                ResizeDirection::East,
                CursorStyle::ResizeRight,
                [w - edge, corner, edge, h - 2. * corner],
            ),
            (
                ResizeDirection::NorthWest,
                CursorStyle::ResizeUpLeftDownRight,
                [0., 0., corner, corner],
            ),
            (
                ResizeDirection::NorthEast,
                CursorStyle::ResizeUpRightDownLeft,
                [w - corner, 0., corner, corner],
            ),
            (
                ResizeDirection::SouthWest,
                CursorStyle::ResizeUpRightDownLeft,
                [0., h - corner, corner, corner],
            ),
            (
                ResizeDirection::SouthEast,
                CursorStyle::ResizeUpLeftDownRight,
                [w - corner, h - corner, corner, corner],
            ),
        ]
        .into_iter()
        .map(|(direction, cursor, [x, y, width, height])| {
            let id = format!("window-resize-{direction:?}");
            div()
                .id(id.clone())
                .absolute()
                .left(x)
                .top(y)
                .size(width.max(0.), height.max(0.))
                .cursor(cursor)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.mouse_down_listener(id, move |this, _, cx| {
                        let result = cx.begin_window_resize(direction);
                        this.result(
                            result.map_err(|error| {
                                compositor::invalid(format!(
                                    "Could not resize the window: {error}."
                                ))
                            }),
                            cx,
                        );
                        cx.prevent_default();
                        cx.stop_propagation();
                    }),
                )
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titlebar_controls_preserve_unsaved_work_and_restore_resize_edges() {
        let mut editor = Editor::with_test_document();
        editor.session_mut().document = Document::new(8, 8).unwrap();
        editor
            .session_mut()
            .edit("Paint", |doc| {
                compositor::edits::fill(doc, [80, 100, 120, 255], false, false)
            })
            .unwrap();
        let original = editor.session().document.clone();
        let (mut cx, view) = quickgui::Application::new()
            .into_test_context(
                quickgui::WindowOptions::new("Titlebar")
                    .size(1000., 700.)
                    .decorations(false),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        for width in [1000., 1920., 1200.] {
            cx.simulate_window_resize(window, quickgui::Size::new(width, 700.))
                .unwrap();
            let title = cx.element_bounds(window, "window-titlebar-title").unwrap();
            assert!((title.x + title.width / 2. - width / 2.).abs() < 1.);
        }
        assert!(
            cx.contains_element(window, "window-resize-SouthEast")
                .unwrap()
        );
        cx.click(window, "window-maximize").unwrap();
        assert!(cx.window_state(window).unwrap().maximized);
        assert!(
            !cx.contains_element(window, "window-resize-SouthEast")
                .unwrap()
        );
        cx.click(window, "window-maximize").unwrap();
        assert!(!cx.window_state(window).unwrap().maximized);
        assert!(
            cx.contains_element(window, "window-resize-SouthEast")
                .unwrap()
        );
        cx.update(view, |editor, cx| {
            editor.pending = true;
            cx.invalidate();
        })
        .unwrap();
        cx.click(window, "window-minimize").unwrap();
        assert!(cx.window_state(window).unwrap().minimized);
        cx.simulate_minimize(window, false).unwrap();
        cx.update(view, |editor, cx| {
            editor.pending = false;
            cx.invalidate();
        })
        .unwrap();
        cx.click(window, "window-close").unwrap();
        assert!(cx.is_window_open(window));
        assert!(cx.contains_element(window, "form-cancel").unwrap());
        cx.click(window, "form-cancel").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
    }
}
