use crate::Vector;

/// Coalesces invalidation and high-frequency wheel input at the OS frame boundary.
#[derive(Debug, Default)]
pub(crate) struct FrameScheduler {
    redraw_queued: bool,
    pending_scroll: Vector,
}

impl FrameScheduler {
    /// Returns true only when the caller needs to queue an OS redraw.
    pub fn invalidate(&mut self) -> bool {
        if self.redraw_queued {
            false
        } else {
            self.redraw_queued = true;
            true
        }
    }

    pub fn accumulate_scroll(&mut self, delta: Vector) -> bool {
        self.pending_scroll += delta;
        self.invalidate()
    }

    pub fn take_scroll(&mut self) -> Vector {
        std::mem::take(&mut self.pending_scroll)
    }

    pub fn begin_redraw(&mut self) {
        self.redraw_queued = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalidations_are_coalesced_until_redraw() {
        let mut scheduler = FrameScheduler::default();
        assert!(scheduler.invalidate());
        assert!(!scheduler.invalidate());
        scheduler.begin_redraw();
        assert!(scheduler.invalidate());
    }

    #[test]
    fn scroll_deltas_are_accumulated_without_losing_axes() {
        let mut scheduler = FrameScheduler::default();
        scheduler.accumulate_scroll(Vector::new(2.0, 3.0));
        scheduler.accumulate_scroll(Vector::new(-1.0, 7.0));
        assert_eq!(scheduler.take_scroll(), Vector::new(1.0, 10.0));
        assert_eq!(scheduler.take_scroll(), Vector::ZERO);
    }
}
