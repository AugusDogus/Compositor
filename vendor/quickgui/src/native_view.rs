use std::{ffi::c_void, fmt, ptr::NonNull};

use objc2::rc::Retained;
use objc2_app_kit::NSView;

use crate::{ElementId, Insets, Rect};

/// Largest outset a native view may claim beyond its layout box, in logical points.
pub const MAX_NATIVE_VIEW_OUTSET: f32 = 256.0;

/// A retained AppKit view that can participate in QuickGUI layout.
///
/// QuickGUI owns one retain while the value exists. The underlying `NSView` stays responsible for
/// its own content, delegates, and platform-specific behavior.
#[derive(Clone)]
pub struct MacNativeView {
    view: Retained<NSView>,
    outset: f32,
}

impl MacNativeView {
    /// Retain an AppKit view for declarative embedding.
    ///
    /// Subclasses such as `WKWebView`, `AVPlayerView`, and `PDFView` coerce to `&NSView`.
    pub fn new(view: &NSView) -> Self {
        let pointer = view as *const NSView as *mut NSView;
        let view = unsafe { Retained::retain(pointer) }
            .expect("a borrowed NSView must have a non-null Objective-C identity");
        Self { view, outset: 0.0 }
    }

    /// Extend the native frame `outset` points past the element's layout box on every side.
    ///
    /// Layout, hit regions, and siblings keep seeing the element's own box; only the AppKit frame
    /// and its clip grow, so a control can draw an effect that spills past its bounds, such as
    /// Liquid Glass, without that headroom pushing its neighbours apart. Points in the extra ring
    /// that hit no hosted content fall through to the framework view beneath. An outset view is
    /// not corner-clipped. Non-finite or negative values mean no outset; larger values clamp to
    /// [`MAX_NATIVE_VIEW_OUTSET`].
    pub fn with_outset(mut self, outset: f32) -> Self {
        self.outset = sanitized_outset(outset);
        self
    }

    /// Distance the native frame extends past the layout box on each side.
    pub fn outset(&self) -> f32 {
        self.outset
    }

    /// The AppKit frame for an element laid out at `bounds`.
    pub(crate) fn frame(&self, bounds: Rect) -> Rect {
        outset_frame(bounds, self.outset)
    }

    pub fn as_ns_view(&self) -> &NSView {
        &self.view
    }

    pub(crate) fn retained(&self) -> Retained<NSView> {
        self.view.clone()
    }

    pub(crate) fn pointer(&self) -> NonNull<c_void> {
        NonNull::from(self.view.as_ref()).cast()
    }
}

impl fmt::Debug for MacNativeView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MacNativeView")
            .field("view", &self.pointer())
            .field("outset", &self.outset)
            .finish()
    }
}

pub(crate) fn sanitized_outset(outset: f32) -> f32 {
    if outset.is_finite() && outset > 0.0 {
        outset.min(MAX_NATIVE_VIEW_OUTSET)
    } else {
        0.0
    }
}

pub(crate) fn outset_frame(bounds: Rect, outset: f32) -> Rect {
    bounds.inset(Insets::all(-sanitized_outset(outset)))
}

#[derive(Clone, Debug)]
pub(crate) struct NativeViewPlacement {
    pub id: ElementId,
    pub view: MacNativeView,
    pub bounds: Rect,
    pub clip: Rect,
    pub corner_radius: f32,
    pub opacity: f32,
    pub z_index: i16,
    pub source_order: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_view_outsets_grow_the_frame_symmetrically_and_stay_bounded() {
        let bounds = Rect::new(10.0, 20.0, 80.0, 22.0);
        assert_eq!(outset_frame(bounds, 0.0), bounds);
        assert_eq!(
            outset_frame(bounds, 24.0),
            Rect::new(-14.0, -4.0, 128.0, 70.0)
        );
        assert_eq!(outset_frame(bounds, -5.0), bounds);
        assert_eq!(outset_frame(bounds, f32::NAN), bounds);
        assert_eq!(outset_frame(bounds, f32::INFINITY), bounds);
        assert_eq!(sanitized_outset(1.0e6), MAX_NATIVE_VIEW_OUTSET);
    }
}
