use super::*;

const NATIVE_DROP_TEXT: u8 = 1 << 0;
const NATIVE_DROP_URL: u8 = 1 << 1;
const NATIVE_DROP_TYPED: u8 = 1 << 2;
const QUICKGUI_TYPED_DRAG_TYPE: &str = "dev.quickgui.typed-drag-token";
const MAX_COMPOSITE_PASTEBOARD_TYPES: usize = 32;
pub(crate) const MAX_NATIVE_TYPED_DRAG_SESSIONS: usize = 256;

static NATIVE_DROP_SUBCLASSES: Mutex<Vec<(&'static AnyClass, &'static AnyClass)>> =
    Mutex::new(Vec::new());
static NATIVE_DROP_ASSOCIATED_OBJECT_KEY: u8 = 0;

fn native_drop_associated_object_key() -> *const c_void {
    (&NATIVE_DROP_ASSOCIATED_OBJECT_KEY as *const u8).cast()
}

#[derive(Clone)]
pub(crate) struct MacTypedDragPayload {
    pub(super) value: Arc<dyn Any>,
    pub(super) value_type: TypeId,
    pub(super) source_window: WindowHandle,
    pub(super) source: ElementId,
}

impl MacTypedDragPayload {
    pub(crate) fn new(
        value: Arc<dyn Any>,
        value_type: TypeId,
        source_window: WindowHandle,
        source: ElementId,
    ) -> Self {
        let actual_type = value.as_ref().type_id();
        debug_assert_eq!(actual_type, value_type);
        Self {
            value,
            value_type: actual_type,
            source_window,
            source,
        }
    }

    pub(crate) fn source_window(&self) -> WindowHandle {
        self.source_window
    }

    pub(crate) fn source(&self) -> ElementId {
        self.source
    }
}

pub(super) struct MacTypedDragRegistryInner {
    pub(super) payloads: HashMap<Arc<str>, MacTypedDragPayload>,
}

/// Main-thread registry that keeps arbitrary Rust drag values out of the native pasteboard.
#[derive(Clone)]
pub(crate) struct MacTypedDragRegistry {
    pub(super) inner: Rc<RefCell<MacTypedDragRegistryInner>>,
    pasteboard_type: Retained<NSString>,
}

impl MacTypedDragRegistry {
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(MacTypedDragRegistryInner {
                payloads: HashMap::with_capacity(8),
            })),
            pasteboard_type: NSString::from_str(QUICKGUI_TYPED_DRAG_TYPE),
        }
    }

    pub(super) fn register(
        &self,
        payload: MacTypedDragPayload,
    ) -> Result<(Arc<str>, MacTypedDragRegistration), String> {
        let mut inner = self.inner.borrow_mut();
        if inner.payloads.len() >= MAX_NATIVE_TYPED_DRAG_SESSIONS {
            return Err(format!(
                "the application already owns {MAX_NATIVE_TYPED_DRAG_SESSIONS} native typed drag sessions"
            ));
        }
        let token = (0..8)
            .find_map(|_| {
                let token: Arc<str> = Arc::from(NSUUID::UUID().UUIDString().to_string());
                (!inner.payloads.contains_key(token.as_ref())).then_some(token)
            })
            .ok_or_else(|| "could not allocate a unique native typed drag token".to_owned())?;
        inner.payloads.insert(token.clone(), payload);
        drop(inner);
        Ok((
            token.clone(),
            MacTypedDragRegistration {
                registry: Rc::downgrade(&self.inner),
                token,
            },
        ))
    }

    pub(super) fn resolve(&self, token: &str) -> Option<MacTypedDragPayload> {
        self.inner.borrow().payloads.get(token).cloned()
    }
}

pub(super) struct MacTypedDragRegistration {
    registry: Weak<RefCell<MacTypedDragRegistryInner>>,
    token: Arc<str>,
}

impl Drop for MacTypedDragRegistration {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            registry.borrow_mut().payloads.remove(self.token.as_ref());
        }
    }
}

/// A bounded native payload accepted by the same exact-type listener path as internal drags.
#[derive(Clone)]
pub(crate) enum MacNativeDropPayload {
    Typed(MacTypedDragPayload),
    Text(Arc<ExternalDragText>),
    Url(Arc<ExternalDragUrl>),
}

impl MacNativeDropPayload {
    pub(crate) fn value_type(&self) -> TypeId {
        match self {
            Self::Typed(value) => value.value_type,
            Self::Text(_) => TypeId::of::<ExternalDragText>(),
            Self::Url(_) => TypeId::of::<ExternalDragUrl>(),
        }
    }

    pub(crate) fn value(&self) -> &dyn Any {
        match self {
            Self::Typed(value) => value.value.as_ref(),
            Self::Text(value) => value.as_ref(),
            Self::Url(value) => value.as_ref(),
        }
    }
}

fn native_drop_payload_ref(payload: &MacNativeDropPayload) -> (TypeId, &dyn Any) {
    (payload.value_type(), payload.value())
}

#[derive(Clone)]
pub(crate) struct MacNativeDropOffer {
    payloads: Arc<[MacNativeDropPayload]>,
}

