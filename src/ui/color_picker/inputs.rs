//! ColorPickerSheet.swift commits hex on submit/blur and clamps integer RGB entry.
use super::*;

impl Editor {
    pub(in crate::ui) fn sync_picker_input(&mut self, cx: &ViewContext<'_, Self>) {
        let hex_focused = cx.is_focused(quickgui::FocusHandle::new(51_003_u64));
        let commit_hex = self.picker_mut().is_some_and(|picker| {
            let draft = picker.current_mut();
            let [r, g, b, _] = draft.hsb.rgb();
            for (index, value) in [r, g, b].into_iter().enumerate() {
                if !cx.is_focused(quickgui::FocusHandle::new(51_000 + index as u64)) {
                    draft.rgb[index] = value.to_string();
                }
            }
            if hex_focused && draft.hex_draft.is_none() {
                draft.hex_draft = Some(draft.hex());
            }
            !hex_focused && draft.hex_draft.is_some()
        });
        if commit_hex {
            self.commit_picker_hex();
            if let Err(error) = self.preview_picker() {
                self.status = error.to_string();
            }
        }
    }

    pub(in crate::ui) fn commit_picker_hex(&mut self) {
        let Some(picker) = self.picker_mut() else {
            return;
        };
        let draft = picker.current_mut();
        if let Some(text) = draft.hex_draft.take() {
            if let Ok(rgb) = parse_hex(&text) {
                draft.hsb.set_rgb(rgb);
            }
            // Invalid text restores the current color, as the source picker does.
            draft.sync();
        }
    }

    pub(super) fn picker_input(&mut self, channel: Option<usize>, value: &str) {
        if channel.is_some() {
            self.commit_picker_hex();
        }
        let Some(picker) = self.picker_mut() else {
            return;
        };
        let draft = picker.current_mut();
        if let Some(channel) = channel {
            draft.rgb[channel] = value.into();
            if let Ok(value) = value.trim().parse::<i64>() {
                let mut rgb = draft.hsb.rgb();
                rgb[channel] = value.clamp(0, 255) as u8;
                draft.hsb.set_rgb(rgb);
                draft.sync();
            }
        } else {
            draft.hex_draft = Some(value.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Keystroke, WindowOptions};

    #[test]
    fn hex_submit_keeps_picker_open_invalid_hex_restores_color_and_rgb_clamps() {
        let mut editor = Editor::with_test_document();
        let original = editor.session().document.clone();
        editor.tools.brush.color = [100, 120, 140, 255];
        editor.open_form(Action::Color);
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Picker inputs").size(1280., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        for (id, input, expected) in [
            (51_003_u64, "#0f0", [0, 255, 0, 255]),
            (51_003, "invalid", [0, 255, 0, 255]),
            (51_000, "-25", [0, 255, 0, 255]),
            (51_002, "300", [0, 255, 255, 255]),
            (51_000, "250", [250, 255, 255, 255]),
            (51_000, "oops", [250, 255, 255, 255]),
        ] {
            cx.focus(window, id).unwrap();
            cx.simulate_keystroke(
                window,
                Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
            )
            .unwrap();
            cx.simulate_input(window, input).unwrap();
            cx.simulate_keystroke(
                window,
                Keystroke::new(
                    if id == 51_003 { Key::Enter } else { Key::Tab },
                    Modifiers::empty(),
                ),
            )
            .unwrap();
            cx.read(view, |e| {
                let Some(Form::Color(picker)) = &e.modal else {
                    panic!("Picker closed while editing a field")
                };
                assert_eq!(picker.current().hsb.rgb(), expected);
                assert_eq!(e.tools.brush.color, [100, 120, 140, 255]);
                assert_eq!(e.session().document, original);
            })
            .unwrap();
        }
        cx.focus(window, 51_003_u64).unwrap();
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
        )
        .unwrap();
        cx.simulate_input(window, "#f00").unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::Escape, Modifiers::empty()))
            .unwrap();
        cx.read(view, |e| {
            assert!(e.modal.is_none());
            assert_eq!(e.tools.brush.color, [100, 120, 140, 255]);
            assert_eq!(e.session().document, original);
        })
        .unwrap();
    }
}
