use objc2::rc::Retained;
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindow,
};
use objc2_foundation::MainThreadMarker;
use winit::window::Window;

use super::appkit_view;
use crate::{MacOsVibrancy, MacOsVisualEffectState};

/// Owns one AppKit material view behind one Winit rendering view.
///
/// Winit retains its rendering view independently from `NSWindow.contentView`, so the view can be
/// reparented beside `NSVisualEffectView` in a shared container without changing its stable window
/// handle, event queue, or CAMetalLayer. Keeping the effect out of the Metal view's ancestor chain
/// also lets AppKit change materials without rebuilding that rendering subtree.
pub(crate) struct MacVibrancyHost {
    window: Retained<NSWindow>,
    content: Retained<NSView>,
    container: Retained<NSView>,
    effect: Retained<NSVisualEffectView>,
}

impl MacVibrancyHost {
    pub(crate) fn new(
        window: &std::sync::Arc<Window>,
        vibrancy: MacOsVibrancy,
        state: MacOsVisualEffectState,
    ) -> Result<Self, String> {
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            "macOS vibrancy must be initialized on the AppKit main thread".to_owned()
        })?;
        let content = appkit_view(window)?;
        let native_window = content
            .window()
            .ok_or_else(|| "the AppKit rendering view is not attached to a window".to_owned())?;
        let current_content = native_window
            .contentView()
            .ok_or_else(|| "the AppKit window has no content view".to_owned())?;
        if !std::ptr::eq(current_content.as_ref(), content.as_ref()) {
            return Err(
                "macOS vibrancy requires the Winit rendering view to own window content".to_owned(),
            );
        }

        let container = unsafe { NSView::initWithFrame(mtm.alloc(), content.frame()) };
        let effect = unsafe { NSVisualEffectView::initWithFrame(mtm.alloc(), container.bounds()) };
        unsafe {
            container.setAutoresizingMask(
                NSAutoresizingMaskOptions::NSViewWidthSizable
                    | NSAutoresizingMaskOptions::NSViewHeightSizable,
            );
            effect.setMaterial(native_material(vibrancy));
            effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            effect.setState(native_effect_state(state));
            effect.setAutoresizingMask(
                NSAutoresizingMaskOptions::NSViewWidthSizable
                    | NSAutoresizingMaskOptions::NSViewHeightSizable,
            );
            container.addSubview(&effect);
            native_window.setContentView(Some(&container));
            content.setFrame(container.bounds());
            content.setAutoresizingMask(
                NSAutoresizingMaskOptions::NSViewWidthSizable
                    | NSAutoresizingMaskOptions::NSViewHeightSizable,
            );
            container.addSubview(&content);
        }

        Ok(Self {
            window: native_window,
            content,
            container,
            effect,
        })
    }

    pub(crate) fn set_vibrancy(&self, vibrancy: MacOsVibrancy) {
        unsafe { self.effect.setMaterial(native_material(vibrancy)) };
    }

    pub(crate) fn set_state(&self, state: MacOsVisualEffectState) {
        unsafe { self.effect.setState(native_effect_state(state)) };
    }

    fn restore_content_view(&self) {
        let Some(current_content) = self.window.contentView() else {
            return;
        };
        if !std::ptr::eq(current_content.as_ref(), self.container.as_ref()) {
            return;
        }
        unsafe { self.content.removeFromSuperview() };
        self.window.setContentView(Some(&self.content));
    }
}

impl Drop for MacVibrancyHost {
    fn drop(&mut self) {
        self.restore_content_view();
    }
}

#[allow(deprecated)]
pub(super) const fn native_material(vibrancy: MacOsVibrancy) -> NSVisualEffectMaterial {
    match vibrancy {
        MacOsVibrancy::AppearanceBased => NSVisualEffectMaterial::AppearanceBased,
        MacOsVibrancy::Titlebar => NSVisualEffectMaterial::Titlebar,
        MacOsVibrancy::Selection => NSVisualEffectMaterial::Selection,
        MacOsVibrancy::Menu => NSVisualEffectMaterial::Menu,
        MacOsVibrancy::Popover => NSVisualEffectMaterial::Popover,
        MacOsVibrancy::Sidebar => NSVisualEffectMaterial::Sidebar,
        MacOsVibrancy::Header => NSVisualEffectMaterial::HeaderView,
        MacOsVibrancy::Sheet => NSVisualEffectMaterial::Sheet,
        MacOsVibrancy::Window => NSVisualEffectMaterial::WindowBackground,
        MacOsVibrancy::Hud => NSVisualEffectMaterial::HUDWindow,
        MacOsVibrancy::FullscreenUi => NSVisualEffectMaterial::FullScreenUI,
        MacOsVibrancy::Tooltip => NSVisualEffectMaterial::ToolTip,
        MacOsVibrancy::Content => NSVisualEffectMaterial::ContentBackground,
        MacOsVibrancy::UnderWindow => NSVisualEffectMaterial::UnderWindowBackground,
        MacOsVibrancy::UnderPage => NSVisualEffectMaterial::UnderPageBackground,
    }
}

pub(super) const fn native_effect_state(state: MacOsVisualEffectState) -> NSVisualEffectState {
    match state {
        MacOsVisualEffectState::FollowWindow => NSVisualEffectState::FollowsWindowActiveState,
        MacOsVisualEffectState::Active => NSVisualEffectState::Active,
        MacOsVisualEffectState::Inactive => NSVisualEffectState::Inactive,
    }
}