impl MacNativeDropOffer {
    pub(super) fn new(payloads: Vec<MacNativeDropPayload>) -> Option<Self> {
        (!payloads.is_empty()).then(|| Self {
            payloads: Arc::from(payloads),
        })
    }

    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = (TypeId, &dyn Any)> + Clone {
        self.payloads.iter().map(
            native_drop_payload_ref
                as for<'a> fn(&'a MacNativeDropPayload) -> (TypeId, &'a dyn Any),
        )
    }

    pub(crate) fn payload(&self, index: usize) -> Option<&MacNativeDropPayload> {
        self.payloads.get(index)
    }
}

pub(crate) enum MacNativeDropPending {
    Hover {
        offer: MacNativeDropOffer,
        point: Point,
    },
    Exit,
    Drop {
        offer: MacNativeDropOffer,
        point: Point,
    },
}

enum NativeDropMode {
    None,
    File,
    Offer(MacNativeDropOffer),
}

struct NativeDropIvars {
    previous_class: &'static AnyClass,
    content_view: Retained<NSView>,
    window: WindowHandle,
    proxy: EventLoopProxy<RuntimeEvent>,
    typed_registry: MacTypedDragRegistry,
    capabilities: Cell<u8>,
    snapshot: RefCell<ExternalDropSnapshot>,
    active: RefCell<NativeDropMode>,
    last_point: Cell<Option<Point>>,
    last_target: Cell<Option<(ElementId, usize)>>,
    pending: RefCell<Option<MacNativeDropPending>>,
    event_queued: Cell<bool>,
}

declare_class!(
    struct QuickGuiNativeDropState;

    unsafe impl ClassType for QuickGuiNativeDropState {
        type Super = NSObject;
        type Mutability = InteriorMutable;
        const NAME: &'static str = "QuickGuiNativeDropState";
    }

    impl DeclaredClass for QuickGuiNativeDropState {
        type Ivars = NativeDropIvars;
    }
);

impl QuickGuiNativeDropState {
    fn new(
        previous_class: &'static AnyClass,
        content_view: Retained<NSView>,
        window: WindowHandle,
        proxy: EventLoopProxy<RuntimeEvent>,
        typed_registry: MacTypedDragRegistry,
    ) -> Retained<Self> {
        let this = Self::alloc().set_ivars(NativeDropIvars {
            previous_class,
            content_view,
            window,
            proxy,
            typed_registry,
            capabilities: Cell::new(0),
            snapshot: RefCell::new(ExternalDropSnapshot::new()),
            active: RefCell::new(NativeDropMode::None),
            last_point: Cell::new(None),
            last_target: Cell::new(None),
            pending: RefCell::new(None),
            event_queued: Cell::new(false),
        });
        unsafe { msg_send_id![super(this), init] }
    }

