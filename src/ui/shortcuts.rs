use super::*;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct OpacityDigits(Option<(u8, Instant)>);

impl OpacityDigits {
    fn push(&mut self, digit: u8, time: Instant) -> f64 {
        let percent = if let Some((first, at)) = self.0
            && time.saturating_duration_since(at) < Duration::from_millis(600)
        {
            self.0 = None;
            (first * 10 + digit).max(1)
        } else {
            self.0 = Some((digit, time));
            if digit == 0 { 100 } else { digit * 10 }
        };
        f64::from(percent) / 100.
    }
}

impl Editor {
    pub(super) fn opacity_digit(&mut self, digit: u8, time: Instant) -> Result<()> {
        if digit > 9
            || self.gesture.is_some()
            || self.pending
            || !(self.tools.tool.has_brush_cursor()
                || matches!(self.tools.tool, Tool::Move | Tool::Gradient))
        {
            return Ok(());
        }
        let opacity = self.tools.opacity_digits.push(digit, time);
        match self.tools.tool {
            Tool::Move => {
                if !self.can_edit_layers() {
                    return Ok(());
                }
                self.finish_pending_edits()?;
                self.session_mut().edit("Layer Opacity", |doc| {
                    for layer in &mut doc.layers {
                        if doc.selected.contains(&layer.id) {
                            layer.opacity = opacity;
                        }
                    }
                    Ok(())
                })
            }
            Tool::Gradient => {
                self.tools.gradient.opacity = opacity;
                self.refresh_gradient()
            }
            _ => {
                self.tools.brush.opacity = opacity;
                Ok(())
            }
        }
    }

