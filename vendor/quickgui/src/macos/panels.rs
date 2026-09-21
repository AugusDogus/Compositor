use std::collections::HashMap;

use objc2::{class, rc::Allocated, runtime::AnyClass};
use objc2_foundation::NSNotificationName;

use super::*;
use crate::{
    Color, ColorPanelMode, Font, FontFamily, FontStyle, FontWeight, MAX_FONT_FAMILY_BYTES, Rect,
    ShareItem, platform::MAX_BIOMETRIC_REASON_BYTES, runtime::NativePanelEvent,
};

/// `LAPolicyDeviceOwnerAuthenticationWithBiometrics`.
const LA_POLICY_BIOMETRICS: isize = 1;
/// `NSItalicFontMask`.
const NS_ITALIC_FONT_MASK: usize = 1;
/// `NSBoldFontMask`.
const NS_BOLD_FONT_MASK: usize = 2;
/// Point size used when handing a QuickGUI [`Font`] to the system font panel.
///
/// QuickGUI's inherited font configuration carries no point size, so the panel is seeded with the
/// AppKit control size and only the family, weight, and slant round-trip.
const FONT_PANEL_POINT_SIZE: f64 = 13.0;

// LocalAuthentication is loaded lazily by the runtime class lookup below. Linking it here keeps
// the framework resolvable without adding a crate dependency.
#[link(name = "LocalAuthentication", kind = "framework")]
unsafe extern "C" {}

struct PanelResponderIvars {
    proxy: EventLoopProxy<RuntimeEvent>,
}

/// One in-flight biometric evaluation and the context that must outlive it.
struct PendingBiometricRequest {
    responder: PlatformResponder<bool>,
    _context: Retained<AnyObject>,
}

declare_class!(
    /// Core-owned target of the system color and font panels.
    ///
    /// AppKit holds panel targets weakly, so exactly one of these is retained per process for as
    /// long as a panel observation is installed.
    struct QuickGuiPanelResponder;

    unsafe impl ClassType for QuickGuiPanelResponder {
        type Super = NSObject;
        type Mutability = InteriorMutable;
        const NAME: &'static str = "QuickGuiPanelResponder";
    }

    impl DeclaredClass for QuickGuiPanelResponder {
        type Ivars = PanelResponderIvars;
    }

    unsafe impl QuickGuiPanelResponder {
        #[method(quickGuiColorChanged:)]
        fn color_changed(&self, sender: &AnyObject) {
            if let Some(color) = panel_color(sender) {
                self.send(NativePanelEvent::ColorChanged(color));
            }
        }

        #[method(quickGuiColorPanelWillClose:)]
        fn color_panel_will_close(&self, notification: &NSNotification) {
            let object = unsafe { notification.object() };
            if let Some(color) = object.as_deref().and_then(panel_color) {
                self.send(NativePanelEvent::ColorChanged(color));
            }
        }

        #[method(changeFont:)]
        fn change_font(&self, sender: &AnyObject) {
            if let Some(font) = converted_panel_font(sender) {
                self.send(NativePanelEvent::FontChanged(font));
            }
        }

        #[method(validModesForFontPanel:)]
        fn valid_modes_for_font_panel(&self, _panel: &AnyObject) -> usize {
            // NSFontPanelModeMaskFace | NSFontPanelModeMaskSize | NSFontPanelModeMaskCollection
            0x0000_0001 | 0x0000_0002 | 0x0000_0004
        }
    }
);

impl QuickGuiPanelResponder {
    fn new(proxy: EventLoopProxy<RuntimeEvent>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PanelResponderIvars { proxy });
        unsafe { msg_send_id![super(this), init] }
    }

    fn send(&self, event: NativePanelEvent) {
        let _ = self
            .ivars()
            .proxy
            .send_event(RuntimeEvent::NativePanel(event));
    }
}

thread_local! {
    /// The retained target of the currently observed system panels.
    static PANEL_RESPONDER: RefCell<Option<Retained<QuickGuiPanelResponder>>> =
        const { RefCell::new(None) };
    /// Font currently seeded into the font panel, needed by `changeFont:`.
    static PANEL_FONT: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    /// Responders and their `LAContext` values, waiting for a reply from a background queue.
    ///
    /// The context must outlive its evaluation, and the entry is removed - releasing both - as
    /// soon as the reply is delivered on the main thread, so repeated authentications retain at
    /// most one entry each while they are in flight.
    static PENDING_BIOMETRICS: RefCell<HashMap<u64, PendingBiometricRequest>> =
        RefCell::new(HashMap::new());
    /// Share pickers retained until AppKit finishes presenting them.
    static ACTIVE_SHARE_PICKER: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

fn responder(proxy: &EventLoopProxy<RuntimeEvent>) -> Retained<QuickGuiPanelResponder> {
    PANEL_RESPONDER.with(|slot| {
        let mut slot = slot.borrow_mut();
        slot.get_or_insert_with(|| QuickGuiPanelResponder::new(proxy.clone()))
            .clone()
    })
}

fn main_thread(action: &'static str) -> Result<MainThreadMarker, PlatformError> {
    MainThreadMarker::new().ok_or_else(|| {
        PlatformError::Platform(format!("{action} must run on the AppKit main thread").into())
    })
}

fn class_named(name: &str) -> Result<&'static AnyClass, PlatformError> {
    AnyClass::get(name).ok_or_else(|| {
        PlatformError::Platform(format!("{name} is not available in this process").into())
    })
}

