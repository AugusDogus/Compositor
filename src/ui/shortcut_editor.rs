//! Shortcut recording stays inside its dialog; typing in ordinary fields is unchanged.
use super::keymap::{Chord, Keymap, definitions};
use super::*;
use compositor::invalid;

#[derive(Clone)]
pub(super) struct Draft {
    settings: Keymap,
    query: String,
    recording: Option<String>,
    error: String,
}

impl Draft {
    pub(super) fn is_recording(&self) -> bool {
        self.recording.is_some()
    }
}

impl Editor {
    pub(super) fn open_shortcuts(&mut self, cx: &mut EventContext) {
        if self.modal.is_some() || self.pending {
            return;
        }
        self.modal = Some(Form::Shortcuts(Box::new(Draft {
            settings: self.keymap.clone(),
            query: String::new(),
            recording: None,
            error: String::new(),
        })));
        self.changed(cx);
    }
    pub(crate) fn restore_shortcuts(&mut self) {
        let Some(path) = Self::shortcut_path() else {
            return;
        };
        match Keymap::read(&path) {
            Ok(settings) => self.keymap = settings,
            Err(error) => self.report_startup_error(format!("Could not restore keyboard shortcuts: {error} Default shortcuts remain active; open Keyboard Shortcuts and save to replace the invalid settings.")),
        }
    }
    fn shortcut_path() -> Option<std::path::PathBuf> {
        super::update_preferences::path().map(|path| path.with_file_name("keyboard-shortcuts.json"))
    }
    fn save_shortcuts(&mut self) -> Result<()> {
        let Some(Form::Shortcuts(draft)) = &self.modal else {
            return Ok(());
        };
        let path = Self::shortcut_path().ok_or_else(|| invalid("Shortcut settings cannot be saved because no configuration directory is available."))?;
        draft.settings.save(&path)?;
        self.keymap = draft.settings.clone();
        self.modal = None;
        self.menus.close();
        Ok(())
    }
    fn record_shortcut(&mut self, key: &Key, modifiers: Modifiers, cx: &mut EventContext) {
        let Some(Form::Shortcuts(draft)) = &mut self.modal else {
            return;
        };
        let Some(id) = draft.recording.clone() else {
            cx.propagate();
            return;
        };
        cx.prevent_default();
        cx.stop_propagation();
        if *key == Key::Escape && modifiers.is_empty() {
            draft.recording = None;
            cx.invalidate();
            return;
        }
        if let Some(chord) = Chord::from_event(key, modifiers) {
            if let Some(definition) = definitions().iter().find(|d| d.id == id) {
                draft.settings.set(definition, chord);
            }
            draft.recording = None;
            draft.error = draft
                .settings
                .validate()
                .err()
                .map_or_else(String::new, |error| error.to_string());
        }
        cx.invalidate();
    }
    fn shortcut_rows(&self, cx: &mut ViewContext<'_, Self>, draft: &Draft) -> Element {
        let mut rows = div().w_full().flex_col().gap(5.);
        let query = draft.query.to_lowercase();
        let mut group = "";
        for definition in definitions().into_iter().filter(|definition| {
            definition.title.to_lowercase().contains(&query)
                || definition.group.to_lowercase().contains(&query)
        }) {
            if definition.group != group {
                group = definition.group;
                rows = rows.child(text(group).font_semibold().mt(12.).mb(5.));
            }
            let id = definition.id.clone();
            let recording = draft.recording.as_ref() == Some(&id);
            let chord = draft.settings.chord(&definition).label();
            let key = format!("shortcut-record-{id}");
            let click_id = id.clone();
            let control = Self::control(if recording {
                "Press a key…".into()
            } else {
                chord
            })
            .id(key.clone())
            .w(165.)
            .selected(recording)
            .on_click(cx.listener(key.clone(), move |this, cx| {
                if let Some(Form::Shortcuts(draft)) = &mut this.modal {
                    draft.recording = Some(click_id.clone());
                }
                cx.focus(quickgui::FocusHandle::new(format!(
                    "shortcut-record-{click_id}"
                )));
                cx.invalidate();
            }))
            .on_key_down(cx.key_down_listener(key, |this, event, cx| {
                this.record_shortcut(&event.key, event.modifiers, cx)
            }));
            let reset = format!("shortcut-reset-{id}");
            let row = div()
                .w_full()
                .flex_row()
                .items_center()
                .gap(10.)
                .child(
                    text(definition.title.clone())
                        .text_size(12.)
                        .flex_1()
                        .min_w(0.)
                        .wrap(),
                )
                .child(control)
                .child(Self::segment("Reset", false).on_click(cx.listener(
                    reset,
                    move |this, cx| {
                        if let Some(Form::Shortcuts(draft)) = &mut this.modal {
                            draft.settings.set(&definition, definition.original.clone());
                            draft.recording = None;
                            draft.error = draft
                                .settings
                                .validate()
                                .err()
                                .map_or_else(String::new, |error| error.to_string());
                        }
                        cx.invalidate();
                    },
                )));
            rows = rows.child(row);
        }
        rows
    }
    pub(super) fn shortcuts_view(
        &mut self,
        cx: &mut ViewContext<'_, Self>,
        draft: &Draft,
    ) -> Element {
        let dialog = quickgui::Dialog::new("editor-dialog", true)
            .initial_focus("shortcut-search")
            .restore_focus_to("workspace")
            .dismiss_on_backdrop(false);
        let rows = self.shortcut_rows(cx, draft);
        let mut contents = div()
            .flex_col()
            .gap(14.)
            .p(22.)
            .rounded(10.)
            .bg(Color::rgb8(43, 43, 43))
            .w(700_f32.min((cx.size().width - 40.).max(320.)))
            .max_h((cx.size().height - 60.).max(300.));
        contents = contents
            .child(text("Keyboard Shortcuts").text_size(17.).font_semibold())
            .child(
                text("Click a shortcut, then press its replacement. Escape cancels recording.")
                    .text_size(12.)
                    .wrap(),
            )
            .child(
                Self::text_field(draft.query.clone())
                    .id("shortcut-search")
                    .placeholder("Search commands")
                    .w_full()
                    .on_input(cx.input_listener("shortcut-search", |this, value, cx| {
                        if let Some(Form::Shortcuts(draft)) = &mut this.modal {
                            draft.query = value.into();
                            draft.recording = None;
                        }
                        cx.invalidate();
                    })),
            )
            .child(
                div()
                    .id("shortcut-list")
                    .w_full()
                    .h((cx.size().height - 280.).clamp(120., 470.))
                    .flex_shrink_0()
                    .overflow_y_scroll()
                    .child(rows),
            );
        if !draft.error.is_empty() {
            contents = contents.child(
                text(draft.error.clone())
                    .text_size(12.)
                    .text_color(Color::rgb8(255, 140, 125))
                    .wrap(),
            );
        }
        let buttons = div()
            .flex_row()
            .gap(8.)
            .child(Self::segment("Reset all", false).on_click(cx.listener(
                "shortcuts-reset-all",
                |this, cx| {
                    if let Some(Form::Shortcuts(draft)) = &mut this.modal {
                        draft.settings = Keymap::default();
                        draft.recording = None;
                        draft.error.clear();
                    }
                    cx.invalidate();
                },
            )))
            .child(div().flex_1())
            .child(
                Self::segment("Cancel", false)
                    .on_click(cx.listener("shortcuts-cancel", |this, cx| this.cancel_form(cx))),
            )
            .child(
                Self::segment("Save", true)
                    .disabled(draft.settings.validate().is_err())
                    .on_click(cx.listener("shortcuts-save", |this, cx| {
                        if let Err(error) = this.save_shortcuts()
                            && let Some(Form::Shortcuts(draft)) = &mut this.modal
                        {
                            draft.error = error.to_string();
                        }
                        this.changed(cx);
                    })),
            );
        self.mount_form(
            cx,
            dialog,
            contents.child(buttons),
            700.,
            "Keyboard Shortcuts",
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};
    #[test]
    fn shortcut_dialog_records_cancels_and_remapped_tools_dispatch() {
        let editor = Editor::with_test_document();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Shortcuts").size(1400., 1000.), editor)
            .unwrap();
        let window = view.window_handle();
        cx.update(view, |editor, cx| editor.open_shortcuts(cx))
            .unwrap();
        assert!(cx.element_bounds(window, "shortcut-search").is_ok());
        assert!(cx.element_bounds(window, "shortcut-list").unwrap().height >= 300.);

        cx.update(view, |editor, cx| {
            if let Some(Form::Shortcuts(draft)) = &mut editor.modal {
                draft.recording = Some("Canvas:Brush".into());
            }
            editor.record_shortcut(&Key::Function(2), Modifiers::empty(), cx);
            if let Some(Form::Shortcuts(draft)) = &editor.modal {
                assert!(draft.settings.validate().is_ok());
                editor.keymap = draft.settings.clone();
            }
            editor.cancel_form(cx);
        })
        .unwrap();
        cx.focus(window, "workspace").unwrap();
        cx.simulate_keystrokes(window, "v f2").unwrap();
        assert_eq!(
            cx.read(view, |editor| editor.tools.tool).unwrap(),
            Tool::Brush
        );
        cx.simulate_keystrokes(window, "v b").unwrap();
        assert_eq!(
            cx.read(view, |editor| editor.tools.tool).unwrap(),
            Tool::Move
        );
    }
}
