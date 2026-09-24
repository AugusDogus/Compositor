use super::*;

#[test]
fn text_color_picker_previews_canvas_without_changing_history_and_cancel_restores() {
    for foreground in [false, true] {
        let mut editor = Editor::with_test_document();
        let style = compositor::text::Text {
            content: "Color preview".into(),
            ..Default::default()
        };
        let pixels = compositor::text::TextRenderer::default()
            .render(&style)
            .unwrap();
        let layer = compositor::text::new_layer(style, pixels, [120., 100.]).unwrap();
        editor.session_mut().document.add(layer).unwrap();
        let original = editor.session().document.clone();
        editor.edit_active_text().unwrap();
        let palette = editor.tools.brush.color;
        let undo = editor.session().undo_label().map(str::to_owned);
        if foreground {
            editor.open_form(Action::Color);
        } else {
            editor.open_text_color_picker();
        }
        for (channel, value) in ["25", "150", "230"].into_iter().enumerate() {
            editor.picker_input(Some(channel), value);
        }
        editor.preview_picker().unwrap();
        let preview = editor.preview_document();
        let text = preview.active_layer().unwrap().text.as_ref().unwrap();
        assert_eq!(
            [text.red, text.green, text.blue],
            [25. / 255., 150. / 255., 230. / 255.]
        );
        assert_eq!(editor.session().document, original);
        assert_eq!(editor.session().undo_label(), undo.as_deref());
        assert_eq!(editor.tools.brush.color, palette);
        editor.finish_color(false).unwrap();
        assert!(matches!(editor.modal, Some(Form::Text(_))));
        assert_eq!(editor.preview_document(), original);
        assert_eq!(editor.tools.brush.color, palette);
        if foreground {
            editor.open_form(Action::Color);
        } else {
            editor.open_text_color_picker();
        }
        for (channel, value) in ["25", "150", "230"].into_iter().enumerate() {
            editor.picker_input(Some(channel), value);
        }
        editor.finish_color(true).unwrap();
        assert_eq!(editor.session().document, original);
        assert_eq!(
            editor.tools.brush.color,
            if foreground {
                [25, 150, 230, 255]
            } else {
                palette
            }
        );
        let Some(Form::Text(draft)) = &editor.modal else {
            panic!("text draft missing")
        };
        let style = draft.parsed().unwrap();
        assert_eq!(
            [style.red, style.green, style.blue],
            [25. / 255., 150. / 255., 230. / 255.]
        );
        assert_eq!(editor.session().document, original);
    }
}