    fn point(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> Option<Point> {
        let window_point = unsafe { sender.draggingLocation() };
        let point = self
            .ivars()
            .content_view
            .convertPoint_fromView(window_point, None);
        (point.x.is_finite() && point.y.is_finite())
            .then(|| Point::new(point.x as f32, point.y as f32))
    }

    fn target(&self, offer: &MacNativeDropOffer, point: Point) -> Option<(ElementId, usize)> {
        self.ivars()
            .snapshot
            .borrow()
            .offer_target_at(point, offer.iter())
    }

    /// Queue runtime work only when the native target changes; a drop carries its exact point.
    fn hover(&self, offer: &MacNativeDropOffer, point: Point, force: bool) -> bool {
        let target = self.target(offer, point);
        let first = self.ivars().last_point.replace(Some(point)).is_none();
        let changed = self.ivars().last_target.replace(target) != target;
        if force || first || changed {
            self.queue(MacNativeDropPending::Hover {
                offer: offer.clone(),
                point,
            });
        }
        target.is_some()
    }

    fn reset_hover(&self) {
        self.ivars().last_point.set(None);
        self.ivars().last_target.set(None);
    }

    fn queue(&self, pending: MacNativeDropPending) {
        let mut slot = self.ivars().pending.borrow_mut();
        // AppKit normally concludes a successful drop without another exit callback. Preserve
        // the submitted value if a destination nevertheless emits a late exit before the event
        // loop consumes this coalesced slot.
        if !matches!(slot.as_ref(), Some(MacNativeDropPending::Drop { .. }))
            || matches!(&pending, MacNativeDropPending::Drop { .. })
        {
            *slot = Some(pending);
        }
        drop(slot);
        if self.ivars().event_queued.replace(true) {
            return;
        }
        if self
            .ivars()
            .proxy
            .send_event(RuntimeEvent::NativeDropChanged(self.ivars().window))
            .is_err()
        {
            self.ivars().event_queued.set(false);
            self.ivars().pending.borrow_mut().take();
        }
    }
}

fn native_drop_state(delegate: &AnyObject) -> &QuickGuiNativeDropState {
    let state = unsafe {
        objc_getAssociatedObject(
            delegate as *const AnyObject as *const _,
            native_drop_associated_object_key(),
        )
    };
    // The dynamic subclass is installed only after this state and is restored before it clears.
    unsafe { (state as *const QuickGuiNativeDropState).as_ref() }
        .expect("QuickGUI native-drop subclass is missing its associated state")
}

fn pasteboard_has_type(pasteboard: &NSPasteboard, expected: &NSPasteboardType) -> bool {
    unsafe { pasteboard.types() }.is_some_and(|types| {
        (0..types.len()).any(|index| types.get(index).is_some_and(|value| value == expected))
    })
}

/// Convert at most `maximum + 4` UTF-8 bytes, enough to preserve one final scalar boundary.
pub(super) fn bounded_pasteboard_string(
    value: &NSString,
    maximum: usize,
) -> Option<(String, bool)> {
    let capacity = maximum.checked_add(4)?;
    let mut bytes = vec![0_u8; capacity];
    let mut used = 0_usize;
    let mut remaining = NSRange::default();
    let converted = unsafe {
        value.getBytes_maxLength_usedLength_encoding_options_range_remainingRange(
            bytes.as_mut_ptr().cast(),
            bytes.len(),
            &mut used,
            NSUTF8StringEncoding,
            NSStringEncodingConversionOptions(0),
            NSRange::new(0, value.length()),
            &mut remaining,
        )
    };
    if !converted && used == 0 && value.length() != 0 {
        return None;
    }
    if used > bytes.len() {
        return None;
    }
    bytes.truncate(used);
    let mut value = String::from_utf8(bytes).ok()?;
    let truncated = remaining.length != 0 || value.len() > maximum;
    if value.len() > maximum {
        let mut end = maximum;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
    }
    Some((value, truncated))
}

fn native_offer(
    state: &QuickGuiNativeDropState,
    pasteboard: &NSPasteboard,
) -> Option<MacNativeDropOffer> {
    let capabilities = state.ivars().capabilities.get();
    let mut payloads = Vec::with_capacity(3);
    let typed_type = state.ivars().typed_registry.pasteboard_type.as_ref();
    if capabilities & NATIVE_DROP_TYPED != 0
        && pasteboard_has_type(pasteboard, typed_type)
        && let Some(value) = unsafe { pasteboard.stringForType(typed_type) }
        && let Some((token, false)) = bounded_pasteboard_string(&value, 64)
        && let Some(payload) = state.ivars().typed_registry.resolve(&token)
    {
        payloads.push(MacNativeDropPayload::Typed(payload));
    }
    if capabilities & NATIVE_DROP_URL != 0
        && pasteboard_has_type(pasteboard, unsafe { NSPasteboardTypeURL })
        && let Some(value) = unsafe { pasteboard.stringForType(NSPasteboardTypeURL) }
        && let Some((value, false)) = bounded_pasteboard_string(&value, MAX_EXTERNAL_DRAG_URL_BYTES)
        && let Ok(value) = ExternalDragUrl::new(value)
    {
        payloads.push(MacNativeDropPayload::Url(Arc::new(value)));
    }
    if capabilities & NATIVE_DROP_TEXT != 0
        && pasteboard_has_type(pasteboard, unsafe { NSPasteboardTypeString })
        && let Some(value) = unsafe { pasteboard.stringForType(NSPasteboardTypeString) }
        && let Some((value, truncated)) =
            bounded_pasteboard_string(&value, MAX_EXTERNAL_DRAG_TEXT_BYTES)
    {
        payloads.push(MacNativeDropPayload::Text(Arc::new(
            ExternalDragText::from_bounded(value, truncated),
        )));
    }
    MacNativeDropOffer::new(payloads)
}

unsafe extern "C" fn native_drag_entered(
    this: &AnyObject,
    _cmd: Sel,
    sender: &ProtocolObject<dyn NSDraggingInfo>,
) -> NSDragOperation {
    let state = native_drop_state(this);
    state.reset_hover();
    let pasteboard = unsafe { sender.draggingPasteboard() };
    if pasteboard_has_type(&pasteboard, unsafe { NSFilenamesPboardType }) {
        *state.ivars().active.borrow_mut() = NativeDropMode::File;
        return unsafe {
            msg_send![super(this, state.ivars().previous_class), draggingEntered: sender]
        };
    }
    let Some(offer) = native_offer(state, &pasteboard) else {
        *state.ivars().active.borrow_mut() = NativeDropMode::None;
        return NSDragOperation::None;
    };
    let Some(point) = state.point(sender) else {
        *state.ivars().active.borrow_mut() = NativeDropMode::None;
        return NSDragOperation::None;
    };
    let accepted = state.hover(&offer, point, true);
    *state.ivars().active.borrow_mut() = NativeDropMode::Offer(offer);
    if accepted {
        NSDragOperation::Copy
    } else {
        NSDragOperation::None
    }
}

unsafe extern "C" fn native_drag_updated(
    this: &AnyObject,
    _cmd: Sel,
    sender: &ProtocolObject<dyn NSDraggingInfo>,
) -> NSDragOperation {
    let state = native_drop_state(this);
    let active = state.ivars().active.borrow();
    match &*active {
        NativeDropMode::File => NSDragOperation::Copy,
        NativeDropMode::Offer(offer) => {
            let Some(point) = state.point(sender) else {
                return NSDragOperation::None;
            };
            let accepted = state.hover(offer, point, false);
            if accepted {
                NSDragOperation::Copy
            } else {
                NSDragOperation::None
            }
        }
        NativeDropMode::None => NSDragOperation::None,
    }
}

unsafe extern "C" fn native_drag_exited(
    this: &AnyObject,
    _cmd: Sel,
    sender: Option<&ProtocolObject<dyn NSDraggingInfo>>,
) {
    let state = native_drop_state(this);
    let active = std::mem::replace(
        &mut *state.ivars().active.borrow_mut(),
        NativeDropMode::None,
    );
    state.reset_hover();
    match active {
        NativeDropMode::File => unsafe {
            let _: () = msg_send![
                super(this, state.ivars().previous_class),
                draggingExited: sender
            ];
        },
        NativeDropMode::Offer(_) => state.queue(MacNativeDropPending::Exit),
        NativeDropMode::None => {}
    }
}

unsafe extern "C" fn native_prepare_drag(
    this: &AnyObject,
    _cmd: Sel,
    sender: &ProtocolObject<dyn NSDraggingInfo>,
) -> Bool {
    let state = native_drop_state(this);
    match &*state.ivars().active.borrow() {
        NativeDropMode::File => unsafe {
            msg_send![
                super(this, state.ivars().previous_class),
                prepareForDragOperation: sender
            ]
        },
        NativeDropMode::Offer(offer) => Bool::new(
            state
                .point(sender)
                .is_some_and(|point| state.target(offer, point).is_some()),
        ),
        NativeDropMode::None => Bool::NO,
    }
}

unsafe extern "C" fn native_perform_drag(
    this: &AnyObject,
    _cmd: Sel,
    sender: &ProtocolObject<dyn NSDraggingInfo>,
) -> Bool {
    let state = native_drop_state(this);
    match &*state.ivars().active.borrow() {
        NativeDropMode::File => unsafe {
            msg_send![
                super(this, state.ivars().previous_class),
                performDragOperation: sender
            ]
        },
        NativeDropMode::Offer(offer) => {
            let Some(point) = state.point(sender) else {
                return Bool::NO;
            };
            if state.target(offer, point).is_none() {
                state.queue(MacNativeDropPending::Exit);
                return Bool::NO;
            }
            state.queue(MacNativeDropPending::Drop {
                offer: offer.clone(),
                point,
            });
            Bool::YES
        }
        NativeDropMode::None => Bool::NO,
    }
}

unsafe extern "C" fn native_conclude_drag(
    this: &AnyObject,
    _cmd: Sel,
    sender: Option<&ProtocolObject<dyn NSDraggingInfo>>,
) {
    let state = native_drop_state(this);
    let active = std::mem::replace(
        &mut *state.ivars().active.borrow_mut(),
        NativeDropMode::None,
    );
    state.reset_hover();
    if matches!(active, NativeDropMode::File) {
        unsafe {
            let _: () = msg_send![
                super(this, state.ivars().previous_class),
                concludeDragOperation: sender
            ];
        }
    }
}

unsafe extern "C" fn native_wants_periodic_drag_updates(_this: &AnyObject, _cmd: Sel) -> Bool {
    Bool::NO
}

/// Extends Winit's existing NSWindow drag destination while forwarding its Finder file path.
pub(crate) struct MacNativeDropHost {
    window: Retained<NSWindow>,
    delegate: Retained<AnyObject>,
    associated: Retained<QuickGuiNativeDropState>,
}

impl MacNativeDropHost {
    pub(crate) fn new(
        window: &Arc<Window>,
        handle: WindowHandle,
        proxy: EventLoopProxy<RuntimeEvent>,
        typed_registry: MacTypedDragRegistry,
    ) -> Result<Self, String> {
        MainThreadMarker::new().ok_or_else(|| {
            "native drop handling must be installed on the AppKit main thread".to_owned()
        })?;
        let raw = window
            .window_handle()
            .map_err(|error| format!("could not access the AppKit window handle: {error}"))?;
        let RawWindowHandle::AppKit(raw) = raw.as_raw() else {
            return Err("the active window does not expose an AppKit view".to_owned());
        };
        let content_view = unsafe { Retained::retain(raw.ns_view.as_ptr().cast::<NSView>()) }
            .ok_or_else(|| "the AppKit content view could not be retained".to_owned())?;
        let native_window = content_view
            .window()
            .ok_or_else(|| "the AppKit content view is not attached to a window".to_owned())?;
        let delegate = unsafe { native_window.delegate() }
            .ok_or_else(|| "the AppKit window has no Winit delegate".to_owned())?;
        let delegate: Retained<AnyObject> = unsafe { Retained::cast(delegate) };
        let existing = unsafe {
            objc_getAssociatedObject(
                Retained::as_ptr(&delegate) as *const _,
                native_drop_associated_object_key(),
            )
        };
        if !existing.is_null() {
            return Err("native drop handling is already installed on this window".to_owned());
        }

        let previous_class = unsafe { &*(delegate.class() as *const AnyClass) };
        let subclass = {
            let mut subclasses = NATIVE_DROP_SUBCLASSES
                .lock()
                .map_err(|_| "native-drop class registry is poisoned".to_owned())?;
            if let Some((_, subclass)) = subclasses
                .iter()
                .find(|(candidate, _)| *candidate == previous_class)
            {
                *subclass
            } else {
                let name = format!("QuickGuiNativeDropOf{}", previous_class.name());
                let mut builder = ClassBuilder::new(&name, previous_class).ok_or_else(|| {
                    format!("could not declare Objective-C native-drop subclass {name}")
                })?;
                unsafe {
                    builder.add_method(
                        sel!(draggingEntered:),
                        native_drag_entered as unsafe extern "C" fn(_, _, _) -> _,
                    );
                    builder.add_method(
                        sel!(draggingUpdated:),
                        native_drag_updated as unsafe extern "C" fn(_, _, _) -> _,
                    );
                    builder.add_method(
                        sel!(draggingExited:),
                        native_drag_exited as unsafe extern "C" fn(_, _, _),
                    );
                    builder.add_method(
                        sel!(prepareForDragOperation:),
                        native_prepare_drag as unsafe extern "C" fn(_, _, _) -> _,
                    );
                    builder.add_method(
                        sel!(performDragOperation:),
                        native_perform_drag as unsafe extern "C" fn(_, _, _) -> _,
                    );
                    builder.add_method(
                        sel!(concludeDragOperation:),
                        native_conclude_drag as unsafe extern "C" fn(_, _, _),
                    );
                    builder.add_method(
                        sel!(wantsPeriodicDraggingUpdates),
                        native_wants_periodic_drag_updates as unsafe extern "C" fn(_, _) -> _,
                    );
                }
                let subclass = builder.register();
                subclasses.push((previous_class, subclass));
                subclass
            }
        };
        let associated = QuickGuiNativeDropState::new(
            previous_class,
            content_view,
            handle,
            proxy,
            typed_registry,
        );
        unsafe {
            objc_setAssociatedObject(
                Retained::as_ptr(&delegate) as *mut _,
                native_drop_associated_object_key(),
                Retained::as_ptr(&associated) as *mut _,
                OBJC_ASSOCIATION_RETAIN_NONATOMIC,
            );
            object_setClass(
                Retained::as_ptr(&delegate) as *mut _,
                (subclass as *const AnyClass).cast(),
            );
        }
        let host = Self {
            window: native_window,
            delegate,
            associated,
        };
        host.register_types(0);
        Ok(host)
    }

