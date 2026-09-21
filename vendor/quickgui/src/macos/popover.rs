use super::*;

struct PopoverWatch {
    handle: WindowHandle,
    window: Retained<NSWindow>,
    anchor_window: Retained<NSWindow>,
    anchor_screen_rect: NSRect,
}

#[derive(Default)]
struct PopoverMonitorState {
    watches: Vec<PopoverWatch>,
    dismiss_pending: bool,
}

impl PopoverMonitorState {
    fn request_top_dismiss(&mut self, event: &NSEvent) -> Option<(WindowHandle, bool)> {
        let watch = self.watches.last()?;
        let event_window = MainThreadMarker::new().and_then(|mtm| unsafe { event.window(mtm) });
        let inside = event_window
            .as_ref()
            .is_some_and(|window| popover_window_contains(&watch.window, window.as_ref()));
        if inside || self.dismiss_pending {
            return None;
        }
        let same_window = event_window
            .as_ref()
            .is_some_and(|window| std::ptr::eq(watch.anchor_window.as_ref(), window.as_ref()));
        let window_point = unsafe { event.locationInWindow() };
        let screen_point = event_window.as_ref().map_or(window_point, |window| unsafe {
            window.convertPointToScreen(window_point)
        });
        let consume_anchor_press = should_consume_popover_anchor_press(
            unsafe { event.r#type() },
            same_window,
            watch.anchor_screen_rect,
            screen_point,
        );
        self.dismiss_pending = true;
        Some((watch.handle, consume_anchor_press))
    }
}

pub(super) fn should_consume_popover_anchor_press(
    event_type: NSEventType,
    same_window: bool,
    anchor_rect: NSRect,
    point: NSPoint,
) -> bool {
    event_type == NSEventType::LeftMouseDown
        && same_window
        && point.x.is_finite()
        && point.y.is_finite()
        && !point_outside_ns_rect(point, anchor_rect)
}

fn popover_window_contains(root: &NSWindow, candidate: &NSWindow) -> bool {
    if std::ptr::eq(root, candidate) {
        return true;
    }
    let mut parent = unsafe { candidate.parentWindow() };
    for _ in 0..MAX_GRABBING_POPOVERS {
        let Some(window) = parent else {
            return false;
        };
        if std::ptr::eq(root, window.as_ref()) {
            return true;
        }
        parent = unsafe { window.parentWindow() };
    }
    false
}

/// One lazy local AppKit event monitor services every nested grabbing popover.
///
/// The monitor is absent while no popover owns a grab, so this path adds no idle mouse-event work
/// to ordinary windows. Entries are bounded and the topmost popover alone owns dismissal semantics.
/// Cross-application dismissal is deliberately handled after `NSApplication` has resigned active;
/// closing from a global mouse monitor can race AppKit's activation handoff and return key appearance
/// to the owner window.
pub(crate) struct MacPopoverMonitor {
    proxy: EventLoopProxy<RuntimeEvent>,
    state: Rc<RefCell<PopoverMonitorState>>,
    local_monitor: Option<Retained<AnyObject>>,
}

impl MacPopoverMonitor {
    pub(crate) fn new(proxy: EventLoopProxy<RuntimeEvent>) -> Self {
        Self {
            proxy,
            state: Rc::new(RefCell::new(PopoverMonitorState::default())),
            local_monitor: None,
        }
    }

    fn install(&mut self) -> Result<(), String> {
        if self.local_monitor.is_some() {
            return Ok(());
        }
        MainThreadMarker::new().ok_or_else(|| {
            "popover event monitoring must be installed on the AppKit main thread".to_owned()
        })?;
        let mask =
            NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown;

        let local_state = Rc::clone(&self.state);
        let local_proxy = self.proxy.clone();
        let local_block = RcBlock::new(move |event: NonNull<NSEvent>| {
            let dismiss = local_state
                .borrow_mut()
                .request_top_dismiss(unsafe { event.as_ref() });
            if let Some((handle, consume_anchor_press)) = dismiss {
                let _ =
                    local_proxy.send_event(RuntimeEvent::PopoverPointerDismissRequested(handle));
                if consume_anchor_press {
                    // The active anchor is a close affordance. Consume its mouse-down so the
                    // owner cannot receive a later click and reopen after dismissal state lands.
                    return std::ptr::null_mut();
                }
            }
            event.as_ptr()
        });
        self.local_monitor = Some(
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local_block) }
                .ok_or_else(|| "could not install the AppKit popover event monitor".to_owned())?,
        );
        Ok(())
    }

    fn uninstall(&mut self) {
        if let Some(monitor) = self.local_monitor.take() {
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
    }

    pub(crate) fn watch(
        &mut self,
        handle: WindowHandle,
        window: &Arc<Window>,
        anchor_window: &Arc<Window>,
        anchor_rect: Rect,
    ) -> Result<(), String> {
        let window = appkit_window(window)?;
        let anchor_view = appkit_view(anchor_window)?;
        let anchor_window = anchor_view
            .window()
            .ok_or_else(|| "the popover anchor view is not attached to a window".to_owned())?;
        let parent_content = anchor_window.contentRectForFrameRect(anchor_window.frame());
        let anchor_screen_rect = NSRect::new(
            NSPoint::new(
                parent_content.origin.x + f64::from(anchor_rect.x),
                parent_content.origin.y + parent_content.size.height
                    - f64::from(anchor_rect.bottom()),
            ),
            NSSize::new(f64::from(anchor_rect.width), f64::from(anchor_rect.height)),
        );
        {
            let mut state = self.state.borrow_mut();
            state.watches.retain(|watch| watch.handle != handle);
            if state.watches.len() == MAX_GRABBING_POPOVERS {
                return Err(format!(
                    "an application cannot retain more than {MAX_GRABBING_POPOVERS} grabbing popovers"
                ));
            }
            state.watches.push(PopoverWatch {
                handle,
                window,
                anchor_window,
                anchor_screen_rect,
            });
            state.dismiss_pending = false;
        }
        if let Err(error) = self.install() {
            self.state
                .borrow_mut()
                .watches
                .retain(|watch| watch.handle != handle);
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn unwatch(&mut self, handle: WindowHandle) {
        let empty = {
            let mut state = self.state.borrow_mut();
            let previous_top = state.watches.last().map(|watch| watch.handle);
            state.watches.retain(|watch| watch.handle != handle);
            let top = state.watches.last().map(|watch| watch.handle);
            if top != previous_top {
                state.dismiss_pending = false;
            }
            state.watches.is_empty()
        };
        if empty {
            self.uninstall();
        }
    }
}

impl Drop for MacPopoverMonitor {
    fn drop(&mut self) {
        self.uninstall();
    }
}
