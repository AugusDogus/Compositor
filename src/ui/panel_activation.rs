//! FloatingPanelController hides panels when the application deactivates.
use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Window {
    Editor,
    About,
}

pub(super) enum Activation {
    Active(Window),
    Inactive,
}

impl Default for Activation {
    fn default() -> Self {
        Self::Active(Window::Editor)
    }
}

impl Activation {
    pub(super) fn focus(&mut self, window: Window, focused: bool) {
        if focused {
            *self = Self::Active(window);
        } else if matches!(self, Self::Active(current) if *current == window) {
            *self = Self::Inactive;
        }
    }

    pub(super) fn present(&self, panel: Element) -> Element {
        if matches!(self, Self::Inactive) {
            // Keep the focused text input mounted, including its caret and unfinished draft.
            // Keyboard input returns only after a window reactivates. `invisible` would evict
            // keyboard focus and commit/normalize fields merely because another app opened.
            panel.opacity(0.).accessibility_hidden(true)
        } else {
            panel
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct AboutFocus(pub bool);

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Dialog, WindowOptions};

    #[test]
    fn inactive_panels_reveal_the_canvas_and_retain_position_focus_and_text_drafts() {
        for kind in [
            Some(Kind::Levels),
            Some(Kind::HueSaturation),
            Some(Kind::Exposure),
            None,
        ] {
            let mut editor = Editor::with_test_document();
            editor.tabs[0].set_document(Document::new(8, 8).unwrap(), None);
            compositor::edits::fill(
                &mut editor.session_mut().document,
                [80, 120, 160, 255],
                false,
                false,
            )
            .unwrap();
            let original = editor.session().document.clone();
            let (mut cx, view) = Application::new()
                .font(crate::UI_FONT)
                .into_test_context(
                    WindowOptions::new("Panel activation").size(1500., 900.),
                    editor,
                )
                .unwrap();
            let window = view.window_handle();
            let underlay = cx.capture_screenshot(window).unwrap();
            cx.update(view, |e, cx| {
                e.action(kind.map_or(Action::Color, Action::AdjustPixels), cx)
            })
            .unwrap();
            let field = if kind.is_some() {
                50_000_u64
            } else {
                51_003_u64
            };
            let draft = if kind.is_some() { "0." } else { "12" };
            cx.focus(window, field).unwrap();
            cx.simulate_keystrokes(window, "ctrl-a").unwrap();
            cx.simulate_input(window, draft).unwrap();
            let panel = if kind.is_some() {
                Dialog::new("editor-dialog", true).popover_id()
            } else {
                "color-picker".into()
            };
            let bounds = cx.element_bounds(window, panel).unwrap();
            cx.update(view, |e, cx| e.event(&Event::Focused(false), cx))
                .unwrap();
            let hidden = cx.capture_screenshot(window).unwrap();
            let scale = hidden.width() as f32 / 1500.;
            for y in (bounds.y * scale).ceil() as u32..(bounds.bottom() * scale).floor() as u32 {
                for x in (bounds.x * scale).ceil() as u32..(bounds.right() * scale).floor() as u32 {
                    assert_eq!(
                        hidden.pixel(x, y),
                        underlay.pixel(x, y),
                        "Inactive {kind:?} panel still paints at {x},{y}"
                    );
                }
            }
            assert_eq!(cx.focused(window).unwrap(), Some(field.into()));
            assert_eq!(
                cx.focused_input_value(window).unwrap().as_deref(),
                Some(draft)
            );
            let tree = cx.accessibility_update(window).unwrap();
            assert!(
                !tree
                    .nodes
                    .iter()
                    .any(|(_, node)| node.value() == Some(draft))
            );
            cx.update(view, |e, cx| e.event(&Event::Focused(true), cx))
                .unwrap();
            assert_eq!(cx.element_bounds(window, panel).unwrap(), bounds);
            assert_eq!(
                cx.focused_input_value(window).unwrap().as_deref(),
                Some(draft)
            );
            cx.click(window, "form-cancel").unwrap();
            cx.read(view, |e| {
                assert_eq!(e.session().document, original);
                assert!(e.session().undo_label().is_none());
            })
            .unwrap();
        }
    }

    #[test]
    fn about_keeps_panels_visible_and_project_sheets_remain_visible_when_inactive() {
        let mut editor = Editor::with_test_document();
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [80, 120, 160, 255],
            false,
            false,
        )
        .unwrap();
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Application activation").size(1500., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let shows = |cx: &mut quickgui::TestAppContext, label: &str| {
            cx.accessibility_update(window)
                .unwrap()
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some(label))
        };
        cx.update(view, |e, cx| {
            e.action(Action::AdjustPixels(Kind::HueSaturation), cx);
            e.open_about(cx);
            e.event(&Event::Focused(false), cx);
            cx.dispatch_action(AboutFocus(true));
        })
        .unwrap();
        assert!(shows(&mut cx, "Hue/Saturation"));
        // Native backends may report the editor's blur after About gained focus.
        cx.update(view, |e, cx| e.event(&Event::Focused(false), cx))
            .unwrap();
        assert!(shows(&mut cx, "Hue/Saturation"));
        cx.update(view, |_, cx| cx.dispatch_action(AboutFocus(false)))
            .unwrap();
        assert!(!shows(&mut cx, "Hue/Saturation"));
        cx.update(view, |e, cx| e.event(&Event::Focused(true), cx))
            .unwrap();
        let about = cx.read(view, |e| e.about_window.unwrap()).unwrap();
        cx.simulate_close_requested(about).unwrap();
        assert!(shows(&mut cx, "Hue/Saturation"));
        cx.update(view, |e, cx| e.action(Action::CanvasSize, cx))
            .unwrap();
        cx.update(view, |e, cx| e.event(&Event::Focused(false), cx))
            .unwrap();
        assert!(shows(&mut cx, "Canvas Size"));
        assert!(!shows(&mut cx, "Hue/Saturation"));
        cx.update(view, |e, cx| e.event(&Event::Focused(true), cx))
            .unwrap();
        cx.click(window, "form-cancel").unwrap();
        cx.click(window, "form-cancel").unwrap();
        cx.read(view, |e| {
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
    }
}
