use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::c_void,
    ptr::{NonNull, null_mut},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use block2::{Block, RcBlock};
use core_foundation::{
    base::TCFType,
    runloop::{CFRunLoop, CFRunLoopSource, CFRunLoopSourceRef, kCFRunLoopDefaultMode},
};
use objc2::{
    ClassType, DeclaredClass,
    declare::ClassBuilder,
    declare_class,
    ffi::{
        OBJC_ASSOCIATION_RETAIN_NONATOMIC, objc_getAssociatedObject, objc_setAssociatedObject,
        object_setClass,
    },
    msg_send_id,
    mutability::{InteriorMutable, MainThreadOnly},
    rc::Retained,
    runtime::{AnyClass, AnyObject, Bool, NSObjectProtocol, ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::{
    NSApplication, NSApplicationDidBecomeActiveNotification,
    NSApplicationDidChangeScreenParametersNotification, NSApplicationDidResignActiveNotification,
    NSApplicationTerminateReply, NSEvent, NSEventModifierFlags, NSEventSubtype, NSEventType,
    NSMenu, NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
    NSWorkspaceDidWakeNotification, NSWorkspaceSessionDidBecomeActiveNotification,
    NSWorkspaceSessionDidResignActiveNotification, NSWorkspaceWillPowerOffNotification,
    NSWorkspaceWillSleepNotification,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSBundle, NSError, NSNotification, NSNotificationCenter, NSObject,
    NSPoint, NSProcessInfoPowerStateDidChangeNotification,
    NSProcessInfoThermalStateDidChangeNotification, NSSet, NSString, NSURL, NSUTF8StringEncoding,
};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationAction,
    UNNotificationActionOptions, UNNotificationAttachment, UNNotificationCategory,
    UNNotificationCategoryOptions, UNNotificationDefaultActionIdentifier,
    UNNotificationDismissActionIdentifier, UNNotificationPresentationOptions,
    UNNotificationRequest, UNNotificationResponse, UNNotificationSettings, UNNotificationSound,
    UNNotificationTrigger, UNTextInputNotificationAction, UNTextInputNotificationResponse,
    UNTimeIntervalNotificationTrigger, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use winit::event_loop::EventLoopProxy;

use crate::macos_menu::MacDockMenuHost;
use crate::{
    Menu, NotificationPermissionStatus, OpenUrls, PowerEvent, SystemNotification,
    SystemNotificationAction, SystemNotificationActionKind, SystemNotificationResponse,
    SystemNotificationSound,
    platform::{
        MAX_OPEN_URLS, MAX_OPEN_URLS_TOTAL_BYTES, MAX_PENDING_NOTIFICATION_PERMISSION_REQUESTS,
        MAX_PENDING_SYSTEM_NOTIFICATIONS, MAX_PLATFORM_TEXT_BYTES, MAX_PLATFORM_URL_BYTES,
        MAX_SYSTEM_NOTIFICATION_ACTION_BYTES, MAX_SYSTEM_NOTIFICATION_CATEGORIES,
        MAX_SYSTEM_NOTIFICATION_REPLY_BYTES, MAX_SYSTEM_NOTIFICATION_TAG_BYTES, PlatformError,
        PlatformResponder,
    },
    runtime::RuntimeEvent,
};

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPSNotificationCreateRunLoopSource(
        callback: unsafe extern "C" fn(*mut c_void),
        context: *mut c_void,
    ) -> CFRunLoopSourceRef;
}

/// Opaque libdispatch queue; only the main queue's address is used.
#[repr(C)]
struct DispatchQueue {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    static _dispatch_main_q: DispatchQueue;
    fn dispatch_async_f(
        queue: *const DispatchQueue,
        context: *mut c_void,
        work: unsafe extern "C" fn(*mut c_void),
    );
    fn dispatch_after_f(
        when: u64,
        queue: *const DispatchQueue,
        context: *mut c_void,
        work: unsafe extern "C" fn(*mut c_void),
    );
    fn dispatch_time(when: u64, delta: i64) -> u64;
}

const DISPATCH_TIME_NOW: u64 = 0;
/// Retry period while a modal session or a tracking loop owns the main thread.
const PUMP_INTERRUPT_RETRY_NANOS: i64 = 50_000_000;
/// One interrupt block is in flight; later requests are served by the same block.
static PUMP_INTERRUPT_PENDING: AtomicBool = AtomicBool::new(false);

/// Make a blocking `AppRunner::pump` return, from any thread.
///
/// Winit's event-loop proxy signals a run-loop source and calls `CFRunLoopWakeUp`, but Core
/// Foundation discards wake-ups while the main run loop is not sleeping, which is exactly the
/// state between two pumps and during event dispatch. The signalled source alone never stops
/// `[NSApp run]`: only the post-wait observer does, and a run-loop pass that merely services a
/// source skips that observer. A wake-up lost that way leaves the pump blocked until the next
/// OS event, so an embedding runtime's command would wait for a mouse move.
///
/// A main-queue block travels through the dispatch port, which the run loop services on its
/// next pass whether or not it slept, and stopping the application from inside that block ends
/// the current `[NSApp run]` the same way winit's own pump timeout does. When no pump is running
/// the block waits for the next one, which then returns after one pass.
pub(crate) fn interrupt_pump() {
    if PUMP_INTERRUPT_PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    // SAFETY: the main queue is a process-lifetime global and the work function takes no context.
    unsafe {
        dispatch_async_f(
            &raw const _dispatch_main_q,
            null_mut(),
            stop_application_run,
        )
    }
}

unsafe extern "C" fn stop_application_run(_context: *mut c_void) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    // `stop:` ends whichever AppKit loop is innermost. A modal session (`runModal`) would return
    // without an answer and a tracking loop (menus, live resize, drags) is not `run` either, so
    // wait until the main run loop is back in its default mode before stopping it.
    if !main_run_loop_in_default_mode() {
        // SAFETY: same queue and context contract as `interrupt_pump`.
        unsafe {
            dispatch_after_f(
                dispatch_time(DISPATCH_TIME_NOW, PUMP_INTERRUPT_RETRY_NANOS),
                &raw const _dispatch_main_q,
                null_mut(),
                stop_application_run,
            );
        }
        return;
    }
    PUMP_INTERRUPT_PENDING.store(false, Ordering::Release);
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: `stop:` only sets the flag `run` checks after the current event, and the posted
    // application-defined event is the documented way to make `run` observe it promptly.
    unsafe {
        app.stop(None);
        let event = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
            NSEventType::ApplicationDefined,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags(0),
            0.0,
            0,
            None,
            NSEventSubtype::WindowExposed.0,
            0,
            0,
        );
        if let Some(event) = event {
            app.postEvent_atStart(&event, true);
        }
    }
}