    fn register_types(&self, capabilities: u8) {
        let mut types = vec![unsafe { NSFilenamesPboardType.copy() }];
        if capabilities & NATIVE_DROP_TEXT != 0 {
            types.push(unsafe { NSPasteboardTypeString.copy() });
        }
        if capabilities & NATIVE_DROP_URL != 0 {
            types.push(unsafe { NSPasteboardTypeURL.copy() });
        }
        if capabilities & NATIVE_DROP_TYPED != 0 {
            types.push(
                self.associated
                    .ivars()
                    .typed_registry
                    .pasteboard_type
                    .clone(),
            );
        }
        // AppKit does not expose the current registration set. Replace it synchronously on the main
        // thread and always restore Winit's legacy Finder type as the first entry.
        unsafe {
            self.window.unregisterDraggedTypes();
        }
        self.window
            .registerForDraggedTypes(&NSArray::from_vec(types));
    }

    pub(crate) fn update(
        &self,
        has_text: bool,
        has_url: bool,
        has_typed: bool,
        update_snapshot: impl FnOnce(&mut ExternalDropSnapshot),
    ) {
        let capabilities = (u8::from(has_text) * NATIVE_DROP_TEXT)
            | (u8::from(has_url) * NATIVE_DROP_URL)
            | (u8::from(has_typed) * NATIVE_DROP_TYPED);
        if self.associated.ivars().capabilities.replace(capabilities) != capabilities {
            self.register_types(capabilities);
        }
        let mut snapshot = self.associated.ivars().snapshot.borrow_mut();
        if capabilities == 0 {
            snapshot.clear();
        } else {
            update_snapshot(&mut snapshot);
        }
        drop(snapshot);

        let offer = match &*self.associated.ivars().active.borrow() {
            NativeDropMode::Offer(offer) => Some(offer.clone()),
            NativeDropMode::None | NativeDropMode::File => None,
        };
        if let (Some(offer), Some(point)) = (offer, self.associated.ivars().last_point.get()) {
            self.associated.hover(&offer, point, false);
        }
    }

