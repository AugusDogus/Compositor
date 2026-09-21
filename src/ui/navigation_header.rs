//! NavigationToolHeader.swift: a draft percentage applies on submit, exit, or blur.
use super::*;

const FIELD: &str = "zoom-percent";

pub(super) struct ZoomDraft {
    session: uuid::Uuid,
    displayed: String,
    text: String,
}

fn percentage(zoom: f64) -> String {
    format!("{:.2}", zoom * 100.)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .into()
}

impl Editor {
    pub(super) fn finish_zoom_input(&mut self) {
        let Some(draft) = self.zoom_draft.take() else {
            return;
        };
        if draft.text == draft.displayed || self.pending {
            return;
        }
        let value = draft.text.trim().replace('%', "").parse::<f64>();
        if let Ok(value) = value
            && value.is_finite()
            && value > 0.
            && let Some(session) = self
                .tabs
                .iter_mut()
                .find(|tab| tab.id == draft.session)
                .and_then(ProjectTab::session_mut)
        {
            session.zoom_at(value / 100., [0., 0.]);
        }
    }

    pub(super) fn sync_zoom_input(&mut self, cx: &ViewContext<'_, Self>) {
        if self.zoom_draft.is_some()
            && (!cx.is_focused(quickgui::FocusHandle::new(FIELD)) || self.tools.tool != Tool::Zoom)
        {
            self.finish_zoom_input();
        }
    }

    pub(super) fn navigation_header(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let mut bar = div()
            .h(42.)
            .flex_shrink_0()
            .flex_row()
            .items_center()
            .px(18.)
            .gap(12.)
            .bg(Color::rgb8(38, 38, 38))
            .child(
                text(if self.tools.tool == Tool::Hand {
                    "Pan"
                } else {
                    "Zoom"
                })
                .text_size(13.)
                .line_height(16.)
                .font_semibold(),
            );
        if self.tools.tool == Tool::Zoom {
            let value = self
                .zoom_draft
                .as_ref()
                .filter(|draft| draft.session == self.tabs[self.current].id)
                .map_or_else(
                    || {
                        percentage(
                            self.tabs[self.current]
                                .session()
                                .map_or(1., |session| session.zoom),
                        )
                    },
                    |draft| draft.text.clone(),
                );
            let field = Self::text_field(value)
                .id(FIELD)
                .w(72.)
                .h(24.)
                .text_input_padding(5.)
                .rounded(5.)
                .text_size(12.)
                .line_height(15.)
                .text_right()
                .bg(Color::rgb8(29, 29, 29))
                .border(1., Color::rgb8(67, 67, 67))
                .accessibility_label("Zoom percentage")
                .tooltip("Zoom percentage (0.1–3200%). Press Return to apply.")
                .disabled(self.pending || !self.has_document())
                .on_input(cx.input_listener(FIELD, |this, value, cx| {
                    if this.pending || !this.has_document() {
                        return;
                    }
                    if let Some(draft) = &mut this.zoom_draft {
                        draft.text = value.into();
                    } else {
                        this.zoom_draft = Some(ZoomDraft {
                            session: this.session().id,
                            displayed: percentage(this.session().zoom),
                            text: value.into(),
                        });
                    }
                    cx.invalidate();
                }))
                .on_key_down(cx.key_down_listener(FIELD, |this, event, cx| {
                    match event.key {
                        Key::Enter | Key::Escape => {
                            this.finish_zoom_input();
                            cx.focus(quickgui::FocusHandle::new("workspace"));
                            cx.prevent_default();
                        }
                        Key::ArrowUp | Key::ArrowDown if !this.pending && this.has_document() => {
                            let value = this
                                .zoom_draft
                                .as_ref()
                                .and_then(|draft| {
                                    draft
                                        .text
                                        .chars()
                                        .filter(|c| c.is_ascii_digit() || *c == '.')
                                        .collect::<String>()
                                        .parse::<f64>()
                                        .ok()
                                })
                                .unwrap_or(this.session().zoom * 100.);
                            let step = if event.modifiers.contains(Modifiers::SHIFT) {
                                10.
                            } else {
                                1.
                            };
                            let value = (value
                                + if event.key == Key::ArrowUp {
                                    step
                                } else {
                                    -step
                                })
                            .clamp(0.1, 3200.);
                            this.zoom_draft = None;
                            this.session_mut().zoom_at(value / 100., [0., 0.]);
                            cx.prevent_default();
                        }
                        _ => {}
                    }
                    cx.stop_propagation();
                    cx.invalidate();
                }));
            bar = bar.child(Self::unit_suffix(field, "%"));
        }
        bar.child(div().flex_1())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Keystroke, WindowOptions};

    #[test]
    fn zoom_drafts_apply_on_enter_escape_and_blur_without_editing_the_document() {
        let mut editor = Editor::with_test_document();
        editor.tools.tool = Tool::Zoom;
        editor.session_mut().zoom_at(1.234567, [0., 0.]);
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Zoom input").size(1280., 850.), editor)
            .unwrap();
        let window = view.window_handle();
        for (input, key, expected) in [
            ("123.46", Key::Enter, 1.234567),
            ("150%", Key::Escape, 1.5),
            ("50", Key::Tab, 0.5),
            ("NaN", Key::Enter, 0.5),
            ("0", Key::Enter, 0.5),
            ("4000", Key::Enter, 32.),
            ("0.01", Key::Enter, 0.001),
        ] {
            let before = cx.read(view, |e| e.session().zoom).unwrap();
            cx.focus(window, FIELD).unwrap();
            cx.simulate_keystroke(
                window,
                Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
            )
            .unwrap();
            cx.simulate_input(window, input).unwrap();
            assert_eq!(cx.read(view, |e| e.session().zoom).unwrap(), before);
            cx.simulate_keystroke(window, Keystroke::new(key, Modifiers::empty()))
                .unwrap();
            assert_eq!(cx.read(view, |e| e.session().zoom).unwrap(), expected);
        }
        cx.focus(window, FIELD).unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::ArrowUp, Modifiers::SHIFT))
            .unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::ArrowDown, Modifiers::empty()))
            .unwrap();
        cx.read(view, |e| {
            assert!((e.session().zoom - 0.091).abs() < 1e-12);
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
    }
}
