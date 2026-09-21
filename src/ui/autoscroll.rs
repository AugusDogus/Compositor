use super::*;
use quickgui::{PointerEvent, PointerPhase};
use std::time::{Duration, Instant};

const FRAME_TIME: Duration = Duration::from_nanos(1_000_000_000 / 60);

pub(super) struct SelectionScroll {
    session: uuid::Uuid,
    pointer: PointerEvent,
    next: Instant,
}

fn delta(event: &PointerEvent) -> [f64; 2] {
    let speed = |past: f32| {
        if past <= 0. {
            0.
        } else {
            f64::from((2. + past * 0.4).min(40.))
        }
    };
    [
        speed(12. - event.local_position.x)
            - speed(event.local_position.x - event.size.width + 12.),
        speed(12. - event.local_position.y)
            - speed(event.local_position.y - event.size.height + 12.),
    ]
}

impl Editor {
    pub(super) fn selection_scroll_modifiers(&mut self, modifiers: Modifiers) {
        if let Some(scroll) = &mut self.selection_scroll {
            scroll.pointer.modifiers = modifiers;
        }
    }

    fn can_scroll_selection(&self) -> bool {
        !self.pending
            && self.modal.is_none()
            && matches!(
                self.gesture,
                Some(
                    Gesture::SelectionMove { .. }
                        | Gesture::Region {
                            tool: Tool::Rectangle | Tool::Ellipse,
                            ..
                        }
                )
            )
    }

    pub(super) fn track_selection_scroll(&mut self, event: &PointerEvent) {
        if !self.can_scroll_selection()
            || !matches!(event.phase, PointerPhase::Down | PointerPhase::Move)
            || delta(event) == [0., 0.]
        {
            self.selection_scroll = None;
            return;
        }
        if let Some(scroll) = &mut self.selection_scroll {
            scroll.pointer = *event;
        } else {
            self.selection_scroll = Some(SelectionScroll {
                session: self.session().id,
                pointer: *event,
                next: Instant::now() + FRAME_TIME,
            });
        }
    }

