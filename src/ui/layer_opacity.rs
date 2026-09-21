//! LayerAppearanceControls.swift's percentage field, committed on leaving the field.
use super::*;
use compositor::invalid;
use uuid::Uuid;

const FIELD: &str = "layer-opacity-value";

pub(super) struct OpacityDraft {
    session: Uuid,
    layer: Uuid,
    text: String,
}

impl Editor {
    pub(super) fn finish_opacity_input(&mut self) -> Result<()> {
        let Some(draft) = self.opacity_draft.take() else {
            return Ok(());
        };
        let value = draft
            .text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| {
                invalid("Enter a finite opacity percentage. The previous opacity is preserved.")
            })?;
        let session = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == draft.session)
            .and_then(ProjectTab::session_mut)
            .ok_or_else(|| {
                invalid("The opacity field's project has closed. No opacity was changed.")
            })?;
        session.edit("Layer Opacity", |doc| {
            let layer = doc
                .layers
                .iter_mut()
                .find(|l| l.id == draft.layer && !l.is_group())
                .ok_or_else(|| {
                    invalid("The layer being edited was removed. No opacity was changed.")
                })?;
            layer.opacity = value.clamp(0., 100.) / 100.;
            Ok(())
        })
    }

    pub(super) fn sync_opacity_input(&mut self, cx: &ViewContext<'_, Self>) {
        if self.opacity_draft.is_some()
            && !cx.is_focused(quickgui::FocusHandle::new(FIELD))
            && let Err(error) = self.finish_opacity_input()
        {
            self.status = error.to_string();
        }
    }

    pub(super) fn layer_opacity_input(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let doc = self.current_document();
        let value = self
            .opacity_draft
            .as_ref()
            .filter(|draft| {
                draft.session == self.tabs[self.current].id
                    && doc.is_some_and(|doc| Some(draft.layer) == doc.active)
            })
            .map(|draft| draft.text.clone())
            .unwrap_or_else(|| {
                format!(
                    "{:.0}",
                    doc.and_then(Document::active_layer)
                        .map_or(100., |l| l.opacity * 100.)
                )
            });
        Self::text_field(value)
            .id(FIELD)
            .accessibility_label("Opacity percent")
            .w(44.)
            .h(26.)
            .text_input_padding(5.)
            .rounded(5.)
            .text_size(13.)
            .bg(Color::rgb8(29, 29, 29))
            .border(1., Color::rgb8(72, 72, 72))
            .disabled(!self.can_edit_appearance())
            .on_input(cx.input_listener(FIELD, |this, value, cx| {
                if !this.can_edit_appearance() {
                    return;
                }
                if this.opacity_draft.is_none() {
                    if let Err(error) = this.finish_pending_edits() {
                        this.result(Err(error), cx);
                        return;
                    }
                    if let Some(layer) = this.session().document.active {
                        this.opacity_draft = Some(OpacityDraft {
                            session: this.session().id,
                            layer,
                            text: value.into(),
                        });
                    }
                } else if let Some(draft) = &mut this.opacity_draft {
                    draft.text = value.into();
                }
                cx.invalidate();
            }))
            .on_key_down(cx.key_down_listener(FIELD, |this, event, cx| {
                match event.key {
                    Key::Enter | Key::Escape => {
                        // Swift's onSubmit and onExitCommand both apply the typed percentage.
                        let result = this.finish_opacity_input();
                        cx.focus(quickgui::FocusHandle::new("workspace"));
                        this.result(result, cx);
                        cx.prevent_default();
                        cx.stop_propagation();
                    }
                    Key::ArrowUp | Key::ArrowDown if this.can_edit_appearance() => {
                        let amount = if event.modifiers.contains(Modifiers::SHIFT) {
                            10.
                        } else {
                            1.
                        };
                        this.opacity_draft = None;
                        let result = this.session_mut().edit("Layer Opacity", |doc| {
                            if let Some(layer) = doc.active_layer_mut() {
                                let percent = (layer.opacity * 100.).round()
                                    + if event.key == Key::ArrowUp {
                                        amount
                                    } else {
                                        -amount
                                    };
                                layer.opacity = percent.clamp(0., 100.) / 100.;
                            }
                            Ok(())
                        });
                        this.result(result, cx);
                        cx.prevent_default();
                        cx.stop_propagation();
                    }
                    _ => cx.stop_propagation(),
                }
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Keystroke, WindowOptions};

    #[test]
    fn typed_percentage_commits_on_enter_escape_and_blur_with_one_undo_step() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Opacity field").size(1280., 900.),
                Editor::with_test_document(),
            )
            .unwrap();
        let window = view.window_handle();
        for (percent, key) in [(25., Key::Enter), (40., Key::Escape), (75., Key::Tab)] {
            let original = cx.read(view, |e| e.session().document.clone()).unwrap();
            cx.focus(window, FIELD).unwrap();
            cx.simulate_keystroke(
                window,
                Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
            )
            .unwrap();
            cx.simulate_input(window, &percent.to_string()).unwrap();
            cx.read(view, |e| {
                assert_eq!(e.session().document, original);
                assert!(e.modal.is_none());
            })
            .unwrap();
            cx.simulate_keystroke(window, Keystroke::new(key, Modifiers::empty()))
                .unwrap();
            cx.read(view, |e| {
                assert_eq!(
                    e.session().document.active_layer().unwrap().opacity,
                    percent / 100.
                );
                assert_eq!(e.session().undo_label(), Some("Layer Opacity"));
            })
            .unwrap();
            cx.update(view, |e, cx| {
                e.session_mut().undo();
                cx.invalidate();
            })
            .unwrap();
            assert_eq!(
                cx.read(view, |e| e.session().document.clone()).unwrap(),
                original
            );
        }
    }

    #[test]
    fn opacity_arrows_clamp_and_invalid_text_preserves_the_layer() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Opacity keys").size(1280., 900.),
                Editor::with_test_document(),
            )
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, FIELD).unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::ArrowDown, Modifiers::SHIFT))
            .unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::ArrowDown, Modifiers::empty()))
            .unwrap();
        assert_eq!(
            cx.read(view, |e| e
                .session()
                .document
                .active_layer()
                .unwrap()
                .opacity)
                .unwrap(),
            0.89
        );
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
        )
        .unwrap();
        cx.simulate_input(window, "NaN").unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::Enter, Modifiers::empty()))
            .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.session().document.active_layer().unwrap().opacity, 0.89);
            assert!(e.status.contains("previous opacity is preserved"));
        })
        .unwrap();
        cx.focus(window, FIELD).unwrap();
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
        )
        .unwrap();
        cx.simulate_input(window, "150").unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::Enter, Modifiers::empty()))
            .unwrap();
        assert_eq!(
            cx.read(view, |e| e
                .session()
                .document
                .active_layer()
                .unwrap()
                .opacity)
                .unwrap(),
            1.
        );
    }
}
