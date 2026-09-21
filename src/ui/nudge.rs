use super::*;

enum Target {
    Pixels,
    Outline,
    Layers,
}

impl Editor {
    pub(super) fn nudge(&mut self, key: &Key, modifiers: Modifiers) -> Result<()> {
        if self.gesture.is_some()
            || self.tools.polygon.is_some()
            || modifiers.contains(Modifiers::ALT)
        {
            return Ok(());
        }
        let selection = self
            .session()
            .document
            .selection
            .as_ref()
            .is_some_and(|s| s.bounds().is_some());
        let target = if modifiers.contains(Modifiers::CONTROL) {
            if !self.can_float_selection() {
                return Ok(());
            }
            Target::Pixels
        } else if self.tools.tool == Tool::Move {
            Target::Layers
        } else if selection
            && matches!(
                self.tools.tool,
                Tool::Rectangle
                    | Tool::Ellipse
                    | Tool::Lasso
                    | Tool::Polygon
                    | Tool::Wand
                    | Tool::Object
            )
        {
            Target::Outline
        } else {
            return Ok(());
        };
        let amount = if modifiers.contains(Modifiers::SHIFT) {
            10.
        } else {
            1.
        };
        let delta = match key {
            Key::ArrowLeft => [-amount, 0.],
            Key::ArrowRight => [amount, 0.],
            Key::ArrowUp => [0., -amount],
            Key::ArrowDown => [0., amount],
            _ => return Ok(()),
        };
        if matches!(target, Target::Layers)
            && (self.transform_edit.is_some() || self.pending_pixels.is_some())
        {
            return self.nudge_pending_transform(delta);
        }
        if !self.can_edit_layers() {
            return Ok(());
        }
        self.finish_pending_edits()?;
        let mask = self.tools.mask_target;
        let label = match target {
            Target::Pixels => "Move Pixels",
            Target::Outline => "Move Selection",
            Target::Layers => self.transform_history_label(false),
        };
        self.session_mut().edit(label, |doc| match target {
            Target::Pixels => compositor::floating::move_pixels(doc, delta, false),
            Target::Outline => {
                if let Some(selection) = &doc.selection {
                    doc.selection = Some(selection.translated(delta)?);
                }
                Ok(())
            }
            Target::Layers => compositor::edits::move_selected(doc, delta, mask),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::selection::Selection;

    #[test]
    fn tool_shortcuts_during_a_brush_stroke_preserve_the_stroke_until_escape() {
        use compositor::brush::{PaintMode, Stroke};
        use quickgui::{Application, Keystroke, WindowOptions};
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(Document::new(20, 20).unwrap(), None).into()];
        let original = e.session().document.clone();
        e.tools.tool = Tool::Brush;
        e.session_mut().begin("Brush").unwrap();
        let brush = e.tools.brush;
        let stroke = Stroke::start(
            &mut e.session_mut().document,
            [10., 10.],
            brush,
            PaintMode::Paint,
            false,
            false,
        )
        .unwrap();
        e.gesture = Some(Gesture::Paint {
            id: uuid::Uuid::new_v4(),
            stroke: Box::new(stroke),
        });
        let painting = e.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Stroke shortcuts").size(1280., 850.), e)
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, "workspace").unwrap();
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("e".into()), Modifiers::empty()),
        )
        .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tools.tool, Tool::Brush);
            assert!(matches!(e.gesture, Some(Gesture::Paint { .. })));
            assert_eq!(e.session().document, painting);
        })
        .unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::Escape, Modifiers::empty()))
            .unwrap();
        cx.read(view, |e| {
            assert!(e.gesture.is_none());
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
    }

    #[test]
    fn arrow_keys_only_move_the_targets_allowed_by_the_active_tool_and_modifiers() {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(20, 20).unwrap();
        compositor::edits::fill(&mut doc, [180, 50, 80, 255], false, false).unwrap();
        e.tabs = vec![Session::new(doc.clone(), None).into()];
        for tool in [
            Tool::Brush,
            Tool::Gradient,
            Tool::Rectangle,
            Tool::Lasso,
            Tool::Zoom,
        ] {
            e.tools.tool = tool;
            e.nudge(&Key::ArrowRight, Modifiers::empty()).unwrap();
            e.nudge(&Key::ArrowRight, Modifiers::CONTROL).unwrap();
            assert_eq!(e.session().document, doc);
            assert!(e.session().undo_label().is_none());
        }
        e.tools.tool = Tool::Move;
        e.nudge(&Key::ArrowRight, Modifiers::ALT).unwrap();
        assert_eq!(e.session().document, doc);
        e.nudge(&Key::ArrowRight, Modifiers::SHIFT).unwrap();
        assert_eq!(e.session().document.layers[0].transform.origin, [10., 0.]);
        e.session_mut().undo();
        e.session_mut().document.selection =
            Some(Selection::marquee(20, 20, [2., 2.], [8., 8.], false, false).unwrap());
        let selected = e.session().document.clone();
        e.tools.tool = Tool::Rectangle;
        e.nudge(&Key::ArrowDown, Modifiers::empty()).unwrap();
        assert_eq!(e.session().document.layers, selected.layers);
        assert_eq!(
            e.session()
                .document
                .selection
                .as_ref()
                .unwrap()
                .coverage([4.5, 2.5]),
            0.
        );
        assert_eq!(
            e.session()
                .document
                .selection
                .as_ref()
                .unwrap()
                .coverage([4.5, 8.5]),
            1.
        );
        e.session_mut().undo();
        e.tools.tool = Tool::Brush;
        e.nudge(&Key::ArrowRight, Modifiers::CONTROL).unwrap();
        assert_ne!(e.session().document.layers, selected.layers);
        e.session_mut().undo();
        assert_eq!(e.session().document, selected);
    }
}