    pub(super) fn brush_step(&mut self, increase: bool, hardness: bool) {
        if !self.tools.tool.has_brush_cursor() || self.gesture.is_some() {
            return;
        }
        if hardness {
            let quarter = self.tools.brush.hardness * 4.;
            self.tools.brush.hardness = (if increase {
                (quarter + 0.001).floor() + 1.
            } else {
                (quarter - 0.001).ceil() - 1.
            })
            .clamp(0., 4.)
                / 4.;
        } else {
            let diameter = self.tools.brush.diameter;
            self.tools.brush.diameter = if increase {
                (diameter + 1.).max((diameter * 1.2).round())
            } else {
                (diameter - 1.).min((diameter / 1.2).round())
            }
            .clamp(1., 2000.);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Keystroke, WindowOptions};

    #[test]
    fn space_drag_pans_without_committing_a_pending_gradient_and_release_restores_the_tool() {
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(Document::new(20, 20).unwrap(), None).into()];
        e.session_mut().fit = false;
        e.tools.tool = Tool::Gradient;
        let original = e.session().document.clone();
        e.begin_gradient([0., 0.]).unwrap();
        e.pending_gradient.as_mut().unwrap().end = [20., 20.];
        e.refresh_gradient().unwrap();
        let preview = e.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Space pan").size(1280., 900.), e)
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, "workspace").unwrap();
        let space = Keystroke::new(Key::Space, Modifiers::empty());
        cx.simulate_keystroke(window, space.clone()).unwrap();
        let bounds = cx.element_bounds(window, "canvas").unwrap();
        let from = quickgui::Point::new(bounds.x + 100., bounds.y + 100.);
        cx.simulate_pointer_drag(
            window,
            "canvas",
            from,
            quickgui::Point::new(from.x + 30., from.y + 20.),
        )
        .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tools.tool, Tool::Gradient);
            assert_eq!(e.session().pan, [30., 20.]);
            assert_eq!(e.session().document, preview);
            assert!(e.session().undo_label().is_none());
            assert!(e.pending_gradient.is_some());
        })
        .unwrap();
        cx.simulate_key_up(window, space).unwrap();
        assert!(!cx.read(view, |e| e.space_pan).unwrap());
        cx.simulate_keystroke(window, Keystroke::new(Key::Escape, Modifiers::empty()))
            .unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
    }

    #[test]
    fn command_modifiers_open_size_forms_create_layers_and_edit_pixels() {
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(Document::new(4, 4).unwrap(), None).into()];
        e.tools.brush.color = [180, 20, 40, 255];
        e.tools.background = [30, 90, 150, 255];
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Command modifiers").size(1280., 900.), e)
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, "workspace").unwrap();
        for (key, title) in [("c", "Canvas Size"), ("i", "Image Size")] {
            cx.simulate_keystroke(
                window,
                Keystroke::new(
                    Key::Character(key.into()),
                    Modifiers::CONTROL | Modifiers::ALT,
                ),
            )
            .unwrap();
            cx.read(view, |e| {
                assert!(
                    matches!(&e.modal, Some(Form::Edit { title: actual, .. }) if *actual == title)
                )
            })
            .unwrap();
            cx.simulate_keystroke(window, Keystroke::new(Key::Escape, Modifiers::empty()))
                .unwrap();
        }
        cx.simulate_keystroke(
            window,
            Keystroke::new(
                Key::Character("n".into()),
                Modifiers::CONTROL | Modifiers::SHIFT,
            ),
        )
        .unwrap();
        let active = cx
            .read(view, |e| {
                assert_eq!(e.tabs.len(), 1);
                assert_eq!(e.session().document.layers.len(), 2);
                e.session().document.active.unwrap()
            })
            .unwrap();
        for (modifiers, expected) in [
            (Modifiers::ALT, [180, 20, 40, 255]),
            (Modifiers::CONTROL, [30, 90, 150, 255]),
        ] {
            cx.simulate_keystroke(window, Keystroke::new(Key::Backspace, modifiers))
                .unwrap();
            cx.read(view, |e| {
                let layer = e.session().document.active_layer().unwrap();
                assert_eq!(layer.id, active);
                let compositor::document::LayerContent::Raster(Some(pixels)) = &layer.content
                else {
                    panic!("fill must retain a raster layer");
                };
                assert!(pixels.pixels().all(|p| p.0 == expected));
            })
            .unwrap();
        }
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("[".into()), Modifiers::CONTROL),
        )
        .unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.layers[0].id)
                .unwrap(),
            active
        );
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("]".into()), Modifiers::CONTROL),
        )
        .unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.layers[1].id)
                .unwrap(),
            active
        );
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("m".into()), Modifiers::CONTROL),
        )
        .unwrap();
        assert_eq!(
            cx.read(view, |e| e.adjustment_edit.as_ref().unwrap().settings.kind)
                .unwrap(),
            Kind::Curves
        );
    }

    #[test]
    fn opacity_digits_target_selected_layers_or_current_tool_and_support_exact_values() {
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(Document::new(4, 4).unwrap(), None).into()];
        let second = compositor::document::Layer::blank("Second", 4, 4);
        let second_id = second.id;
        let mut group = compositor::document::Layer::blank("Folder", 4, 4);
        group.content = compositor::document::LayerContent::Group;
        e.session_mut().document.layers.extend([second, group]);
        e.session_mut().document.selected =
            e.session().document.layers.iter().map(|l| l.id).collect();
        e.session_mut().document.active = Some(second_id);
        let now = Instant::now();
        e.opacity_digit(5, now).unwrap();
        assert_eq!(
            e.session()
                .document
                .layers
                .iter()
                .map(|l| l.opacity)
                .collect::<Vec<_>>(),
            [0.5, 0.5, 0.5]
        );
        e.session_mut().undo();
        assert!(e.session().document.layers.iter().all(|l| l.opacity == 1.));
        assert!(e.session().undo_label().is_none());
        e.tools.tool = Tool::Brush;
        e.opacity_digit(4, now + Duration::from_secs(1)).unwrap();
        e.opacity_digit(5, now + Duration::from_millis(1100))
            .unwrap();
        assert_eq!(e.tools.brush.opacity, 0.45);
        e.opacity_digit(0, now + Duration::from_secs(2)).unwrap();
        e.opacity_digit(5, now + Duration::from_millis(2100))
            .unwrap();
        assert_eq!(e.tools.brush.opacity, 0.05);
        e.tools.tool = Tool::Gradient;
        e.opacity_digit(1, now + Duration::from_secs(3)).unwrap();
        assert_eq!(e.tools.gradient.opacity, 0.1);
        assert_eq!(e.tools.brush.opacity, 0.05);
    }
    #[test]
    fn brush_shortcuts_reach_one_pixel_and_snap_hardness_to_quarters() {
        let mut e = Editor::with_test_document();
        e.tools.tool = Tool::Brush;
        e.tools.brush.diameter = 2.;
        e.brush_step(false, false);
        assert_eq!(e.tools.brush.diameter, 1.);
        e.brush_step(true, false);
        assert_eq!(e.tools.brush.diameter, 2.);
        e.tools.brush.hardness = 0.8;
        e.brush_step(false, true);
        assert_eq!(e.tools.brush.hardness, 0.75);
        e.tools.brush.hardness = 0.8;
        e.brush_step(true, true);
        assert_eq!(e.tools.brush.hardness, 1.);
        e.tools.tool = Tool::Rectangle;
        e.brush_step(false, true);
        assert_eq!(e.tools.brush.hardness, 1.);
    }
}
