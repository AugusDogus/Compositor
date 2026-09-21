use super::*;
use quickgui::{MouseButton, PointerEvent, PointerPhase};

#[derive(Clone, Copy)]
pub(super) struct SampleRing {
    pub position: quickgui::Point,
    pub original: [u8; 4],
    pub sampled: [u8; 4],
}

impl Editor {
    pub(super) fn update_sample_ring(
        &mut self,
        event: &PointerEvent,
        original: [u8; 4],
        sampled: [u8; 4],
    ) {
        match event.phase {
            PointerPhase::Down if event.button == MouseButton::Left => {
                self.sample_ring = Some(SampleRing {
                    position: event.local_position,
                    original,
                    sampled,
                });
            }
            PointerPhase::Move => {
                if let Some(ring) = &mut self.sample_ring {
                    ring.position = event.local_position;
                    ring.sampled = sampled;
                }
            }
            PointerPhase::Up | PointerPhase::Cancel => self.sample_ring = None,
            _ => {}
        }
    }

    pub(super) fn sample_ring_overlay(&self) -> Element {
        let Some(ring) = self
            .sample_ring
            .filter(|_| self.tools.shows_sample_ring && !self.space_pan)
        else {
            return div();
        };
        quickgui::canvas(move |_, painter| {
            // A 43-point radius and 24/16-point strokes match the macOS sample ring.
            for (start, end, width, color) in [
                (0., std::f32::consts::TAU, 24., [115, 115, 115, 255]),
                (
                    std::f32::consts::PI,
                    std::f32::consts::TAU,
                    16.,
                    ring.sampled,
                ),
                (0., std::f32::consts::PI, 16., ring.original),
            ] {
                let mut path = quickgui::PathBuilder::stroke(width);
                for i in 0..=128 {
                    let angle = start + (end - start) * i as f32 / 128.;
                    let point =
                        quickgui::Point::new(58. + 43. * angle.cos(), 58. + 43. * angle.sin());
                    if i == 0 {
                        path.move_to(point);
                    } else {
                        path.line_to(point);
                    }
                }
                let path = path.build().expect("The fixed sample-ring arc is finite");
                painter.paint_path(&path, Color::rgba8(color[0], color[1], color[2], color[3]));
            }
        })
        .id("sample-ring")
        .absolute()
        .w(116.)
        .h(116.)
        .translate(ring.position.x - 58., ring.position.y - 58.)
        .into_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Keystroke, Point, Size, Vector, WindowOptions};

    fn pointer(x: f32, phase: PointerPhase) -> PointerEvent {
        PointerEvent {
            phase,
            position: Point::new(x, 50.),
            origin: Point::new(25., 50.),
            local_position: Point::new(x, 50.),
            local_origin: Point::new(25., 50.),
            delta: Vector::ZERO,
            button: MouseButton::Left,
            modifiers: Modifiers::empty(),
            size: Size::new(100., 100.),
        }
    }

    #[test]
    fn sample_ring_keeps_the_pre_drag_color_and_tracks_palette_and_picker_samples() {
        for picker in [false, true] {
            let mut e = Editor::with_test_document();
            let mut doc = Document::new(100, 100).unwrap();
            doc.layers[0].content = compositor::document::LayerContent::Raster(Some(Arc::new(
                image::RgbaImage::from_fn(100, 100, |x, _| {
                    image::Rgba(if x < 50 {
                        [220, 30, 40, 255]
                    } else {
                        [20, 60, 230, 255]
                    })
                }),
            )));
            e.tabs = vec![Session::new(doc.clone(), None).into()];
            e.session_mut().fit = false;
            e.tools.brush.color = [10, 180, 50, 255];
            e.tools.tool = Tool::Eyedropper;
            if picker {
                e.open_form(Action::Color);
            }
            for (x, phase, expected) in [
                (25., PointerPhase::Down, [220, 30, 40, 255]),
                (75., PointerPhase::Move, [20, 60, 230, 255]),
                (125., PointerPhase::Move, [20, 60, 230, 255]),
            ] {
                let event = pointer(x, phase);
                if picker {
                    e.sample_picker(&event);
                } else {
                    e.pointer(&event).unwrap();
                }
                let ring = e.sample_ring.unwrap();
                assert_eq!(ring.original, [10, 180, 50, 255]);
                assert_eq!(ring.sampled, expected);
                assert_eq!(ring.position, event.local_position);
            }
            let event = pointer(125., PointerPhase::Up);
            if picker {
                e.sample_picker(&event);
                e.finish_color(false).unwrap();
            } else {
                e.pointer(&event).unwrap();
            }
            assert!(e.sample_ring.is_none());
            assert_eq!(e.session().document, doc);
            assert!(e.session().undo_label().is_none());
            if picker {
                assert_eq!(e.tools.brush.color, [10, 180, 50, 255]);
            }
        }
    }

    #[test]
    fn idle_shortcut_leaves_pixels_untouched_and_still_allows_space_pan() {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(100, 100).unwrap();
        compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
        e.tabs = vec![Session::new(doc.clone(), None).into()];
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Idle tool").size(1280., 850.), e)
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, "workspace").unwrap();
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("a".into()), Modifiers::empty()),
        )
        .unwrap();
        cx.update(view, |e, _| {
            assert_eq!(e.tools.tool, Tool::Idle);
            e.pointer(&pointer(25., PointerPhase::Down)).unwrap();
            e.pointer(&pointer(75., PointerPhase::Move)).unwrap();
            e.pointer(&pointer(75., PointerPhase::Up)).unwrap();
            e.nudge(&Key::ArrowRight, Modifiers::empty()).unwrap();
            assert_eq!(e.session().document, doc);
            assert!(e.session().undo_label().is_none());
            e.space_pan = true;
            e.pointer(&pointer(25., PointerPhase::Down)).unwrap();
            assert!(matches!(e.gesture, Some(Gesture::Pan)));
            let mut moved = pointer(75., PointerPhase::Move);
            moved.delta = Vector::new(50., 0.);
            e.pointer(&moved).unwrap();
            e.pointer(&pointer(75., PointerPhase::Up)).unwrap();
            assert_eq!(e.session().pan, [50., 0.]);
            assert_eq!(e.tools.tool, Tool::Idle);
            assert_eq!(e.session().document, doc);
        })
        .unwrap();
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
        )
        .unwrap();
        assert!(
            cx.read(view, |e| e.session().document.selection.is_some())
                .unwrap()
        );
    }

    #[test]
    fn sample_ring_toggle_hides_only_the_comparison_overlay() {
        let mut e = Editor::with_test_document();
        e.tools.tool = Tool::Eyedropper;
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Sample ring").size(1280., 850.), e)
            .unwrap();
        let window = view.window_handle();
        cx.update(view, |e, cx| {
            e.update_sample_ring(
                &pointer(25., PointerPhase::Down),
                [0, 0, 0, 255],
                [200, 100, 0, 255],
            );
            cx.invalidate();
        })
        .unwrap();
        assert!(cx.contains_element(window, "sample-ring").unwrap());
        cx.click(window, "sample-ring-toggle").unwrap();
        assert!(!cx.contains_element(window, "sample-ring").unwrap());
        cx.click(window, "sample-ring-toggle").unwrap();
        assert!(cx.contains_element(window, "sample-ring").unwrap());
    }
}