/// Whether the innermost run-loop activity on the main thread is `[NSApp run]` itself.
fn main_run_loop_in_default_mode() -> bool {
    match CFRunLoop::get_main().current_mode() {
        Some(mode) => mode == "kCFRunLoopDefaultMode",
        None => true,
    }
}

struct ApplicationStateIvars {
    proxy: EventLoopProxy<RuntimeEvent>,
    observe_open_urls: bool,
    observe_reopen: bool,
    observe_quit: bool,
    termination_pending: Cell<bool>,
    dock_menu: RefCell<Option<Retained<NSMenu>>>,
}

declare_class!(
    struct QuickGuiApplicationState;

    unsafe impl ClassType for QuickGuiApplicationState {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "QuickGuiApplicationState";
    }

    impl DeclaredClass for QuickGuiApplicationState {
        type Ivars = ApplicationStateIvars;
    }
);

impl QuickGuiApplicationState {
    fn new(
        mtm: MainThreadMarker,
        proxy: EventLoopProxy<RuntimeEvent>,
        observe_open_urls: bool,
        observe_reopen: bool,
        observe_quit: bool,
    ) -> Retained<Self> {
        let allocated = mtm.alloc().set_ivars(ApplicationStateIvars {
            proxy,
            observe_open_urls,
            observe_reopen,
            observe_quit,
            termination_pending: Cell::new(false),
            dock_menu: RefCell::new(None),
        });
        unsafe { msg_send_id![super(allocated), init] }
    }
}

static APPLICATION_DELEGATE_SUBCLASSES: Mutex<Vec<(&'static AnyClass, &'static AnyClass)>> =
    Mutex::new(Vec::new());
static APPLICATION_STATE_ASSOCIATED_OBJECT_KEY: u8 = 0;

fn application_state_associated_object_key() -> *const c_void {
    (&APPLICATION_STATE_ASSOCIATED_OBJECT_KEY as *const u8).cast()
}

fn application_state(delegate: &AnyObject) -> Option<&QuickGuiApplicationState> {
    let state = unsafe {
        objc_getAssociatedObject(
            (delegate as *const AnyObject).cast(),
            application_state_associated_object_key(),
        )
    };
    unsafe { (state as *const QuickGuiApplicationState).as_ref() }
}

unsafe extern "C" fn application_open_urls(
    delegate: &AnyObject,
    _cmd: Sel,
    _application: &NSApplication,
    urls: &NSArray<NSURL>,
) {
    let Some(state) = application_state(delegate) else {
        return;
    };
    if !state.ivars().observe_open_urls {
        return;
    }
    let urls = bounded_open_urls(urls);
    if !urls.is_empty() {
        let _ = state.ivars().proxy.send_event(RuntimeEvent::OpenUrls(urls));
    }
}

unsafe extern "C" fn application_should_handle_reopen(
    delegate: &AnyObject,
    _cmd: Sel,
    _application: &NSApplication,
    has_visible_windows: Bool,
) -> Bool {
    let Some(state) = application_state(delegate) else {
        return Bool::NO;
    };
    if !state.ivars().observe_reopen {
        return Bool::NO;
    }
    let _ = state.ivars().proxy.send_event(RuntimeEvent::Reopen {
        has_visible_windows: has_visible_windows.as_bool(),
    });
    Bool::YES
}

unsafe extern "C" fn application_should_terminate(
    delegate: &AnyObject,
    _cmd: Sel,
    _application: &NSApplication,
) -> NSApplicationTerminateReply {
    let Some(state) = application_state(delegate) else {
        return NSApplicationTerminateReply::NSTerminateNow;
    };
    if !state.ivars().observe_quit {
        return NSApplicationTerminateReply::NSTerminateNow;
    }
    if state.ivars().termination_pending.replace(true) {
        return NSApplicationTerminateReply::NSTerminateLater;
    }
    if state
        .ivars()
        .proxy
        .send_event(RuntimeEvent::QuitRequested)
        .is_err()
    {
        state.ivars().termination_pending.set(false);
        NSApplicationTerminateReply::NSTerminateNow
    } else {
        NSApplicationTerminateReply::NSTerminateLater
    }
}

unsafe extern "C" fn application_dock_menu(
    delegate: &AnyObject,
    _cmd: Sel,
    _application: &NSApplication,
) -> *mut NSMenu {
    let Some(state) = application_state(delegate) else {
        return null_mut();
    };
    state
        .ivars()
        .dock_menu
        .borrow()
        .as_ref()
        .map_or_else(null_mut, |menu| Retained::as_ptr(menu).cast_mut())
}

/// Adds only QuickGUI's optional selectors while preserving Winit's delegate identity, ivars, and
/// inherited lifecycle methods. Winit looks its delegate up dynamically from `NSApplication`, so
/// replacing the object would break run-loop wake and termination handling.
struct MacApplicationDelegateHost {
    delegate: Retained<AnyObject>,
    associated: Retained<QuickGuiApplicationState>,
    previous_class: &'static AnyClass,
}

