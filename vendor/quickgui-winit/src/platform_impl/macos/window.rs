#![allow(clippy::unnecessary_cast)]

use std::cell::Cell;

use objc2::rc::{autoreleasepool, Retained};
use objc2::{declare_class, mutability, ClassType, DeclaredClass};
use objc2_app_kit::{NSPanel, NSResponder, NSWindow};
use objc2_foundation::{MainThreadBound, MainThreadMarker, NSObject};

use super::event_loop::ActiveEventLoop;
use super::window_delegate::WindowDelegate;
use crate::error::OsError as RootOsError;
use crate::window::WindowAttributes;

pub(crate) struct Window {
    window: MainThreadBound<Retained<NSWindow>>,
    /// The window only keeps a weak reference to this, so we must keep it around here.
    delegate: MainThreadBound<Retained<WindowDelegate>>,
}

impl Drop for Window {
    fn drop(&mut self) {
        self.window
            .get_on_main(|window| autoreleasepool(|_| window.close()))
    }
}

impl Window {
    pub(crate) fn new(
        window_target: &ActiveEventLoop,
        attributes: WindowAttributes,
    ) -> Result<Self, RootOsError> {
        let mtm = window_target.mtm;
        let delegate = autoreleasepool(|_| {
            WindowDelegate::new(window_target.app_delegate(), attributes, mtm)
        })?;
        Ok(Window {
            window: MainThreadBound::new(delegate.window().retain(), mtm),
            delegate: MainThreadBound::new(delegate, mtm),
        })
    }

    pub(crate) fn maybe_queue_on_main(&self, f: impl FnOnce(&WindowDelegate) + Send + 'static) {
        // For now, don't actually do queuing, since it may be less predictable
        self.maybe_wait_on_main(f)
    }

    pub(crate) fn maybe_wait_on_main<R: Send>(
        &self,
        f: impl FnOnce(&WindowDelegate) -> R + Send,
    ) -> R {
        self.delegate.get_on_main(|delegate| f(delegate))
    }

    #[cfg(feature = "rwh_06")]
    #[inline]
    pub(crate) fn raw_window_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawWindowHandle, rwh_06::HandleError> {
        if let Some(mtm) = MainThreadMarker::new() {
            Ok(self.delegate.get(mtm).raw_window_handle_rwh_06())
        } else {
            Err(rwh_06::HandleError::Unavailable)
        }
    }

    #[cfg(feature = "rwh_06")]
    #[inline]
    pub(crate) fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::RawDisplayHandle::AppKit(
            rwh_06::AppKitDisplayHandle::new(),
        ))
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub usize);

impl WindowId {
    pub const fn dummy() -> Self {
        Self(0)
    }
}

impl From<WindowId> for u64 {
    fn from(window_id: WindowId) -> Self {
        window_id.0 as u64
    }
}

impl From<u64> for WindowId {
    fn from(raw_id: u64) -> Self {
        Self(raw_id as usize)
    }
}

declare_class!(
    #[derive(Debug)]
    pub struct WinitWindow;

    unsafe impl ClassType for WinitWindow {
        #[inherits(NSResponder, NSObject)]
        type Super = NSWindow;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "WinitWindow";
    }

    impl DeclaredClass for WinitWindow {
        type Ivars = WinitWindowIvars;
    }

    unsafe impl WinitWindow {
        #[method(canBecomeMainWindow)]
        fn can_become_main_window(&self) -> bool {
            trace_scope!("canBecomeMainWindow");
            true
        }

        #[method(canBecomeKeyWindow)]
        fn can_become_key_window(&self) -> bool {
            trace_scope!("canBecomeKeyWindow");
            self.ivars().can_become_key_window.get()
        }
    }
);

#[derive(Debug)]
pub struct WinitWindowIvars {
    can_become_key_window: Cell<bool>,
}

impl Default for WinitWindowIvars {
    fn default() -> Self {
        Self {
            can_become_key_window: Cell::new(true),
        }
    }
}

impl WinitWindow {
    pub(super) fn set_can_become_key_window(&self, can_become_key_window: bool) {
        self.ivars()
            .can_become_key_window
            .set(can_become_key_window);
    }
}

#[derive(Debug)]
pub struct WinitPanelIvars {
    can_become_key_window: Cell<bool>,
}

impl Default for WinitPanelIvars {
    fn default() -> Self {
        Self {
            can_become_key_window: Cell::new(true),
        }
    }
}

declare_class!(
    #[derive(Debug)]
    pub struct WinitPanel;

    unsafe impl ClassType for WinitPanel {
        #[inherits(NSWindow, NSResponder, NSObject)]
        type Super = NSPanel;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "WinitPanel";
    }

    impl DeclaredClass for WinitPanel {
        type Ivars = WinitPanelIvars;
    }

    unsafe impl WinitPanel {
        // Borderless NSPanel instances do not become key by default. Winit's view supports text
        // input, so retain the same key-window capability as WinitWindow.
        #[method(canBecomeKeyWindow)]
        fn can_become_key_window(&self) -> bool {
            trace_scope!("canBecomeKeyWindow");
            self.ivars().can_become_key_window.get()
        }
    }
);

impl WinitPanel {
    pub(super) fn set_can_become_key_window(&self, can_become_key_window: bool) {
        self.ivars()
            .can_become_key_window
            .set(can_become_key_window);
    }
}

pub(super) fn window_id(window: &NSWindow) -> WindowId {
    WindowId(window as *const NSWindow as usize)
}