fn panel_color(panel: &AnyObject) -> Option<Color> {
    let color: Option<Retained<NSColor>> = unsafe { msg_send_id![panel, color] };
    color.as_deref().and_then(native_color)
}

/// Convert an `NSColor` into QuickGUI's linear-light color through the sRGB color space.
fn native_color(color: &NSColor) -> Option<Color> {
    let space: Option<Retained<AnyObject>> =
        unsafe { msg_send_id![class!(NSColorSpace), sRGBColorSpace] };
    let space = space?;
    let converted: Option<Retained<NSColor>> =
        unsafe { msg_send_id![color, colorUsingColorSpace: &*space] };
    let converted = converted?;
    let (mut red, mut green, mut blue, mut alpha) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    unsafe {
        let _: () = msg_send![
            &converted,
            getRed: &mut red,
            green: &mut green,
            blue: &mut blue,
            alpha: &mut alpha,
        ];
    }
    let component = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(Color::rgba8(
        component(red),
        component(green),
        component(blue),
        component(alpha),
    ))
}

fn native_ns_color(color: Color) -> Retained<NSColor> {
    let [red, green, blue, alpha] = color.to_srgba8();
    let scale = |value: u8| f64::from(value) / 255.0;
    unsafe {
        NSColor::colorWithSRGBRed_green_blue_alpha(
            scale(red),
            scale(green),
            scale(blue),
            scale(alpha),
        )
    }
}

