use super::*;

impl Editor {
    pub(super) fn step_numeric_field(
        &mut self,
        index: usize,
        up: bool,
        modifiers: Modifiers,
    ) -> bool {
        let Some(Form::Edit { action, fields, .. }) = &self.modal else {
            return false;
        };
        let bounds = match (action, index) {
            (Action::Transform, 0 | 1) => (-1_000_000., 1_000_000.),
            (Action::Transform, 2 | 3) => (1., 1_000_000.),
            (Action::Transform, 4) => (-360_000., 360_000.),
            (Action::Transform, 5) => (0.1, 100_000.),
            _ => return false,
        };
        let Some(value) = fields
            .get(index)
            .and_then(|field| field.1.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite())
        else {
            return false;
        };
        let step = if modifiers.contains(Modifiers::SHIFT) {
            10.
        } else {
            1.
        };
        let next = (value + if up { step } else { -step }).clamp(bounds.0, bounds.1);
        self.update_dimension(index, &next.to_string());
        self.update_transform_scale(index);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Keystroke, WindowOptions};

    #[test]
    fn arrows_step_linked_transform_fields_without_moving_canvas_or_applying_early() {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(100, 50).unwrap();
        compositor::edits::fill(&mut doc, [255; 4], false, false).unwrap();
        let original = doc.clone();
        e.tabs = vec![Session::new(doc, None).into()];
        e.open_form(Action::Transform);
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Numeric fields").size(1280., 1000.), e)
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, 50_002_u64).unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::ArrowUp, Modifiers::SHIFT))
            .unwrap();
        cx.read(view, |e| {
            let Some(Form::Edit { fields, .. }) = &e.modal else {
                panic!("transform form was closed");
            };
            assert_eq!(fields[2].1, "110");
            assert_eq!(fields[3].1, "55");
            assert_eq!(e.session().document, original);
        })
        .unwrap();
        cx.click(window, "form-apply").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.layers[0].transform.size)
                .unwrap(),
            [110., 55.]
        );
        cx.update(view, |e, _| e.session_mut().undo()).unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
    }
}