    pub(crate) fn take_pending(&self) -> Option<MacNativeDropPending> {
        self.associated.ivars().event_queued.set(false);
        self.associated.ivars().pending.borrow_mut().take()
    }
}

impl Drop for MacNativeDropHost {
    fn drop(&mut self) {
        self.register_types(0);
        let previous_class = self.associated.ivars().previous_class;
        unsafe {
            object_setClass(
                Retained::as_ptr(&self.delegate) as *mut _,
                (previous_class as *const AnyClass).cast(),
            );
            objc_setAssociatedObject(
                Retained::as_ptr(&self.delegate) as *mut _,
                native_drop_associated_object_key(),
                null_mut(),
                OBJC_ASSOCIATION_RETAIN_NONATOMIC,
            );
        }
    }
}

struct TypedDragWriterIvars {
    pasteboard_type: Retained<NSString>,
    token: Retained<NSString>,
    fallback: Option<Retained<AnyObject>>,
}

declare_class!(
    struct QuickGuiTypedDragWriter;

    unsafe impl ClassType for QuickGuiTypedDragWriter {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "QuickGuiTypedDragWriter";
    }

    impl DeclaredClass for QuickGuiTypedDragWriter {
        type Ivars = TypedDragWriterIvars;
    }

    unsafe impl NSObjectProtocol for QuickGuiTypedDragWriter {}

    unsafe impl NSPasteboardWriting for QuickGuiTypedDragWriter {
        #[method_id(writableTypesForPasteboard:)]
        fn writable_types(&self, pasteboard: &NSPasteboard) -> Retained<NSArray<NSPasteboardType>> {
            let mut types = Vec::with_capacity(4);
            types.push(self.ivars().pasteboard_type.clone());
            if let Some(fallback) = &self.ivars().fallback {
                let fallback_types: Retained<NSArray<NSPasteboardType>> = unsafe {
                    msg_send_id![fallback.as_ref(), writableTypesForPasteboard: pasteboard]
                };
                for index in 0..fallback_types.len().min(MAX_COMPOSITE_PASTEBOARD_TYPES) {
                    if let Some(value) = fallback_types.get_retained(index)
                        && value.as_ref() != self.ivars().pasteboard_type.as_ref()
                    {
                        types.push(value);
                    }
                }
            }
            NSArray::from_vec(types)
        }

        #[method_id(pasteboardPropertyListForType:)]
        fn property_list(&self, value_type: &NSPasteboardType) -> Option<Retained<AnyObject>> {
            if value_type == self.ivars().pasteboard_type.as_ref() {
                Some(unsafe { Retained::cast(self.ivars().token.clone()) })
            } else {
                self.ivars().fallback.as_ref().and_then(|fallback| unsafe {
                    msg_send_id![fallback.as_ref(), pasteboardPropertyListForType: value_type]
                })
            }
        }
    }
);

impl QuickGuiTypedDragWriter {
    fn new(
        mtm: MainThreadMarker,
        pasteboard_type: Retained<NSString>,
        token: Retained<NSString>,
        fallback: Option<&ProtocolObject<dyn NSPasteboardWriting>>,
    ) -> Retained<Self> {
        let fallback = fallback.and_then(|fallback| unsafe {
            Retained::retain(
                (fallback as *const ProtocolObject<dyn NSPasteboardWriting>)
                    .cast::<AnyObject>()
                    .cast_mut(),
            )
        });
        let allocated = mtm.alloc().set_ivars(TypedDragWriterIvars {
            pasteboard_type,
            token,
            fallback,
        });
        unsafe { msg_send_id![super(allocated), init] }
    }
}

struct ExternalDragSourceIvars {
    proxy: EventLoopProxy<RuntimeEvent>,
    window: WindowHandle,
}

declare_class!(
    struct QuickGuiExternalDragSource;

    unsafe impl ClassType for QuickGuiExternalDragSource {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "QuickGuiExternalDragSource";
    }

    impl DeclaredClass for QuickGuiExternalDragSource {
        type Ivars = ExternalDragSourceIvars;
    }

    unsafe impl NSObjectProtocol for QuickGuiExternalDragSource {}

    unsafe impl NSDraggingSource for QuickGuiExternalDragSource {
        #[method(draggingSession:sourceOperationMaskForDraggingContext:)]
        fn source_operation_mask(
            &self,
            _session: &NSDraggingSession,
            _context: NSDraggingContext,
        ) -> NSDragOperation {
            // Payloads are offered as copies. Advertising move without a source-side commit
            // callback could let a destination mutate application-owned data unexpectedly.
            NSDragOperation::Copy
        }

        #[method(draggingSession:endedAtPoint:operation:)]
        fn dragging_session_ended(
            &self,
            _session: &NSDraggingSession,
            _screen_point: NSPoint,
            operation: NSDragOperation,
        ) {
            let operation = external_drag_operation(operation);
            let _ = self
                .ivars()
                .proxy
                .send_event(RuntimeEvent::ExternalDragEnded(
                    self.ivars().window,
                    operation,
                ));
        }
    }
);

impl QuickGuiExternalDragSource {
    fn new(
        mtm: MainThreadMarker,
        window: WindowHandle,
        proxy: EventLoopProxy<RuntimeEvent>,
    ) -> Retained<Self> {
        let allocated = mtm
            .alloc()
            .set_ivars(ExternalDragSourceIvars { proxy, window });
        unsafe { msg_send_id![super(allocated), init] }
    }
}

pub(super) fn external_drag_operation(operation: NSDragOperation) -> ExternalDragOperation {
    if operation.contains(NSDragOperation::Delete) {
        ExternalDragOperation::Deleted
    } else if operation.contains(NSDragOperation::Move) {
        ExternalDragOperation::Moved
    } else if operation.contains(NSDragOperation::Link) {
        ExternalDragOperation::Linked
    } else if operation.contains(NSDragOperation::Copy) {
        ExternalDragOperation::Copied
    } else if operation == NSDragOperation::None {
        ExternalDragOperation::Cancelled
    } else {
        ExternalDragOperation::Other
    }
}

/// The exact AppKit left-mouse-down event required to promote a later internal drag.
#[derive(Clone)]
pub(crate) struct MacMouseDownEvent(Retained<NSEvent>);

/// Retains the AppKit source and session until the native destination finishes the drag.
pub(crate) struct MacExternalDragSession {
    _source: Retained<QuickGuiExternalDragSource>,
    _session: Retained<NSDraggingSession>,
    _typed_registration: MacTypedDragRegistration,
}

/// Lazily observes AppKit drag motion after Winit's cursor leaves the content view.
///
/// Winit can stop delivering `CursorMoved` before an `NSView` tracking area emits `CursorLeft`
/// during a captured primary-button gesture. The local monitor sends at most one boundary event
/// per armed gesture and returns the original event unchanged.
pub(crate) struct MacExternalDragMonitor {
    event_monitor: Option<Retained<AnyObject>>,
    enabled: Arc<AtomicBool>,
    boundary_sent: Arc<AtomicBool>,
}

impl MacExternalDragMonitor {
    pub(crate) fn new(
        window: &Arc<Window>,
        window_handle: WindowHandle,
        proxy: EventLoopProxy<RuntimeEvent>,
    ) -> Result<Self, String> {
        MainThreadMarker::new().ok_or_else(|| {
            "external drag monitoring must be installed on the AppKit main thread".to_owned()
        })?;
        let handle = window
            .window_handle()
            .map_err(|error| format!("could not access the AppKit window handle: {error}"))?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Err("the active window does not expose an AppKit view".to_owned());
        };
        let view = unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
            .ok_or_else(|| "the AppKit content view could not be retained".to_owned())?;
        let native_window = view
            .window()
            .ok_or_else(|| "the AppKit content view is not attached to a window".to_owned())?;
        let enabled = Arc::new(AtomicBool::new(false));
        let boundary_sent = Arc::new(AtomicBool::new(false));
        let monitor_enabled = Arc::clone(&enabled);
        let monitor_boundary_sent = Arc::clone(&boundary_sent);
        let monitor_block = RcBlock::new(move |event: NonNull<NSEvent>| {
            if monitor_enabled.load(Ordering::Relaxed)
                && !monitor_boundary_sent.load(Ordering::Relaxed)
            {
                let belongs_to_window = MainThreadMarker::new().is_some_and(|mtm| unsafe {
                    event.as_ref().window(mtm).is_some_and(|event_window| {
                        Retained::as_ptr(&event_window) == Retained::as_ptr(&native_window)
                    })
                });
                if belongs_to_window {
                    let window_point = unsafe { event.as_ref().locationInWindow() };
                    let point = view.convertPoint_fromView(window_point, None);
                    let bounds = view.bounds();
                    if point_outside_ns_rect(point, bounds)
                        && !monitor_boundary_sent.swap(true, Ordering::Relaxed)
                    {
                        let _ = proxy.send_event(RuntimeEvent::ExternalDragBoundary(
                            window_handle,
                            Point::new(point.x as f32, point.y as f32),
                        ));
                    }
                }
            }
            event.as_ptr()
        });
        let event_monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                NSEventMask::LeftMouseDragged,
                &monitor_block,
            )
        }
        .ok_or_else(|| "could not monitor AppKit drag motion".to_owned())?;
        Ok(Self {
            event_monitor: Some(event_monitor),
            enabled,
            boundary_sent,
        })
    }

    pub(crate) fn arm(&self) {
        self.boundary_sent.store(false, Ordering::Relaxed);
        self.enabled.store(true, Ordering::Relaxed);
    }

    pub(crate) fn disarm(&self) {
        self.enabled.store(false, Ordering::Relaxed);
        self.boundary_sent.store(false, Ordering::Relaxed);
    }
}

