use std::{
    any::{Any, TypeId},
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    ffi::{CStr, CString, c_void},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr::{NonNull, null, null_mut},
    rc::{Rc, Weak},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use block2::RcBlock;
use objc2::{
    ClassType, DeclaredClass,
    declare::ClassBuilder,
    declare_class,
    ffi::{
        OBJC_ASSOCIATION_RETAIN_NONATOMIC, objc_getAssociatedObject, objc_setAssociatedObject,
        object_setClass,
    },
    msg_send, msg_send_id,
    mutability::{InteriorMutable, MainThreadOnly},
    rc::Retained,
    runtime::{AnyClass, AnyObject, Bool, NSObjectProtocol, ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::{
    NSAccessibilityLayoutChangedNotification, NSAccessibilityPostNotification,
    NSAccessibilityUnignoredChildren, NSAlert, NSAlertFirstButtonReturn, NSAlertStyle,
    NSApplication, NSAutoresizingMaskOptions, NSBox, NSBoxType, NSColor, NSDragOperation,
    NSDraggingContext, NSDraggingFormation, NSDraggingInfo, NSDraggingItem, NSDraggingSession,
    NSDraggingSource, NSEvent, NSEventMask, NSEventType, NSFilenamesPboardType,
    NSFloatingWindowLevel, NSImage, NSModalResponse, NSModalResponseCancel, NSModalResponseOK,
    NSNormalWindowLevel, NSOpenPanel, NSPanel, NSPasteboard, NSPasteboardType,
    NSPasteboardTypeString, NSPasteboardTypeURL, NSPasteboardWriting, NSPopUpMenuWindowLevel,
    NSResponder, NSSavePanel, NSScreen, NSTitlePosition, NSView, NSViewLayerContentsRedrawPolicy,
    NSWindow, NSWindowAnimationBehavior, NSWindowButton, NSWindowCollectionBehavior,
    NSWindowDidBecomeKeyNotification, NSWindowDidChangeOcclusionStateNotification,
    NSWindowDidExitFullScreenNotification, NSWindowDidResizeNotification,
    NSWindowDidUpdateNotification, NSWindowOrderingMode, NSWindowStyleMask, NSWindowTabGroup,
    NSWindowTabbingMode, NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSCopying, NSFileManager, NSNotification, NSNotificationCenter,
    NSObject, NSPoint, NSRange, NSRect, NSSize, NSString, NSStringEncodingConversionOptions, NSURL,
    NSUTF8StringEncoding, NSUUID,
};
use winit::{
    event_loop::EventLoopProxy,
    platform::macos::WindowExtMacOS,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

use crate::{
    ElementId, ExternalDragOperation, ExternalDragPayload, ExternalDragText, ExternalDragUrl,
    MAX_EXTERNAL_DRAG_TEXT_BYTES, MAX_EXTERNAL_DRAG_URL_BYTES, MAX_GRABBING_POPOVERS,
    MAX_SYSTEM_WINDOW_TABS, Point, PopoverOptions, Rect, Size, WindowHandle, WindowKind,
    WindowLevel, WindowTabState,
    native_view::NativeViewPlacement,
    platform::{
        FileDialogFilter, MAX_PLATFORM_PATH_BYTES, MAX_SELECTED_PATHS,
        MAX_SELECTED_PATHS_TOTAL_BYTES, PathPromptOptions, PlatformDialogId, PlatformError,
        PlatformResponder, PromptButton, PromptLevel, SavePathOptions,
    },
    runtime::RuntimeEvent,
    ui_tree::ExternalDropSnapshot,
};

mod file_dialog;
mod image;
mod message_box;
mod panels;
mod quick_look;

pub(crate) use file_dialog::{present_native_open_panel, present_native_save_panel};
pub(crate) use image::{native_image_with_metadata, rasterize_native_image, system_image};
pub(crate) use message_box::present_native_message_box;
pub(crate) use panels::{
    authenticate_with_biometrics, close_color_panel, complete_biometric_authentication,
    share_items, show_color_panel, show_font_panel,
};
pub(crate) use quick_look::{close_file_preview, preview_file};

mod drag_drop;
mod native_host;
mod popover;
pub(crate) mod spell;
mod vibrancy;
mod windowing;

pub(crate) use drag_drop::{
    MacExternalDragMonitor, MacExternalDragSession, MacMouseDownEvent, MacNativeDropHost,
    MacNativeDropOffer, MacNativeDropPayload, MacNativeDropPending, MacTypedDragPayload,
    MacTypedDragRegistry, capture_left_mouse_down, start_external_drag,
};
pub(crate) use native_host::MacNativeHost;
pub(crate) use popover::MacPopoverMonitor;
pub(crate) use vibrancy::MacVibrancyHost;
pub(crate) use windowing::{
    MacFirstFrameGuard, MacPlatformDialog, MacPlatformDialogContext, MacPlatformDialogFocus,
    MacTrafficLightHost, MacWindowTabAction, configure_document_window,
    configure_gpu_window_resize, configure_window_kind, current_cursor_screen_position,
    current_pointer_position, dismiss_window_relation, is_window_fullscreen, is_window_maximized,
    is_window_miniaturized, native_file_url, order_window_above, order_window_front,
    perform_window_close, perform_window_drag, perform_window_tab_action, position_system_popover,
    position_traffic_lights, present_native_prompt, present_window_relation,
    set_window_aspect_ratio, set_window_button_visibility, set_window_document_edited,
    set_window_focusable, set_window_ignores_mouse_events, set_window_input_enabled,
    set_window_level, set_window_movable, set_window_opacity, set_window_represented_file,
    set_window_tabbing_identifier, set_window_visibility, set_window_visible_on_all_workspaces,
    shell_open_path, shell_open_url, shell_reveal_path, shell_trash_path, show_character_palette,
    window_tab_state,
};

use drag_drop::point_outside_ns_rect;
pub(crate) use windowing::appkit_view;
use windowing::{appkit_window, deepest_appkit_sheet, finish_native_dialog, native_file_path};

#[cfg(test)]
use drag_drop::{
    MAX_NATIVE_TYPED_DRAG_SESSIONS, bounded_pasteboard_string, external_drag_operation,
    external_dragging_item,
};
#[cfg(test)]
use popover::should_consume_popover_anchor_press;
#[cfg(test)]
use windowing::traffic_light_layout;

#[cfg(test)]
mod tests;