impl MacApplicationDelegateHost {
    fn new(
        mtm: MainThreadMarker,
        proxy: EventLoopProxy<RuntimeEvent>,
        observe_open_urls: bool,
        observe_reopen: bool,
        observe_quit: bool,
    ) -> Result<Self, String> {
        let application = NSApplication::sharedApplication(mtm);
        let delegate = unsafe { application.delegate() }
            .ok_or_else(|| "Winit did not install its NSApplicationDelegate".to_owned())?;
        let delegate: Retained<AnyObject> = unsafe { Retained::cast(delegate) };
        let existing = unsafe {
            objc_getAssociatedObject(
                Retained::as_ptr(&delegate).cast(),
                application_state_associated_object_key(),
            )
        };
        if !existing.is_null() {
            return Err("QuickGUI application callbacks are already installed".to_owned());
        }

        // Objective-C classes live for the process. This reference is restored before Winit drops
        // its retained delegate at application teardown.
        let previous_class = unsafe { &*(delegate.class() as *const AnyClass) };
        let subclass = {
            let mut subclasses = APPLICATION_DELEGATE_SUBCLASSES
                .lock()
                .map_err(|_| "application-delegate class registry is poisoned".to_owned())?;
            if let Some((_, subclass)) = subclasses
                .iter()
                .find(|(candidate, _)| *candidate == previous_class)
            {
                *subclass
            } else {
                let name = format!("QuickGuiApplicationOf{}", previous_class.name());
                let mut builder = ClassBuilder::new(&name, previous_class).ok_or_else(|| {
                    format!("could not declare Objective-C application subclass {name}")
                })?;
                unsafe {
                    builder.add_method(
                        sel!(application:openURLs:),
                        application_open_urls as unsafe extern "C" fn(_, _, _, _),
                    );
                    builder.add_method(
                        sel!(applicationShouldHandleReopen:hasVisibleWindows:),
                        application_should_handle_reopen as unsafe extern "C" fn(_, _, _, _) -> _,
                    );
                    builder.add_method(
                        sel!(applicationShouldTerminate:),
                        application_should_terminate as unsafe extern "C" fn(_, _, _) -> _,
                    );
                    builder.add_method(
                        sel!(applicationDockMenu:),
                        application_dock_menu as unsafe extern "C" fn(_, _, _) -> _,
                    );
                }
                let subclass = builder.register();
                subclasses.push((previous_class, subclass));
                subclass
            }
        };

        let associated = QuickGuiApplicationState::new(
            mtm,
            proxy,
            observe_open_urls,
            observe_reopen,
            observe_quit,
        );
        unsafe {
            objc_setAssociatedObject(
                Retained::as_ptr(&delegate).cast_mut().cast(),
                application_state_associated_object_key(),
                Retained::as_ptr(&associated).cast_mut().cast(),
                OBJC_ASSOCIATION_RETAIN_NONATOMIC,
            );
            // The dynamic subclass adds no ivars. Winit's concrete delegate remains the same
            // object and still satisfies its `is_kind_of::<ApplicationDelegate>()` checks.
            object_setClass(
                Retained::as_ptr(&delegate).cast_mut().cast(),
                (subclass as *const AnyClass).cast(),
            );
        }
        Ok(Self {
            delegate,
            associated,
            previous_class,
        })
    }
}

impl Drop for MacApplicationDelegateHost {
    fn drop(&mut self) {
        unsafe {
            object_setClass(
                Retained::as_ptr(&self.delegate).cast_mut().cast(),
                (self.previous_class as *const AnyClass).cast(),
            );
            objc_setAssociatedObject(
                Retained::as_ptr(&self.delegate).cast_mut().cast(),
                application_state_associated_object_key(),
                null_mut(),
                OBJC_ASSOCIATION_RETAIN_NONATOMIC,
            );
        }
        // Keep the explicit retained state alive until after the association has been cleared.
        let _ = &self.associated;
    }
}

struct ApplicationObserverIvars {
    proxy: EventLoopProxy<RuntimeEvent>,
}

declare_class!(
    struct QuickGuiApplicationObserver;

    unsafe impl ClassType for QuickGuiApplicationObserver {
        type Super = NSObject;
        type Mutability = InteriorMutable;
        const NAME: &'static str = "QuickGuiApplicationObserver";
    }

    impl DeclaredClass for QuickGuiApplicationObserver {
        type Ivars = ApplicationObserverIvars;
    }

    unsafe impl NSObjectProtocol for QuickGuiApplicationObserver {}

    unsafe impl QuickGuiApplicationObserver {
        #[method(quickGuiSystemDidWake:)]
        fn system_did_wake(&self, _notification: &NSNotification) {
            let _ = self.ivars().proxy.send_event(RuntimeEvent::SystemWake);
            let _ = self
                .ivars()
                .proxy
                .send_event(RuntimeEvent::Power(PowerEvent::Resume));
        }

        #[method(quickGuiSystemWillSleep:)]
        fn system_will_sleep(&self, _notification: &NSNotification) {
            let _ = self
                .ivars()
                .proxy
                .send_event(RuntimeEvent::Power(PowerEvent::Suspend));
        }

        #[method(quickGuiSessionDidResignActive:)]
        fn session_did_resign_active(&self, _notification: &NSNotification) {
            let _ = self
                .ivars()
                .proxy
                .send_event(RuntimeEvent::Power(PowerEvent::LockScreen));
        }

        #[method(quickGuiSessionDidBecomeActive:)]
        fn session_did_become_active(&self, _notification: &NSNotification) {
            let _ = self
                .ivars()
                .proxy
                .send_event(RuntimeEvent::Power(PowerEvent::UnlockScreen));
        }

        #[method(quickGuiSystemWillPowerOff:)]
        fn system_will_power_off(&self, _notification: &NSNotification) {
            let _ = self
                .ivars()
                .proxy
                .send_event(RuntimeEvent::Power(PowerEvent::ShutdownRequested));
        }

        #[method(quickGuiThermalStateDidChange:)]
        fn thermal_state_did_change(&self, _notification: &NSNotification) {
            if let Ok(state) = quickgui_system::PowerMonitor::current_thermal_state() {
                let _ = self.ivars().proxy.send_event(RuntimeEvent::Power(
                    PowerEvent::ThermalStateChanged(state),
                ));
            }
        }

        #[method(quickGuiLowPowerModeDidChange:)]
        fn low_power_mode_did_change(&self, _notification: &NSNotification) {
            if let Ok(snapshot) = quickgui_system::PowerMonitor::snapshot()
                && let Some(enabled) = snapshot.low_power_mode()
            {
                let _ = self.ivars().proxy.send_event(RuntimeEvent::Power(
                    PowerEvent::LowPowerModeChanged(enabled),
                ));
            }
        }

        #[method(quickGuiDisplaysDidChange:)]
        fn displays_did_change(&self, _notification: &NSNotification) {
            let _ = self.ivars().proxy.send_event(RuntimeEvent::DisplaysChanged);
        }

        #[method(quickGuiApplicationDidBecomeActive:)]
        fn application_did_become_active(&self, _notification: &NSNotification) {
            let _ = self
                .ivars()
                .proxy
                .send_event(RuntimeEvent::ApplicationActivated);
        }

        #[method(quickGuiApplicationDidResignActive:)]
        fn application_did_resign_active(&self, _notification: &NSNotification) {
            let _ = self
                .ivars()
                .proxy
                .send_event(RuntimeEvent::ApplicationDeactivated);
        }

        #[method(quickGuiKeyboardLayoutDidChange:)]
        fn keyboard_layout_did_change(&self, _notification: &NSNotification) {
            let _ = self
                .ivars()
                .proxy
                .send_event(RuntimeEvent::KeyboardLayoutChanged);
        }

        #[method(quickGuiSystemPreferencesDidChange:)]
        fn system_preferences_did_change(&self, _notification: &NSNotification) {
            if let Ok(preferences) = quickgui_system::SystemPreferences::snapshot() {
                let _ = self
                    .ivars()
                    .proxy
                    .send_event(RuntimeEvent::SystemPreferencesChanged(preferences));
            }
        }

    }
);