pub(super) fn point_outside_ns_rect(point: NSPoint, bounds: NSRect) -> bool {
    point.x < bounds.origin.x
        || point.y < bounds.origin.y
        || point.x > bounds.origin.x + bounds.size.width
        || point.y > bounds.origin.y + bounds.size.height
}

impl Drop for MacExternalDragMonitor {
    fn drop(&mut self) {
        if let Some(event_monitor) = self.event_monitor.take() {
            unsafe {
                NSEvent::removeMonitor(&event_monitor);
            }
        }
    }
}
/// Capture the native event synchronously while Winit delivers its matching button press.
pub(crate) fn capture_left_mouse_down() -> Option<MacMouseDownEvent> {
    let mtm = MainThreadMarker::new()?;
    let event = NSApplication::sharedApplication(mtm).currentEvent()?;
    (unsafe { event.r#type() } == NSEventType::LeftMouseDown).then_some(MacMouseDownEvent(event))
}

/// Promote one process-local typed value plus an optional public payload to AppKit.
pub(crate) fn start_external_drag(
    window: &Arc<Window>,
    mouse_down: &MacMouseDownEvent,
    payload: Option<&ExternalDragPayload>,
    typed_payload: MacTypedDragPayload,
    typed_registry: &MacTypedDragRegistry,
    window_handle: WindowHandle,
    proxy: EventLoopProxy<RuntimeEvent>,
) -> Result<MacExternalDragSession, String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "external dragging must start on the AppKit main thread".to_owned())?;
    let (token, typed_registration) = typed_registry.register(typed_payload)?;
    let token = NSString::from_str(&token);
    let pasteboard_type = typed_registry.pasteboard_type.clone();

    let handle = window
        .window_handle()
        .map_err(|error| format!("could not access the AppKit window handle: {error}"))?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("the active window does not expose an AppKit view".to_owned());
    };
    let view = unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
        .ok_or_else(|| "the AppKit content view could not be retained".to_owned())?;
    let window_point = unsafe { mouse_down.0.locationInWindow() };
    let location = view.convertPoint_fromView(window_point, None);
    let frame = NSRect::new(
        NSPoint::new(location.x - 16.0, location.y - 16.0),
        NSSize::new(32.0, 32.0),
    );
    let (items, formation) = match payload {
        Some(ExternalDragPayload::Files(paths)) => {
            if paths.is_empty() {
                return Err("the external drag contains no retained paths".to_owned());
            }
            let file_icon = external_drag_icon("doc.fill");
            let directory_icon = external_drag_icon("folder.fill");
            let mut items = Vec::with_capacity(paths.entries().len());
            for (path, is_directory) in paths.entries() {
                let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
                    tracing::warn!("external drag skipped a path containing an interior nul byte");
                    continue;
                };
                let path = NonNull::new(path.as_ptr().cast_mut())
                    .expect("a CString always exposes a non-null pointer");
                let url = unsafe {
                    NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(
                        path,
                        *is_directory,
                        None,
                    )
                };
                let writer: &ProtocolObject<dyn NSPasteboardWriting> =
                    ProtocolObject::from_ref(url.as_ref());
                let icon = if *is_directory {
                    directory_icon.as_deref()
                } else {
                    file_icon.as_deref()
                };
                items.push(typed_dragging_item(
                    mtm,
                    pasteboard_type.clone(),
                    token.clone(),
                    Some(writer),
                    frame,
                    icon,
                ));
            }
            if items.is_empty() {
                return Err("none of the retained paths could be represented by AppKit".to_owned());
            }
            (items, NSDraggingFormation::Stack)
        }
        Some(ExternalDragPayload::Text(text)) => {
            if text.is_empty() {
                return Err("the external text drag is empty".to_owned());
            }
            let text = NSString::from_str(text.as_str());
            let writer: &ProtocolObject<dyn NSPasteboardWriting> =
                ProtocolObject::from_ref(text.as_ref());
            let icon = external_drag_icon("text.alignleft");
            (
                vec![typed_dragging_item(
                    mtm,
                    pasteboard_type.clone(),
                    token.clone(),
                    Some(writer),
                    frame,
                    icon.as_deref(),
                )],
                NSDraggingFormation::None,
            )
        }
        Some(ExternalDragPayload::Url(url)) => {
            let url_string = NSString::from_str(url.as_str());
            let url = unsafe { NSURL::URLWithString(&url_string) }
                .ok_or_else(|| "AppKit rejected the external drag URL".to_owned())?;
            let writer: &ProtocolObject<dyn NSPasteboardWriting> =
                ProtocolObject::from_ref(url.as_ref());
            let icon = external_drag_icon("link");
            (
                vec![typed_dragging_item(
                    mtm,
                    pasteboard_type.clone(),
                    token.clone(),
                    Some(writer),
                    frame,
                    icon.as_deref(),
                )],
                NSDraggingFormation::None,
            )
        }
        None => {
            let icon = external_drag_icon("square.dashed");
            (
                vec![typed_dragging_item(
                    mtm,
                    pasteboard_type,
                    token,
                    None,
                    frame,
                    icon.as_deref(),
                )],
                NSDraggingFormation::None,
            )
        }
    };

    let items = NSArray::from_vec(items);
    let source = QuickGuiExternalDragSource::new(mtm, window_handle, proxy);
    let source_protocol: &ProtocolObject<dyn NSDraggingSource> =
        ProtocolObject::from_ref(source.as_ref());
    let session = unsafe {
        view.beginDraggingSessionWithItems_event_source(
            &items,
            mouse_down.0.as_ref(),
            source_protocol,
        )
    };
    unsafe {
        session.setDraggingFormation(formation);
        session.setAnimatesToStartingPositionsOnCancelOrFail(true);
    }
    Ok(MacExternalDragSession {
        _source: source,
        _session: session,
        _typed_registration: typed_registration,
    })
}

