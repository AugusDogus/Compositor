use super::*;
use quickgui::WindowOptions;
use std::sync::OnceLock;

struct About {
    icon: Image,
}

pub(super) fn app_icon() -> Result<Image> {
    static ICON: OnceLock<Image> = OnceLock::new();
    if let Some(icon) = ICON.get() {
        return Ok(icon.clone());
    }
    let icon = Image::decode(include_bytes!(
        "../../Compositor/Assets.xcassets/AppIcon.appiconset/app-icon-256.png"
    ))
    .map_err(|error| {
        compositor::invalid(format!(
            "Could not load the application icon: {error}. Your document is unchanged."
        ))
    })?;
    Ok(ICON.get_or_init(|| icon).clone())
}

impl Editor {
    pub(super) fn open_about(&mut self, cx: &mut EventContext) {
        if let Some(window) = self.about_window {
            cx.focus_window(window);
            return;
        }
        let icon = match app_icon() {
            Ok(icon) => icon,
            Err(error) => {
                self.operation_result(alerts::Operation::About, Err(error), cx);
                return;
            }
        };
        self.about_window = Some(
            cx.open_window(
                WindowOptions::new("About Compositor")
                    .size(360., 330.)
                    .resizable(false)
                    .minimizable(false)
                    .maximizable(false)
                    .icon(icon.clone()),
                About { icon },
            ),
        );
        cx.invalidate();
    }
}

impl View for About {
    fn event(&mut self, event: &Event, cx: &mut EventContext) {
        if let Event::Focused(focused) = event {
            cx.dispatch_action_to_parent(super::panel_activation::AboutFocus(*focused));
        }
        if let Event::KeyDown { key, modifiers, .. } = event {
            let close = (*key == Key::Escape && modifiers.is_empty())
                || (matches!(key, Key::Character(key) if key.eq_ignore_ascii_case("w"))
                    && *modifiers == Modifiers::CONTROL);
            if close {
                cx.prevent_default();
                cx.close_window();
            } else if matches!(key, Key::Character(key) if key.eq_ignore_ascii_case("q"))
                && *modifiers == Modifiers::CONTROL
            {
                cx.prevent_default();
                cx.dispatch_action_to_parent(closing::Quit);
                // Reveal any unsaved-project confirmation in the editor.
                cx.close_window();
            }
        }
    }

    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
            .font_family("Inter Variable")
            .size_full()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(12.)
            .bg(Color::rgb8(36, 36, 36))
            .text_color(Color::rgb8(224, 224, 224))
            .child(quickgui::img(self.icon.clone()).w(96.).h(96.))
            .child(text("Compositor").text_size(22.).font_semibold())
            .child(text(concat!("Version ", env!("CARGO_PKG_VERSION"))).text_size(13.))
            .child(
                text("Copyright © 2026 Wonder Assembly LLC")
                    .text_size(11.)
                    .text_color(Color::rgb8(165, 165, 165)),
            )
            .child(text("MIT License").text_size(11.))
            .child(
                Editor::control("Close")
                    .id("about-close")
                    .auto_focus()
                    .mt(6.)
                    .on_click(cx.listener("about-close", |_, cx| cx.close_window())),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Menubar};

    #[test]
    fn about_is_singleton_and_reopens_after_every_close_method() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("About lifecycle").size(1280., 900.),
                Editor::new(Vec::new()).unwrap(),
            )
            .unwrap();
        let window = view.window_handle();
        for close_method in ["button", "escape", "ctrl-w", "native"] {
            cx.click(window, Menubar::new("application-menu").item_id(7))
                .unwrap();
            let command = cx
                .read(view, |e| e.menus.command_id("About Compositor"))
                .unwrap();
            cx.click(window, command).unwrap();
            let about = cx.read(view, |e| e.about_window.unwrap()).unwrap();
            assert!(cx.is_window_open(about));
            cx.update(view, |e, cx| e.open_about(cx)).unwrap();
            assert_eq!(cx.read(view, |e| e.about_window).unwrap(), Some(about));
            assert_eq!(cx.windows().len(), 2);
            // Native menu dismissal can restore its trigger behind the child.
            cx.focus(window, Menubar::new("application-menu").item_id(7))
                .unwrap();
            match close_method {
                "button" => cx.click(about, "about-close").unwrap(),
                "native" => {
                    cx.simulate_close_requested(about).unwrap();
                }
                key => cx.simulate_keystrokes(about, key).unwrap(),
            }
            assert!(!cx.is_window_open(about));
            assert!(cx.read(view, |e| e.about_window.is_none()).unwrap());
            assert_eq!(cx.windows().len(), 1);
            assert_eq!(cx.focused(window).unwrap(), Some("workspace".into()));
        }
    }

    #[test]
    fn quit_from_about_preserves_unsaved_project_and_allows_cancellation() {
        let mut editor = Editor::with_test_document();
        editor
            .session_mut()
            .edit("Paint", |doc| {
                compositor::edits::fill(doc, [80, 120, 200, 255], false, false)
            })
            .unwrap();
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("About quit").size(1280., 900.), editor)
            .unwrap();
        cx.update(view, |e, cx| e.open_about(cx)).unwrap();
        let about = cx.read(view, |e| e.about_window.unwrap()).unwrap();
        cx.simulate_keystrokes(about, "ctrl-q").unwrap();
        cx.read(view, |e| {
            assert!(e.close_intent.is_some());
            assert_eq!(e.session().document, original);
        })
        .unwrap();
        assert!(!cx.is_window_open(about));
        cx.simulate_keystrokes(view.window_handle(), "escape")
            .unwrap();
        cx.read(view, |e| {
            assert!(e.close_intent.is_none());
            assert_eq!(e.session().document, original);
        })
        .unwrap();
    }

    #[test]
    fn about_preserves_uncommitted_gradient_and_its_cancel() {
        let mut editor = Editor::with_test_document();
        let original = editor.session().document.clone();
        editor.tools.tool = Tool::Gradient;
        editor.begin_gradient([10., 20.]).unwrap();
        editor
            .move_gradient([80., 90.], gradient::Endpoint::End, false)
            .unwrap();
        let preview = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("About draft").size(1280., 900.), editor)
            .unwrap();
        let window = view.window_handle();
        cx.click(window, Menubar::new("application-menu").item_id(7))
            .unwrap();
        let command = cx
            .read(view, |e| e.menus.command_id("About Compositor"))
            .unwrap();
        cx.click(window, command).unwrap();
        let about = cx.read(view, |e| e.about_window.unwrap()).unwrap();
        cx.click(about, "about-close").unwrap();
        cx.read(view, |e| {
            let draft = e.pending_gradient.as_ref().unwrap();
            assert_eq!(draft.start, [10., 20.]);
            assert_eq!(draft.end, [80., 90.]);
            assert_eq!(e.session().document, preview);
        })
        .unwrap();
        cx.simulate_keystrokes(window, "escape").unwrap();
        cx.read(view, |e| {
            assert!(e.pending_gradient.is_none());
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
    }
}