impl QuickGuiApplicationObserver {
    fn new(proxy: EventLoopProxy<RuntimeEvent>) -> Retained<Self> {
        let allocated = Self::alloc().set_ivars(ApplicationObserverIvars { proxy });
        unsafe { msg_send_id![super(allocated), init] }
    }
}

struct PowerSourceObserverState {
    proxy: EventLoopProxy<RuntimeEvent>,
    source: quickgui_system::PowerSource,
}

struct MacPowerSourceObserver {
    state: Box<PowerSourceObserverState>,
    source: CFRunLoopSource,
    run_loop: CFRunLoop,
}

impl MacPowerSourceObserver {
    fn new(proxy: EventLoopProxy<RuntimeEvent>) -> Result<Self, String> {
        let source = quickgui_system::PowerMonitor::snapshot()
            .map(|snapshot| snapshot.source())
            .unwrap_or(quickgui_system::PowerSource::Unknown);
        let mut state = Box::new(PowerSourceObserverState { proxy, source });
        let raw_source = unsafe {
            IOPSNotificationCreateRunLoopSource(
                power_source_changed,
                (&mut *state as *mut PowerSourceObserverState).cast(),
            )
        };
        if raw_source.is_null() {
            return Err("IOKit could not create a power-source run-loop source".to_owned());
        }
        let source = unsafe { CFRunLoopSource::wrap_under_create_rule(raw_source) };
        let run_loop = CFRunLoop::get_main();
        run_loop.add_source(&source, unsafe { kCFRunLoopDefaultMode });
        Ok(Self {
            state,
            source,
            run_loop,
        })
    }
}

impl Drop for MacPowerSourceObserver {
    fn drop(&mut self) {
        self.run_loop
            .remove_source(&self.source, unsafe { kCFRunLoopDefaultMode });
        let _ = &self.state;
    }
}

unsafe extern "C" fn power_source_changed(context: *mut c_void) {
    let Some(state) = (unsafe { context.cast::<PowerSourceObserverState>().as_mut() }) else {
        return;
    };
    let Ok(source) = quickgui_system::PowerMonitor::snapshot().map(|snapshot| snapshot.source())
    else {
        return;
    };
    if source != state.source {
        state.source = source;
        let _ = state
            .proxy
            .send_event(RuntimeEvent::Power(PowerEvent::PowerSourceChanged(source)));
    }
}

struct NotificationDelegateIvars {
    proxy: EventLoopProxy<RuntimeEvent>,
}

type NotificationCategories =
    HashMap<Vec<SystemNotificationAction>, (Arc<str>, Retained<UNNotificationCategory>)>;

declare_class!(
    struct QuickGuiNotificationDelegate;

    unsafe impl ClassType for QuickGuiNotificationDelegate {
        type Super = NSObject;
        type Mutability = InteriorMutable;
        const NAME: &'static str = "QuickGuiNotificationDelegate";
    }

    impl DeclaredClass for QuickGuiNotificationDelegate {
        type Ivars = NotificationDelegateIvars;
    }

    unsafe impl NSObjectProtocol for QuickGuiNotificationDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for QuickGuiNotificationDelegate {
        #[method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:)]
        fn did_receive_notification_response(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion_handler: &Block<dyn Fn()>,
        ) {
            let notification = unsafe { response.notification() };
            let request = unsafe { notification.request() };
            let tag = unsafe { request.identifier() };
            let action = unsafe { response.actionIdentifier() };
            let tag = bounded_string(&tag, MAX_SYSTEM_NOTIFICATION_TAG_BYTES);
            let action_id = if action.as_ref() == unsafe { UNNotificationDefaultActionIdentifier }
            {
                Some(None)
            } else if action.as_ref() == unsafe { UNNotificationDismissActionIdentifier } {
                None
            } else {
                bounded_string(&action, MAX_SYSTEM_NOTIFICATION_ACTION_BYTES).map(Some)
            };
            let reply = if response.isKindOfClass(UNTextInputNotificationResponse::class()) {
                // `isKindOfClass:` proves this Objective-C object has the text-response layout.
                let response = unsafe {
                    &*(std::ptr::from_ref(response).cast::<UNTextInputNotificationResponse>())
                };
                let user_text = unsafe { response.userText() };
                bounded_string(&user_text, MAX_SYSTEM_NOTIFICATION_REPLY_BYTES)
            } else {
                None
            };
            if let (Some(tag), Some(action_id)) = (tag, action_id) {
                let _ = self.ivars().proxy.send_event(
                    RuntimeEvent::SystemNotificationResponse(SystemNotificationResponse {
                        tag,
                        action_id,
                        reply,
                    }),
                );
            }
            completion_handler.call(());
        }

        #[method(userNotificationCenter:willPresentNotification:withCompletionHandler:)]
        fn will_present_notification(
            &self,
            _center: &UNUserNotificationCenter,
            notification: &UNNotification,
            completion_handler: &Block<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            let mut options =
                UNNotificationPresentationOptions::UNNotificationPresentationOptionBanner
                    | UNNotificationPresentationOptions::UNNotificationPresentationOptionList;
            let content = unsafe { notification.request().content() };
            if unsafe { content.sound() }.is_some() {
                options |=
                    UNNotificationPresentationOptions::UNNotificationPresentationOptionSound;
            }
            completion_handler.call((options,));
        }
    }
);

impl QuickGuiNotificationDelegate {
    fn new(proxy: EventLoopProxy<RuntimeEvent>) -> Retained<QuickGuiNotificationDelegate> {
        let allocated = Self::alloc().set_ivars(NotificationDelegateIvars { proxy });
        unsafe { msg_send_id![super(allocated), init] }
    }
}

