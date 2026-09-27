use super::*;
use compositor::invalid;
use uuid::Uuid;

pub(super) struct Draft {
    path: Uuid,
    session: Uuid,
    name: String,
}

impl Editor {
    fn begin_path_rename(&mut self) -> Result<()> {
        if !self.can_edit_layers() {
            return Err(invalid("Finish the current edit before renaming a path."));
        }
        let path = self
            .active_path()
            .ok_or_else(|| invalid("Select a saved path to rename."))?;
        self.tools.paths.rename = Some(Draft {
            path: path.id,
            session: self.session().id,
            name: path.name.clone(),
        });
        Ok(())
    }

    pub(super) fn cancel_path_rename(&mut self) {
        self.tools.paths.rename = None;
    }

    pub(super) fn sync_path_rename(&mut self) {
        let stale = self.tools.paths.rename.as_ref().is_some_and(|draft| {
            self.tools.tool != Tool::Pen
                || !self.has_document()
                || self.session().id != draft.session
                || self.active_path().is_none_or(|path| path.id != draft.path)
        });
        if stale {
            self.cancel_path_rename();
        }
    }

    fn finish_path_rename(&mut self) -> Result<()> {
        let Some(draft) = &self.tools.paths.rename else {
            return Ok(());
        };
        if !self.can_edit_layers() {
            return Err(invalid(
                "Finish the current edit before saving the path name. The draft is preserved.",
            ));
        }
        if self.session().id != draft.session {
            return Err(invalid(
                "This name belongs to another project. Cancel and select a path in the current project.",
            ));
        }
        let id = draft.path;
        let name = draft.name.trim().to_owned();
        if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
            return Err(invalid(
                "Enter a path name of 1 to 256 bytes without control characters. The original name is unchanged.",
            ));
        }
        self.session_mut().edit("Rename Path", |document| {
            let path = super::paths::path_mut(document, id)?;
            path.name = name;
            path.validate()
        })?;
        self.cancel_path_rename();
        Ok(())
    }

    pub(super) fn path_rename_button(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(draft) = &self.tools.paths.rename else {
            return self
                .tool_header_control("Rename")
                .id("path-rename")
                .flex_shrink_0()
                .disabled(!self.can_edit_layers() || self.active_path().is_none())
                .on_click(cx.listener("path-rename", |editor, cx| {
                    let result = editor.begin_path_rename();
                    if result.is_ok() {
                        cx.focus(quickgui::FocusHandle::new("path-name"));
                    }
                    editor.result(result, cx);
                }));
        };
        let input = self
            .text_field(draft.name.clone())
            .id("path-name")
            .auto_focus()
            .w(140.)
            .h(24.)
            .flex_shrink_0()
            .text_size(12.)
            .text_input_padding(5.)
            .on_input(cx.input_listener("path-name", |editor, value, cx| {
                if let Some(draft) = &mut editor.tools.paths.rename {
                    draft.name = value.to_owned();
                }
                editor.changed(cx);
            }))
            .on_key_down(cx.key_down_listener("path-name", |editor, event, cx| {
                // Text editing keys must not reach Pen's anchor shortcuts.
                cx.stop_propagation();
                match event.key {
                    Key::Enter => {
                        cx.prevent_default();
                        editor.submit_path_name(cx);
                    }
                    Key::Escape => {
                        cx.prevent_default();
                        editor.dismiss_path_name(cx);
                    }
                    _ => {}
                }
            }));
        div()
            .flex_row()
            .items_center()
            .gap(4.)
            .flex_shrink_0()
            .child(input)
            .child(
                self.tool_header_control("Save")
                    .id("path-name-save")
                    .disabled(!self.can_edit_layers())
                    .on_click(
                        cx.listener("path-name-save", |editor, cx| editor.submit_path_name(cx)),
                    ),
            )
            .child(
                self.tool_header_control("Cancel")
                    .id("path-name-cancel")
                    .on_click(cx.listener("path-name-cancel", |editor, cx| {
                        editor.dismiss_path_name(cx)
                    })),
            )
    }

    fn submit_path_name(&mut self, cx: &mut EventContext) {
        let result = self.finish_path_rename();
        if result.is_ok() {
            cx.focus(quickgui::FocusHandle::new("workspace"));
        }
        self.result(result, cx);
    }

    fn dismiss_path_name(&mut self, cx: &mut EventContext) {
        self.cancel_path_rename();
        cx.focus(quickgui::FocusHandle::new("workspace"));
        self.changed(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::vector_path::{Anchor, BezierPath, Closure, SavedPath};

    fn editor() -> Editor {
        let mut editor = Editor::with_test_document();
        let mut document = Document::new(32, 32).unwrap();
        let path = SavedPath::new(
            "Outline",
            BezierPath {
                anchors: vec![Anchor::corner([2., 2.]), Anchor::corner([20., 20.])],
                closure: Closure::Open,
            },
        )
        .unwrap();
        let id = path.id;
        document.paths.push(path);
        editor.tabs = vec![Session::new(document, None).into()];
        editor.tools.tool = Tool::Pen;
        editor.tools.paths.active = Some(super::super::paths::Active {
            target: super::super::path_target::Target::Saved(id),
            selected: Some(0),
            mode: super::super::paths::Mode::Editing,
        });
        editor
    }

    #[test]
    fn rename_commits_once_and_cancel_keeps_the_original_name() {
        let mut editor = editor();
        editor.begin_path_rename().unwrap();
        editor.tools.paths.rename.as_mut().unwrap().name = "  Subject  ".into();
        assert_eq!(editor.session().document.paths[0].name, "Outline");
        assert!(editor.session().undo_label().is_none());
        editor.finish_path_rename().unwrap();
        assert_eq!(editor.session().document.paths[0].name, "Subject");
        assert_eq!(editor.session().undo_label(), Some("Rename Path"));
        editor.session_mut().undo();
        assert_eq!(editor.session().document.paths[0].name, "Outline");
        assert!(editor.session().undo_label().is_none());
        editor.begin_path_rename().unwrap();
        editor.tools.paths.rename.as_mut().unwrap().name = "Discard".into();
        editor.cancel_path_rename();
        assert_eq!(editor.session().document.paths[0].name, "Outline");
        assert!(editor.session().undo_label().is_none());
    }

    #[test]
    fn invalid_and_stale_names_preserve_document_and_draft() {
        let mut editor = editor();
        editor.begin_path_rename().unwrap();
        for name in [" ".to_owned(), "é".repeat(129), "hello\nworld".into()] {
            editor.tools.paths.rename.as_mut().unwrap().name = name.clone();
            assert!(editor.finish_path_rename().is_err());
            assert_eq!(editor.tools.paths.rename.as_ref().unwrap().name, name);
            assert_eq!(editor.session().document.paths[0].name, "Outline");
            assert!(editor.session().undo_label().is_none());
        }
        editor.tools.paths.rename.as_mut().unwrap().name = "é".repeat(128);
        editor.finish_path_rename().unwrap();
        assert_eq!(editor.session().document.paths[0].name.len(), 256);
        editor.begin_path_rename().unwrap();
        editor.tools.paths.rename.as_mut().unwrap().name = "Stale".into();
        editor.tools.paths.rename.as_mut().unwrap().session = Uuid::new_v4();
        assert!(editor.finish_path_rename().is_err());
        assert_eq!(editor.session().document.paths[0].name.len(), 256);
    }

    #[test]
    fn drafts_follow_their_tab_and_disappear_when_tool_or_target_changes() {
        let mut editor = editor();
        editor.begin_path_rename().unwrap();
        editor.tools.paths.rename.as_mut().unwrap().name = "Draft".into();
        editor
            .tabs
            .push(Session::new(Document::new(8, 8).unwrap(), None).into());
        editor.activate_tab(1);
        editor.sync_path_rename();
        assert!(editor.tools.paths.rename.is_none());
        editor.activate_tab(0);
        editor.sync_path_rename();
        assert_eq!(editor.tools.paths.rename.as_ref().unwrap().name, "Draft");
        assert_eq!(editor.session().document.paths[0].name, "Outline");
        editor.tools.tool = Tool::Brush;
        editor.sync_path_rename();
        assert!(editor.tools.paths.rename.is_none());
        editor.tools.tool = Tool::Pen;
        editor.begin_path_rename().unwrap();
        editor.session_mut().document.paths.clear();
        editor.sync_path_rename();
        assert!(editor.tools.paths.rename.is_none());
        assert!(editor.session().undo_label().is_none());
    }

    #[test]
    fn header_input_owns_text_keys_and_buttons_submit_or_cancel() {
        use quickgui::{Application, Keystroke, WindowOptions};
        let (mut cx, editor) = Application::new()
            .into_test_context(
                WindowOptions::new("Path rename").size(1600., 900.),
                editor(),
            )
            .unwrap();
        let window = editor.window_handle();
        cx.click(window, "path-rename").unwrap();
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
        )
        .unwrap();
        cx.simulate_keystrokes(window, "S u b j e c t").unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::Backspace, Modifiers::empty()))
            .unwrap();
        cx.read(editor, |editor| {
            assert_eq!(editor.session().document.paths[0].geometry.anchors.len(), 2);
            assert_eq!(editor.session().document.paths[0].name, "Outline");
            assert_eq!(editor.tools.paths.rename.as_ref().unwrap().name, "subjec");
        })
        .unwrap();
        cx.click(window, "path-name-save").unwrap();
        cx.read(editor, |editor| {
            assert_eq!(editor.session().document.paths[0].name, "subjec");
            assert!(editor.tools.paths.rename.is_none());
        })
        .unwrap();
        cx.click(window, "path-rename").unwrap();
        cx.simulate_keystrokes(window, "X").unwrap();
        cx.click(window, "path-name-cancel").unwrap();
        cx.read(editor, |editor| {
            assert_eq!(editor.session().document.paths[0].name, "subjec")
        })
        .unwrap();
        cx.click(window, "path-rename").unwrap();
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
        )
        .unwrap();
        cx.simulate_keystrokes(window, "N e w").unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::Enter, Modifiers::empty()))
            .unwrap();
        cx.click(window, "path-rename").unwrap();
        cx.simulate_keystrokes(window, "X").unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::Escape, Modifiers::empty()))
            .unwrap();
        cx.read(editor, |editor| {
            assert_eq!(editor.session().document.paths[0].name, "new");
            assert!(editor.tools.paths.rename.is_none());
            assert_eq!(editor.session().document.paths[0].geometry.anchors.len(), 2);
        })
        .unwrap();
    }
}
