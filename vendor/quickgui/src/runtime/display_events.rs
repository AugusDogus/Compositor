use super::*;

/// Maximum granular display events retained between two runtime turns.
///
/// One reconfiguration produces at most [`crate::MAX_DISPLAY_EVENTS`] events; the queue keeps a
/// small multiple so a burst of reconfigurations that arrives before the next turn cannot grow
/// without a bound. Older events are dropped first because the newest snapshot is authoritative.
pub const MAX_PENDING_DISPLAY_EVENTS: usize = crate::MAX_DISPLAY_EVENTS * 4;

impl Runtime {
    /// Deliver the granular display changes recorded by the last snapshot refresh.
    ///
    /// This runs only when a refresh actually produced events, so an application whose displays
    /// never change performs no work here.
    pub(super) fn flush_display_events(&mut self, event_loop: &ActiveEventLoop) {
        if self.pending_display_events.is_empty() {
            return;
        }
        while self.pending_display_events.len() > MAX_PENDING_DISPLAY_EVENTS {
            self.pending_display_events.pop_front();
        }
        let Some(mut callback) = self.application_callbacks.display_event.take() else {
            self.pending_display_events.clear();
            return;
        };
        let mut context = self.event_context();
        while let Some(event) = self.pending_display_events.pop_front() {
            callback(event, &mut context);
        }
        self.application_callbacks.display_event = Some(callback);
        self.apply_application_context(event_loop, context);
    }
}