/// Present the shared system color panel and observe the user's selection.
pub(crate) fn show_color_panel(
    proxy: &EventLoopProxy<RuntimeEvent>,
    initial: Color,
    mode: ColorPanelMode,
) -> Result<(), PlatformError> {
    let _ = main_thread("the system color panel")?;
    let class = class_named("NSColorPanel")?;
    let panel: Option<Retained<AnyObject>> = unsafe { msg_send_id![class, sharedColorPanel] };
    let panel = panel.ok_or_else(|| {
        PlatformError::Platform("AppKit did not provide its shared color panel".into())
    })?;
    let target = responder(proxy);
    let target_ref: &AnyObject = &target;
    unsafe {
        let _: () = msg_send![&panel, setColor: &*native_ns_color(initial)];
        let _: () = msg_send![&panel, setContinuous: matches!(mode, ColorPanelMode::Continuous)];
        match mode {
            ColorPanelMode::Continuous => {
                let _: () = msg_send![&panel, setTarget: target_ref];
                let _: () = msg_send![&panel, setAction: sel!(quickGuiColorChanged:)];
            }
            ColorPanelMode::OnClose => {
                let _: () = msg_send![&panel, setTarget: std::ptr::null::<AnyObject>()];
                let _: () = msg_send![&panel, setAction: std::ptr::null::<AnyObject>()];
            }
        }
    }
    let center = unsafe { NSNotificationCenter::defaultCenter() };
    let name: &NSNotificationName = &NSString::from_str("NSWindowWillCloseNotification");
    unsafe {
        center.removeObserver_name_object(&target, Some(name), Some(&panel));
        if matches!(mode, ColorPanelMode::OnClose) {
            center.addObserver_selector_name_object(
                &target,
                sel!(quickGuiColorPanelWillClose:),
                Some(name),
                Some(&panel),
            );
        }
        let _: () = msg_send![&panel, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
    }
    Ok(())
}

/// Dismiss the shared system color panel and stop observing it.
pub(crate) fn close_color_panel() -> Result<(), PlatformError> {
    let _ = main_thread("the system color panel")?;
    let class = class_named("NSColorPanel")?;
    let panel: Option<Retained<AnyObject>> = unsafe { msg_send_id![class, sharedColorPanel] };
    let Some(panel) = panel else {
        return Ok(());
    };
    PANEL_RESPONDER.with(|slot| {
        if let Some(target) = slot.borrow().as_ref() {
            let name: &NSNotificationName = &NSString::from_str("NSWindowWillCloseNotification");
            unsafe {
                NSNotificationCenter::defaultCenter().removeObserver_name_object(
                    target,
                    Some(name),
                    Some(&panel),
                );
            }
        }
    });
    unsafe {
        let _: () = msg_send![&panel, setTarget: std::ptr::null::<AnyObject>()];
        let _: () = msg_send![&panel, setAction: std::ptr::null::<AnyObject>()];
        let _: () = msg_send![&panel, orderOut: std::ptr::null::<AnyObject>()];
    }
    Ok(())
}

fn native_font(font: &Font) -> Option<Retained<AnyObject>> {
    let class = class!(NSFont);
    let native: Option<Retained<AnyObject>> = match &font.family {
        FontFamily::Named(name) if !name.is_empty() => unsafe {
            msg_send_id![
                class,
                fontWithName: &*NSString::from_str(name),
                size: FONT_PANEL_POINT_SIZE,
            ]
        },
        FontFamily::Monospace => unsafe {
            msg_send_id![
                class,
                monospacedSystemFontOfSize: FONT_PANEL_POINT_SIZE,
                weight: 0.0_f64,
            ]
        },
        _ => None,
    };
    native.or_else(|| unsafe { msg_send_id![class, systemFontOfSize: FONT_PANEL_POINT_SIZE] })
}

/// Rebuild a QuickGUI [`Font`] from the panel's converted `NSFont`.
fn font_from_native(manager: &AnyObject, native: &AnyObject) -> Option<Font> {
    let family: Option<Retained<NSString>> = unsafe { msg_send_id![native, familyName] };
    let family = family?.to_string();
    if family.is_empty() || family.len() > MAX_FONT_FAMILY_BYTES {
        return None;
    }
    let traits: usize = unsafe { msg_send![manager, traitsOfFont: native] };
    let mut font = Font::new(FontFamily::Named(Arc::from(family)));
    if traits & NS_BOLD_FONT_MASK != 0 {
        font.weight = FontWeight::BOLD;
    }
    if traits & NS_ITALIC_FONT_MASK != 0 {
        font.style = FontStyle::Italic;
    }
    Some(font)
}

fn converted_panel_font(manager: &AnyObject) -> Option<Font> {
    let current = PANEL_FONT.with(|slot| slot.borrow().clone())?;
    let converted: Option<Retained<AnyObject>> =
        unsafe { msg_send_id![manager, convertFont: &*current] };
    let converted = converted?;
    let font = font_from_native(manager, &converted);
    PANEL_FONT.with(|slot| *slot.borrow_mut() = Some(converted));
    font
}

/// Present the shared system font panel seeded with `font`.
pub(crate) fn show_font_panel(
    proxy: &EventLoopProxy<RuntimeEvent>,
    font: &Font,
) -> Result<(), PlatformError> {
    let _ = main_thread("the system font panel")?;
    let class = class_named("NSFontManager")?;
    let manager: Option<Retained<AnyObject>> = unsafe { msg_send_id![class, sharedFontManager] };
    let manager = manager.ok_or_else(|| {
        PlatformError::Platform("AppKit did not provide its shared font manager".into())
    })?;
    let native = native_font(font).ok_or_else(|| {
        PlatformError::Platform("AppKit could not resolve the requested font".into())
    })?;
    let target = responder(proxy);
    let target_ref: &AnyObject = &target;
    unsafe {
        let _: () = msg_send![&manager, setTarget: target_ref];
        let _: () = msg_send![&manager, setSelectedFont: &*native, isMultiple: false];
        let _: () = msg_send![&manager, orderFrontFontPanel: std::ptr::null::<AnyObject>()];
    }
    PANEL_FONT.with(|slot| *slot.borrow_mut() = Some(native));
    Ok(())
}

/// Present a share sheet anchored to a rectangle inside the window's content view.
pub(crate) fn share_items(
    window: Option<&Arc<Window>>,
    items: &[ShareItem],
    anchor: Rect,
) -> Result<(), PlatformError> {
    let mtm = main_thread("a native share sheet")?;
    let window = window.ok_or(PlatformError::Unavailable)?;
    let view = appkit_view(window).map_err(|error| PlatformError::Platform(error.into()))?;
    let class = class_named("NSSharingServicePicker")?;

    let mut natives: Vec<Retained<NSObject>> = Vec::with_capacity(items.len());
    for item in items {
        let native: Retained<NSObject> = match item {
            ShareItem::Text(text) => Retained::into_super(NSString::from_str(text)),
            ShareItem::Url(url) => {
                let url =
                    unsafe { NSURL::initWithString(NSURL::alloc(), &NSString::from_str(url)) }
                        .ok_or_else(|| {
                            PlatformError::Platform(
                                "a shared URL is not valid for Foundation".into(),
                            )
                        })?;
                Retained::into_super(url)
            }
            ShareItem::File(path) => {
                let url = native_file_url(path, false)
                    .map_err(|error| PlatformError::Platform(error.into()))?;
                Retained::into_super(url)
            }
            ShareItem::Image(image) => {
                let image = crate::macos_shell::native_image(mtm, image)?;
                Retained::into_super(image)
            }
        };
        natives.push(native);
    }
    let array = NSArray::from_vec(natives);

    let picker: Option<Retained<AnyObject>> = unsafe {
        let allocated: Allocated<AnyObject> = msg_send_id![class, alloc];
        msg_send_id![allocated, initWithItems: &*array]
    };
    let picker = picker.ok_or_else(|| {
        PlatformError::Platform("AppKit could not create the share-sheet picker".into())
    })?;
    let bounds = view.bounds();
    let rect = NSRect::new(
        NSPoint::new(
            f64::from(anchor.x),
            bounds.size.height - f64::from(anchor.y) - f64::from(anchor.height),
        ),
        NSSize::new(f64::from(anchor.width), f64::from(anchor.height)),
    );
    let view_ref: &NSView = &view;
    unsafe {
        // `NSRectEdgeMinY` places the sheet below the anchor, matching AppKit's default menus.
        let _: () = msg_send![
            &picker,
            showRelativeToRect: rect,
            ofView: view_ref,
            preferredEdge: 1_usize,
        ];
    }
    ACTIVE_SHARE_PICKER.with(|slot| *slot.borrow_mut() = Some(picker));
    Ok(())
}

/// Ask `LAContext` to authenticate the current user with biometrics.
///
/// The reply arrives on a background queue, so the result is forwarded through the event loop and
/// the stored responder is completed on the main thread by
/// [`complete_biometric_authentication`].
pub(crate) fn authenticate_with_biometrics(
    proxy: &EventLoopProxy<RuntimeEvent>,
    reason: &str,
    responder: PlatformResponder<bool>,
) {
    debug_assert!(reason.len() <= MAX_BIOMETRIC_REASON_BYTES);
    if MainThreadMarker::new().is_none() {
        responder.complete(Err(PlatformError::Platform(
            "biometric authentication must start on the AppKit main thread".into(),
        )));
        return;
    }
    let class = match class_named("LAContext") {
        Ok(class) => class,
        Err(error) => {
            responder.complete(Err(error));
            return;
        }
    };
    let context: Option<Retained<AnyObject>> = unsafe { msg_send_id![class, new] };
    let Some(context) = context else {
        responder.complete(Err(PlatformError::Platform(
            "LocalAuthentication could not create a context".into(),
        )));
        return;
    };
    let mut error: *mut AnyObject = null_mut();
    let available: bool = unsafe {
        msg_send![
            &context,
            canEvaluatePolicy: LA_POLICY_BIOMETRICS,
            error: &mut error,
        ]
    };
    if !available {
        responder.complete(Err(PlatformError::Unsupported));
        return;
    }

    static NEXT_REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    PENDING_BIOMETRICS.with(|pending| {
        pending.borrow_mut().insert(
            id,
            PendingBiometricRequest {
                responder,
                _context: context.clone(),
            },
        )
    });

    let proxy = proxy.clone();
    let reply = RcBlock::new(move |granted: Bool, error: *mut AnyObject| {
        let message = unsafe { error.as_ref() }.and_then(|error| {
            let description: Option<Retained<NSString>> =
                unsafe { msg_send_id![error, localizedDescription] };
            description.map(|description| Arc::<str>::from(description.to_string()))
        });
        let _ = proxy.send_event(RuntimeEvent::NativePanel(
            NativePanelEvent::BiometricResult {
                id,
                granted: granted.as_bool(),
                error: message,
            },
        ));
    });
    unsafe {
        let _: () = msg_send![
            &context,
            evaluatePolicy: LA_POLICY_BIOMETRICS,
            localizedReason: &*NSString::from_str(reason),
            reply: &*reply,
        ];
    }
}

/// Complete a pending biometric request on the main thread.
pub(crate) fn complete_biometric_authentication(id: u64, granted: bool, error: Option<Arc<str>>) {
    let Some(pending) = PENDING_BIOMETRICS.with(|pending| pending.borrow_mut().remove(&id)) else {
        return;
    };
    if let Some(error) = error.filter(|_| !granted) {
        // A declined or cancelled attempt is an ordinary answer, not a platform failure.
        tracing::debug!(%error, "biometric authentication was not granted");
    }
    pending.responder.complete(Ok(granted));
}
