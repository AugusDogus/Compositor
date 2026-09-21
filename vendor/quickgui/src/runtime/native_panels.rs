use super::*;

/// One change reported by a system panel or an asynchronous native service.
///
/// These arrive on the event loop rather than inside the calling event, because AppKit delivers
/// them from its own panel targets and, for biometrics, from a background queue.
#[cfg(target_os = "macos")]
#[derive(Clone, Debug)]
pub(crate) enum NativePanelEvent {
    ColorChanged(Color),
    FontChanged(Font),
    BiometricResult {
        id: u64,
        granted: bool,
        error: Option<Arc<str>>,
    },
}

impl Runtime {
    /// Route one system-panel change to its application callback.
    #[cfg(target_os = "macos")]
    pub(super) fn invoke_native_panel_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: NativePanelEvent,
    ) {
        match event {
            NativePanelEvent::ColorChanged(color) => {
                let Some(mut callback) = self.application_callbacks.color_panel_change.take()
                else {
                    return;
                };
                let mut context = self.event_context();
                callback(color, &mut context);
                self.application_callbacks.color_panel_change = Some(callback);
                self.apply_application_context(event_loop, context);
            }
            NativePanelEvent::FontChanged(font) => {
                let Some(mut callback) = self.application_callbacks.font_panel_change.take() else {
                    return;
                };
                let mut context = self.event_context();
                callback(font, &mut context);
                self.application_callbacks.font_panel_change = Some(callback);
                self.apply_application_context(event_loop, context);
            }
            NativePanelEvent::BiometricResult { id, granted, error } => {
                crate::macos::complete_biometric_authentication(id, granted, error);
            }
        }
    }
}