struct MacSystemNotificationCenter {
    center: Retained<UNUserNotificationCenter>,
    // `UNUserNotificationCenter.delegate` is weak.
    _delegate: Retained<QuickGuiNotificationDelegate>,
    categories: RefCell<NotificationCategories>,
    authorization: Cell<NotificationAuthorization>,
    pending: RefCell<Vec<SystemNotification>>,
    pending_permission_status: RefCell<Vec<PlatformResponder<NotificationPermissionStatus>>>,
    pending_permission_requests: RefCell<Vec<PlatformResponder<NotificationPermissionStatus>>>,
    proxy: EventLoopProxy<RuntimeEvent>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NotificationAuthorization {
    NotRequested,
    Pending,
    Granted,
    Denied,
}

impl MacSystemNotificationCenter {
    fn new(proxy: EventLoopProxy<RuntimeEvent>) -> Option<Self> {
        // UserNotifications aborts an unbundled `cargo run` process while resolving its bundle
        // proxy. A real bundle identifier is therefore a required native precondition.
        if unsafe { NSBundle::mainBundle().bundleIdentifier() }.is_none() {
            tracing::info!("system notifications are unavailable outside an application bundle");
            return None;
        }

        let center = unsafe { UNUserNotificationCenter::currentNotificationCenter() };
        let delegate = QuickGuiNotificationDelegate::new(proxy.clone());
        unsafe {
            center.setDelegate(Some(ProtocolObject::from_ref(delegate.as_ref())));
        }
        Some(Self {
            center,
            _delegate: delegate,
            categories: RefCell::new(HashMap::new()),
            authorization: Cell::new(NotificationAuthorization::NotRequested),
            pending: RefCell::new(Vec::with_capacity(4)),
            pending_permission_status: RefCell::new(Vec::with_capacity(2)),
            pending_permission_requests: RefCell::new(Vec::with_capacity(2)),
            proxy,
        })
    }