fn typed_dragging_item(
    mtm: MainThreadMarker,
    pasteboard_type: Retained<NSString>,
    token: Retained<NSString>,
    fallback: Option<&ProtocolObject<dyn NSPasteboardWriting>>,
    frame: NSRect,
    icon: Option<&NSImage>,
) -> Retained<NSDraggingItem> {
    let writer = QuickGuiTypedDragWriter::new(mtm, pasteboard_type, token, fallback);
    let writer: &ProtocolObject<dyn NSPasteboardWriting> =
        ProtocolObject::from_ref(writer.as_ref());
    external_dragging_item(writer, frame, icon)
}

pub(super) fn external_dragging_item(
    writer: &ProtocolObject<dyn NSPasteboardWriting>,
    frame: NSRect,
    icon: Option<&NSImage>,
) -> Retained<NSDraggingItem> {
    let item = unsafe { NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), writer) };
    unsafe {
        item.setDraggingFrame_contents(frame, icon.map(|icon| icon.as_ref()));
    }
    item
}

fn external_drag_icon(symbol: &str) -> Option<Retained<NSImage>> {
    let symbol = NSString::from_str(symbol);
    let image =
        unsafe { NSImage::imageWithSystemSymbolName_accessibilityDescription(&symbol, None) }?;
    unsafe {
        image.setSize(NSSize::new(32.0, 32.0));
    }
    Some(image)
}