    pub(super) fn advance_selection_scroll(&mut self, now: Instant) -> Option<Instant> {
        let mut scroll = self.selection_scroll.take()?;
        if !self.can_scroll_selection() || scroll.session != self.session().id {
            return None;
        }
        if now >= scroll.next {
            let movement = delta(&scroll.pointer);
            if movement == [0., 0.] {
                return None;
            }
            let session = self.session_mut();
            session.fit = false;
            for (pan, movement) in session.pan.iter_mut().zip(movement) {
                *pan += movement;
            }
            // The pointer is stationary while the document moves underneath it.
            // Reuse the gesture's original selection/anchor and modifier rules.
            scroll.pointer.phase = PointerPhase::Move;
            if let Err(error) = self.pointer(&scroll.pointer) {
                self.gesture = None;
                self.session_mut().cancel();
                self.selection_scroll = None;
                self.status = format!(
                    "Selection scrolling stopped: {error}. The selection edit was cancelled."
                );
                self.revision = self.revision.wrapping_add(1);
                return None;
            }
            self.revision = self.revision.wrapping_add(1);
            scroll.next = now + FRAME_TIME;
        }
        let next = scroll.next;
        self.selection_scroll = Some(scroll);
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::selection::Selection;
    use quickgui::{MouseButton, Point, Size, Vector};

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
    fn edge_speed_is_zero_inside_and_capped_outside() {
        assert_eq!(delta(&pointer(50., PointerPhase::Move)), [0., 0.]);
        assert!((delta(&pointer(100., PointerPhase::Move))[0] + 6.8).abs() < 0.00001);
        assert_eq!(delta(&pointer(300., PointerPhase::Move)), [-40., 0.]);
        assert_eq!(delta(&pointer(-300., PointerPhase::Move)), [40., 0.]);
    }

    #[test]
    fn stationary_edge_pointer_extends_marquee_and_stops_on_release_with_one_undo() {
        for tool in [Tool::Rectangle, Tool::Ellipse] {
            let mut e = Editor::with_test_document();
            e.tabs = vec![Session::new(Document::new(1000, 1000).unwrap(), None).into()];
            e.session_mut().fit = false;
            e.tools.tool = tool;
            let original = e.session().document.clone();
            e.pointer(&pointer(25., PointerPhase::Down)).unwrap();
            let mut edge = pointer(100., PointerPhase::Move);
            edge.local_position.y = 75.;
            e.pointer(&edge).unwrap();
            let draft_end = |editor: &Editor| match &editor.gesture {
                Some(Gesture::Region { end, .. }) => *end,
                _ => panic!("Expected an active marquee draft"),
            };
            let before = draft_end(&e);
            let next = e.selection_scroll.as_ref().unwrap().next;
            assert!(e.advance_selection_scroll(next).is_some());
            assert!(e.session().pan[0] < -6.7);
            let after = draft_end(&e);
            assert!(after[0] > before[0]);
            assert_eq!(e.session().document, original);
            edge.phase = PointerPhase::Up;
            e.pointer(&edge).unwrap();
            assert!(e.selection_scroll.is_none());
            let committed = e.session().document.selection.as_ref().unwrap();
            assert!(committed.bounds().unwrap()[2] > before[0]);
            assert!(e.advance_selection_scroll(next + FRAME_TIME).is_none());
            e.session_mut().undo();
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        }
    }

    #[test]
    fn outline_scrolling_preserves_original_selection_and_cancels_cleanly() {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(100, 100).unwrap();
        doc.selection =
            Some(Selection::marquee(100, 100, [20., 40.], [40., 60.], false, true).unwrap());
        e.tabs = vec![Session::new(doc.clone(), None).into()];
        e.session_mut().fit = false;
        e.tools.tool = Tool::Rectangle;
        e.pointer(&pointer(25., PointerPhase::Down)).unwrap();
        e.pointer(&pointer(100., PointerPhase::Move)).unwrap();
        let before = e.session().document.selection.clone().unwrap();
        let next = e.selection_scroll.as_ref().unwrap().next;
        e.advance_selection_scroll(next);
        assert!(
            e.session()
                .document
                .selection
                .as_ref()
                .unwrap()
                .bounds()
                .unwrap()[0]
                > before.bounds().unwrap()[0]
        );
        e.pointer(&pointer(100., PointerPhase::Cancel)).unwrap();
        assert_eq!(e.session().document, doc);
        assert!(e.selection_scroll.is_none());
        assert!(e.session().undo_label().is_none());
    }

    #[test]
    fn stationary_scrolling_uses_new_modifiers_and_stops_when_the_pointer_returns_inside() {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(100, 100).unwrap();
        doc.selection =
            Some(Selection::marquee(100, 100, [20., 40.], [40., 60.], false, true).unwrap());
        e.tabs = vec![Session::new(doc, None).into()];
        e.session_mut().fit = false;
        e.tools.tool = Tool::Rectangle;
        e.pointer(&pointer(25., PointerPhase::Down)).unwrap();
        let mut edge = pointer(100., PointerPhase::Move);
        edge.local_position.y = 70.;
        e.pointer(&edge).unwrap();
        let before = e
            .session()
            .document
            .selection
            .as_ref()
            .unwrap()
            .bounds()
            .unwrap();
        assert_eq!(before[1], 60.);
        e.selection_scroll_modifiers(Modifiers::SHIFT);
        let next = e.selection_scroll.as_ref().unwrap().next;
        e.advance_selection_scroll(next);
        let after = e
            .session()
            .document
            .selection
            .as_ref()
            .unwrap()
            .bounds()
            .unwrap();
        assert!(after[0] > before[0]);
        assert_eq!(after[1], 40.);
        e.pointer(&pointer(50., PointerPhase::Move)).unwrap();
        assert!(e.selection_scroll.is_none());
        let pan = e.session().pan;
        assert!(e.advance_selection_scroll(next + FRAME_TIME).is_none());
        assert_eq!(e.session().pan, pan);
    }
}