    fn permission_status(&self, responder: PlatformResponder<NotificationPermissionStatus>) {
        let mut pending = self.pending_permission_status.borrow_mut();
        pending.retain(|responder| !responder.is_cancelled());
        if pending.len() == MAX_PENDING_NOTIFICATION_PERMISSION_REQUESTS {
            responder.complete(Err(PlatformError::PendingQueueFull));
            return;
        }
        let start_query = pending.is_empty();
        pending.push(responder);
        drop(pending);
        if !start_query {
            return;
        }

        let proxy = self.proxy.clone();
        let completion = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            let status =
                notification_permission_status(unsafe { settings.as_ref().authorizationStatus() });
            let _ = proxy.send_event(RuntimeEvent::SystemNotificationPermissionStatus(status));
        });
        unsafe {
            self.center
                .getNotificationSettingsWithCompletionHandler(&completion);
        }
    }

    fn request_permission(&self, responder: PlatformResponder<NotificationPermissionStatus>) {
        let mut pending = self.pending_permission_requests.borrow_mut();
        pending.retain(|responder| !responder.is_cancelled());
        if pending.len() == MAX_PENDING_NOTIFICATION_PERMISSION_REQUESTS {
            responder.complete(Err(PlatformError::PendingQueueFull));
            return;
        }
        pending.push(responder);
        drop(pending);
        if self.authorization.get() != NotificationAuthorization::Pending {
            self.authorization.set(NotificationAuthorization::Pending);
            self.request_authorization();
        }
    }

    fn request_authorization(&self) {
        debug_assert_eq!(self.authorization.get(), NotificationAuthorization::Pending);
        let proxy = self.proxy.clone();
        let completion = RcBlock::new(move |granted: Bool, error: *mut NSError| {
            let error = unsafe { error.as_ref() }.and_then(|error| {
                bounded_string(&error.localizedDescription(), MAX_PLATFORM_TEXT_BYTES)
            });
            let granted = granted.as_bool() && error.is_none();
            let _ =
                proxy.send_event(RuntimeEvent::SystemNotificationAuthorization { granted, error });
        });
        unsafe {
            self.center
                .requestAuthorizationWithOptions_completionHandler(
                    UNAuthorizationOptions::UNAuthorizationOptionAlert
                        | UNAuthorizationOptions::UNAuthorizationOptionSound,
                    &completion,
                );
        }
    }

    fn show(&self, notification: SystemNotification) {
        match self.authorization.get() {
            NotificationAuthorization::NotRequested => {
                self.enqueue_pending(notification);
                self.authorization.set(NotificationAuthorization::Pending);
                self.request_authorization();
            }
            NotificationAuthorization::Pending => self.enqueue_pending(notification),
            NotificationAuthorization::Granted => self.post(notification),
            NotificationAuthorization::Denied => {}
        }
    }

    fn enqueue_pending(&self, notification: SystemNotification) {
        let mut pending = self.pending.borrow_mut();
        if enqueue_pending_system_notification(&mut pending, notification) {
            tracing::warn!(
                maximum = MAX_PENDING_SYSTEM_NOTIFICATIONS,
                "system notification authorization queue is full; replacing its oldest tag"
            );
        }
    }

    fn complete_authorization(&self, granted: bool, error: Option<Arc<str>>) {
        if self.authorization.get() != NotificationAuthorization::Pending {
            return;
        }
        if let Some(ref error) = error {
            tracing::warn!(%error, "system notification authorization failed");
        } else if !granted {
            tracing::info!("system notification authorization was denied");
        }
        let status = if granted {
            NotificationPermissionStatus::Granted
        } else {
            NotificationPermissionStatus::Denied
        };
        self.authorization.set(if granted {
            NotificationAuthorization::Granted
        } else if error.is_some() {
            NotificationAuthorization::NotRequested
        } else {
            NotificationAuthorization::Denied
        });

        let responders = std::mem::take(&mut *self.pending_permission_requests.borrow_mut());
        for responder in responders {
            if let Some(error) = &error {
                responder.complete(Err(PlatformError::Platform(error.clone())));
            } else {
                responder.complete(Ok(status));
            }
        }

        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        if granted {
            for notification in pending {
                self.post(notification);
            }
        }
    }

    fn complete_permission_status(&self, status: NotificationPermissionStatus) {
        if self.authorization.get() != NotificationAuthorization::Pending {
            self.authorization.set(match status {
                NotificationPermissionStatus::Granted => NotificationAuthorization::Granted,
                NotificationPermissionStatus::Denied => NotificationAuthorization::Denied,
                NotificationPermissionStatus::NotDetermined
                | NotificationPermissionStatus::Unsupported => {
                    NotificationAuthorization::NotRequested
                }
            });
        }
        for responder in std::mem::take(&mut *self.pending_permission_status.borrow_mut()) {
            responder.complete(Ok(status));
        }
    }

    fn post(&self, notification: SystemNotification) {
        let content = unsafe { UNMutableNotificationContent::new() };
        unsafe {
            content.setTitle(&NSString::from_str(&notification.title));
            if let Some(subtitle) = &notification.subtitle {
                content.setSubtitle(&NSString::from_str(subtitle));
            }
            content.setBody(&NSString::from_str(&notification.body));
            match &notification.sound {
                SystemNotificationSound::Default => {
                    let sound = UNNotificationSound::defaultSound();
                    content.setSound(Some(&sound));
                }
                SystemNotificationSound::Silent => content.setSound(None),
                SystemNotificationSound::Named(name) => {
                    let sound = UNNotificationSound::soundNamed(&NSString::from_str(name));
                    content.setSound(Some(&sound));
                }
            }
        }

        let mut attachments = Vec::with_capacity(
            notification.attachments.len() + usize::from(notification.icon.is_some()),
        );
        if let Some(path) = &notification.icon
            && let Some(attachment) = native_notification_attachment("quickgui-icon", path)
        {
            attachments.push(attachment);
        }
        for attachment in &notification.attachments {
            let identifier = format!("quickgui-attachment-{}", attachment.id);
            if let Some(attachment) = native_notification_attachment(&identifier, &attachment.path)
            {
                attachments.push(attachment);
            }
        }
        if !attachments.is_empty() {
            let attachments = NSArray::from_vec(attachments);
            unsafe { content.setAttachments(&attachments) };
        }
        if !notification.actions.is_empty() {
            if let Some(identifier) = self.register_category(&notification.actions) {
                unsafe {
                    content.setCategoryIdentifier(&NSString::from_str(&identifier));
                }
            } else {
                tracing::warn!(
                    maximum = MAX_SYSTEM_NOTIFICATION_CATEGORIES,
                    "system notification action-category capacity reached; posting without actions"
                );
            }
        }

        let trigger: Option<Retained<UNNotificationTrigger>> = notification
            .delivery_at
            .and_then(|delivery_at| delivery_at.duration_since(web_time::SystemTime::now()).ok())
            .filter(|delay| !delay.is_zero())
            .map(|delay| unsafe {
                Retained::into_super(
                    UNTimeIntervalNotificationTrigger::triggerWithTimeInterval_repeats(
                        delay.as_secs_f64().max(1.0),
                        false,
                    ),
                )
            });
        let request = unsafe {
            UNNotificationRequest::requestWithIdentifier_content_trigger(
                &NSString::from_str(&notification.tag),
                &content,
                trigger.as_deref(),
            )
        };
        let completion = RcBlock::new(|error: *mut NSError| {
            if let Some(error) = unsafe { error.as_ref() } {
                tracing::warn!(
                    error = %error.localizedDescription(),
                    "could not deliver system notification"
                );
            }
        });
        unsafe {
            self.center
                .addNotificationRequest_withCompletionHandler(&request, Some(&completion));
        }
    }

    fn register_category(&self, actions: &[SystemNotificationAction]) -> Option<Arc<str>> {
        let mut categories = self.categories.borrow_mut();
        if let Some((identifier, _)) = categories.get(actions) {
            return Some(identifier.clone());
        }
        if categories.len() == MAX_SYSTEM_NOTIFICATION_CATEGORIES {
            return None;
        }

        let identifier: Arc<str> =
            Arc::from(format!("quickgui-system-notification-{}", categories.len()));
        let native_actions = actions
            .iter()
            .map(|action| unsafe {
                match &action.kind {
                    SystemNotificationActionKind::Button => {
                        UNNotificationAction::actionWithIdentifier_title_options(
                            &NSString::from_str(&action.id),
                            &NSString::from_str(&action.label),
                            UNNotificationActionOptions::empty(),
                        )
                    }
                    SystemNotificationActionKind::TextInput { placeholder } => {
                        Retained::into_super(
                            UNTextInputNotificationAction::actionWithIdentifier_title_options_textInputButtonTitle_textInputPlaceholder(
                                &NSString::from_str(&action.id),
                                &NSString::from_str(&action.label),
                                UNNotificationActionOptions::empty(),
                                &NSString::from_str(&action.label),
                                &NSString::from_str(placeholder.as_deref().unwrap_or("")),
                            ),
                        )
                    }
                }
            })
            .collect();
        let native_actions = NSArray::from_vec(native_actions);
        let category = unsafe {
            UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
                &NSString::from_str(&identifier),
                &native_actions,
                &NSArray::new(),
                UNNotificationCategoryOptions::empty(),
            )
        };
        categories.insert(actions.to_vec(), (identifier.clone(), category));

        let all = categories
            .values()
            .map(|(_, category)| category.clone())
            .collect();
        let all = NSArray::from_vec(all);
        let all = unsafe { NSSet::setWithArray(&all) };
        unsafe {
            self.center.setNotificationCategories(&all);
        }
        Some(identifier)
    }

    fn dismiss(&self, tag: &str) {
        self.pending
            .borrow_mut()
            .retain(|notification| notification.tag.as_ref() != tag);
        let identifiers = NSArray::from_vec(vec![NSString::from_str(tag)]);
        unsafe {
            self.center
                .removePendingNotificationRequestsWithIdentifiers(&identifiers);
            self.center
                .removeDeliveredNotificationsWithIdentifiers(&identifiers);
        }
    }
}

impl Drop for MacSystemNotificationCenter {
    fn drop(&mut self) {
        unsafe {
            self.center.setDelegate(None);
        }
    }
}

/// Owns AppKit application callbacks and lazily initialized UserNotifications state.
pub(crate) struct MacApplicationHost {
    application_delegate: Option<MacApplicationDelegateHost>,
    application_notifications: Retained<NSNotificationCenter>,
    application_observer: Retained<QuickGuiApplicationObserver>,
    workspace_notifications: Option<Retained<NSNotificationCenter>>,
    power_source_observer: Option<MacPowerSourceObserver>,
    notifications_initialized: bool,
    notifications: Option<MacSystemNotificationCenter>,
    dock_menu: Option<MacDockMenuHost>,
    proxy: EventLoopProxy<RuntimeEvent>,
}

impl MacApplicationHost {
    pub(crate) fn new(
        proxy: EventLoopProxy<RuntimeEvent>,
        observe_open_urls: bool,
        observe_reopen: bool,
        observe_quit: bool,
        observe_power_events: bool,
        observe_notification_responses: bool,
    ) -> Result<Self, String> {
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            "the AppKit application host must be installed on the main thread".to_owned()
        })?;
        // The delegate host also owns the optional Dock-menu selector, so install it even when the
        // application did not register lifecycle callbacks.
        let application_delegate = Some(MacApplicationDelegateHost::new(
            mtm,
            proxy.clone(),
            observe_open_urls,
            observe_reopen,
            observe_quit,
        )?);

        let application_observer = QuickGuiApplicationObserver::new(proxy.clone());
        let application_notifications = unsafe { NSNotificationCenter::defaultCenter() };
        unsafe {
            application_notifications.addObserver_selector_name_object(
                application_observer.as_ref(),
                sel!(quickGuiDisplaysDidChange:),
                Some(NSApplicationDidChangeScreenParametersNotification),
                None,
            );
            // A regular command-line app receives its Dock presence only after AppKit finishes
            // launching. A left or right Dock can change `NSScreen.visibleFrame` at that point
            // without emitting a screen-parameters notification. Refreshing at activation keeps
            // the immutable work-area snapshot authoritative, while equality suppression makes
            // every unchanged activation free of view invalidation or redraw work.
            application_notifications.addObserver_selector_name_object(
                application_observer.as_ref(),
                sel!(quickGuiDisplaysDidChange:),
                Some(NSApplicationDidBecomeActiveNotification),
                None,
            );
            application_notifications.addObserver_selector_name_object(
                application_observer.as_ref(),
                sel!(quickGuiApplicationDidBecomeActive:),
                Some(NSApplicationDidBecomeActiveNotification),
                None,
            );
            application_notifications.addObserver_selector_name_object(
                application_observer.as_ref(),
                sel!(quickGuiApplicationDidResignActive:),
                Some(NSApplicationDidResignActiveNotification),
                None,
            );
            let keyboard_layout_notification =
                NSString::from_str("NSTextInputContextKeyboardSelectionDidChangeNotification");
            application_notifications.addObserver_selector_name_object(
                application_observer.as_ref(),
                sel!(quickGuiKeyboardLayoutDidChange:),
                Some(&keyboard_layout_notification),
                None,
            );
            if observe_power_events {
                application_notifications.addObserver_selector_name_object(
                    application_observer.as_ref(),
                    sel!(quickGuiThermalStateDidChange:),
                    Some(NSProcessInfoThermalStateDidChangeNotification),
                    None,
                );
                application_notifications.addObserver_selector_name_object(
                    application_observer.as_ref(),
                    sel!(quickGuiLowPowerModeDidChange:),
                    Some(NSProcessInfoPowerStateDidChangeNotification),
                    None,
                );
            }
        }

        let workspace_notifications = unsafe {
            let center = NSWorkspace::sharedWorkspace().notificationCenter();
            center.addObserver_selector_name_object(
                application_observer.as_ref(),
                sel!(quickGuiSystemPreferencesDidChange:),
                Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                None,
            );
            if observe_power_events {
                center.addObserver_selector_name_object(
                    application_observer.as_ref(),
                    sel!(quickGuiSystemDidWake:),
                    Some(NSWorkspaceDidWakeNotification),
                    None,
                );
                center.addObserver_selector_name_object(
                    application_observer.as_ref(),
                    sel!(quickGuiSystemWillSleep:),
                    Some(NSWorkspaceWillSleepNotification),
                    None,
                );
                center.addObserver_selector_name_object(
                    application_observer.as_ref(),
                    sel!(quickGuiSystemWillPowerOff:),
                    Some(NSWorkspaceWillPowerOffNotification),
                    None,
                );
                center.addObserver_selector_name_object(
                    application_observer.as_ref(),
                    sel!(quickGuiSessionDidResignActive:),
                    Some(NSWorkspaceSessionDidResignActiveNotification),
                    None,
                );
                center.addObserver_selector_name_object(
                    application_observer.as_ref(),
                    sel!(quickGuiSessionDidBecomeActive:),
                    Some(NSWorkspaceSessionDidBecomeActiveNotification),
                    None,
                );
            }
            Some(center)
        };
        let power_source_observer = if observe_power_events {
            match MacPowerSourceObserver::new(proxy.clone()) {
                Ok(observer) => Some(observer),
                Err(error) => {
                    tracing::warn!(%error, "macOS power-source monitoring is unavailable");
                    None
                }
            }
        } else {
            None
        };

        let mut host = Self {
            application_delegate,
            application_notifications,
            application_observer,
            workspace_notifications,
            power_source_observer,
            notifications_initialized: false,
            notifications: None,
            dock_menu: None,
            proxy,
        };
        if observe_notification_responses {
            host.ensure_notifications();
        }
        Ok(host)
    }

    pub(crate) fn reply_to_application_should_terminate(&self, terminate: bool) {
        let Some(delegate) = self.application_delegate.as_ref() else {
            return;
        };
        delegate.associated.ivars().termination_pending.set(false);
        let mtm = MainThreadMarker::new()
            .expect("termination replies are dispatched on the AppKit application thread");
        let application = NSApplication::sharedApplication(mtm);
        // SAFETY: This balances one preceding `NSTerminateLater` response on the same application
        // thread and retains no Rust pointer.
        unsafe { application.replyToApplicationShouldTerminate(terminate) };
    }

    pub(crate) fn set_dock_menu(
        &mut self,
        menu: Option<Menu>,
        proxy: EventLoopProxy<RuntimeEvent>,
    ) -> Result<(), String> {
        let next = menu
            .as_ref()
            .map(|menu| MacDockMenuHost::new(menu, proxy))
            .transpose()?;
        let delegate = self
            .application_delegate
            .as_ref()
            .ok_or_else(|| "the AppKit application delegate is unavailable".to_owned())?;
        delegate
            .associated
            .ivars()
            .dock_menu
            .replace(next.as_ref().map(MacDockMenuHost::native_retained));
        self.dock_menu = next;
        Ok(())
    }

    pub(crate) fn show_system_notification(&mut self, notification: SystemNotification) {
        self.ensure_notifications();
        if let Some(center) = &self.notifications {
            center.show(notification);
        }
    }

    pub(crate) fn dismiss_system_notification(&mut self, tag: &str) {
        self.ensure_notifications();
        if let Some(center) = &self.notifications {
            center.dismiss(tag);
        }
    }

    pub(crate) fn complete_system_notification_authorization(
        &mut self,
        granted: bool,
        error: Option<Arc<str>>,
    ) {
        if let Some(center) = &self.notifications {
            center.complete_authorization(granted, error);
        }
    }

    pub(crate) fn system_notification_permission_status(
        &mut self,
        responder: PlatformResponder<NotificationPermissionStatus>,
    ) {
        self.ensure_notifications();
        if let Some(center) = &self.notifications {
            center.permission_status(responder);
        } else {
            responder.complete(Ok(NotificationPermissionStatus::Unsupported));
        }
    }

    pub(crate) fn request_system_notification_permission(
        &mut self,
        responder: PlatformResponder<NotificationPermissionStatus>,
    ) {
        self.ensure_notifications();
        if let Some(center) = &self.notifications {
            center.request_permission(responder);
        } else {
            responder.complete(Ok(NotificationPermissionStatus::Unsupported));
        }
    }

    pub(crate) fn complete_system_notification_permission_status(
        &mut self,
        status: NotificationPermissionStatus,
    ) {
        if let Some(center) = &self.notifications {
            center.complete_permission_status(status);
        }
    }

    fn ensure_notifications(&mut self) {
        if self.notifications_initialized {
            return;
        }
        self.notifications_initialized = true;
        self.notifications = MacSystemNotificationCenter::new(self.proxy.clone());
    }
}

fn notification_permission_status(
    status: objc2_user_notifications::UNAuthorizationStatus,
) -> NotificationPermissionStatus {
    use objc2_user_notifications::UNAuthorizationStatus;
    match status {
        UNAuthorizationStatus::NotDetermined => NotificationPermissionStatus::NotDetermined,
        UNAuthorizationStatus::Denied => NotificationPermissionStatus::Denied,
        UNAuthorizationStatus::Authorized
        | UNAuthorizationStatus::Provisional
        | UNAuthorizationStatus::Ephemeral => NotificationPermissionStatus::Granted,
        _ => NotificationPermissionStatus::Unsupported,
    }
}

impl Drop for MacApplicationHost {
    fn drop(&mut self) {
        self.power_source_observer.take();
        unsafe {
            self.application_notifications
                .removeObserver(self.application_observer.as_ref());
        }
        if let Some(center) = self.workspace_notifications.take() {
            unsafe {
                center.removeObserver(self.application_observer.as_ref());
            }
        }
        drop(self.application_delegate.take());
    }
}

fn bounded_open_urls(urls: &NSArray<NSURL>) -> OpenUrls {
    let mut retained = Vec::with_capacity(urls.len().min(MAX_OPEN_URLS));
    let mut total = 0_usize;
    for index in 0..urls.len().min(MAX_OPEN_URLS) {
        let Some(url) = urls.get(index) else {
            continue;
        };
        let Some(absolute) = (unsafe { url.absoluteString() }) else {
            continue;
        };
        let Some(url) = bounded_string(&absolute, MAX_PLATFORM_URL_BYTES) else {
            continue;
        };
        let Some(next_total) = total.checked_add(url.len()) else {
            break;
        };
        if next_total > MAX_OPEN_URLS_TOTAL_BYTES {
            break;
        }
        total = next_total;
        retained.push(url);
    }
    OpenUrls::from_bounded(retained)
}

fn bounded_string(value: &NSString, maximum: usize) -> Option<Arc<str>> {
    if value.lengthOfBytesUsingEncoding(NSUTF8StringEncoding) > maximum {
        return None;
    }
    let value = value.to_string();
    if value.len() > maximum || value.contains('\0') {
        None
    } else {
        Some(Arc::from(value))
    }
}

fn native_notification_attachment(
    identifier: &str,
    path: &std::path::Path,
) -> Option<Retained<UNNotificationAttachment>> {
    let url = match crate::macos::native_file_url(path, false) {
        Ok(url) => url,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "could not create notification attachment URL");
            return None;
        }
    };
    match unsafe {
        UNNotificationAttachment::attachmentWithIdentifier_URL_options_error(
            &NSString::from_str(identifier),
            &url,
            None,
        )
    } {
        Ok(attachment) => Some(attachment),
        Err(error) => {
            tracing::warn!(
                error = %error.localizedDescription(),
                path = %path.display(),
                "could not attach a local file to a system notification"
            );
            None
        }
    }
}

/// Queue one post while authorization is unresolved. Returns whether the oldest distinct tag had
/// to be evicted. Repeated tags replace in place so a burst cannot defeat replacement semantics.
fn enqueue_pending_system_notification(
    pending: &mut Vec<SystemNotification>,
    notification: SystemNotification,
) -> bool {
    if let Some(existing) = pending
        .iter_mut()
        .find(|existing| existing.tag == notification.tag)
    {
        *existing = notification;
        return false;
    }
    let evicted = pending.len() == MAX_PENDING_SYSTEM_NOTIFICATIONS;
    if evicted {
        pending.remove(0);
    }
    pending.push(notification);
    evicted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_strings_are_bounded_before_rust_retention() {
        assert_eq!(
            bounded_string(&NSString::from_str("quickgui://open"), 32).as_deref(),
            Some("quickgui://open")
        );
        assert!(bounded_string(&NSString::from_str("too long"), 4).is_none());
        assert!(bounded_string(&NSString::from_str("bad\0value"), 32).is_none());
    }

    #[test]
    fn pending_notification_authorization_is_bounded_and_coalesces_tags() {
        let mut pending = Vec::new();
        assert!(!enqueue_pending_system_notification(
            &mut pending,
            SystemNotification::new("build", "First", "Body"),
        ));
        assert!(!enqueue_pending_system_notification(
            &mut pending,
            SystemNotification::new("build", "Replacement", "Body"),
        ));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].title.as_ref(), "Replacement");

        for index in 1..MAX_PENDING_SYSTEM_NOTIFICATIONS {
            assert!(!enqueue_pending_system_notification(
                &mut pending,
                SystemNotification::new(format!("tag-{index}"), "Title", "Body"),
            ));
        }
        assert_eq!(pending.len(), MAX_PENDING_SYSTEM_NOTIFICATIONS);
        assert!(enqueue_pending_system_notification(
            &mut pending,
            SystemNotification::new("newest", "Title", "Body"),
        ));
        assert_eq!(pending.len(), MAX_PENDING_SYSTEM_NOTIFICATIONS);
        assert!(
            pending
                .iter()
                .all(|notification| notification.tag.as_ref() != "build")
        );
        assert_eq!(pending.last().unwrap().tag.as_ref(), "newest");
    }
}
