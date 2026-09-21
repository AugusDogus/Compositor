use super::*;

/// Configure AppKit to ask the layer-backed Winit view for fresh content throughout live resize.
///
/// The default policy can preserve and stretch an old layer snapshot. GPU windows instead need a
/// draw callback inside AppKit's resize transaction so their layout and native children stay in
/// lockstep with the window frame.
pub(crate) fn configure_gpu_window_resize(window: &Arc<Window>) -> Result<(), String> {
    MainThreadMarker::new().ok_or_else(|| {
        "GPU window resize must be configured on the AppKit main thread".to_owned()
    })?;
    let handle = window
        .window_handle()
        .map_err(|error| format!("could not access the AppKit window handle: {error}"))?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("the active window does not expose an AppKit view".to_owned());
    };
    let view = unsafe { handle.ns_view.as_ptr().cast::<NSView>().as_ref() }
        .ok_or_else(|| "the AppKit content view pointer is null".to_owned())?;
    unsafe {
        view.setLayerContentsRedrawPolicy(
            NSViewLayerContentsRedrawPolicy::NSViewLayerContentsRedrawDuringViewResize,
        );
    }
    Ok(())
}

/// Enable or disable AppKit's implicit titlebar/background window dragging.
///
/// Hidden-inset windows disable this and opt into exact declarative drag regions instead.
pub(crate) fn set_window_movable(window: &Arc<Window>, movable: bool) -> Result<(), String> {
    MainThreadMarker::new().ok_or_else(|| {
        "window movability must be configured on the AppKit main thread".to_owned()
    })?;
    let handle = window
        .window_handle()
        .map_err(|error| format!("could not access the AppKit window handle: {error}"))?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("the active window does not expose an AppKit view".to_owned());
    };
    let view = unsafe { handle.ns_view.as_ptr().cast::<NSView>().as_ref() }
        .ok_or_else(|| "the AppKit content view pointer is null".to_owned())?;
    let window = view
        .window()
        .ok_or_else(|| "the AppKit content view is not attached to a window".to_owned())?;
    if unsafe { window.isMovable() } != movable {
        window.setMovable(movable);
    }
    if unsafe { window.isMovableByWindowBackground() } {
        window.setMovableByWindowBackground(false);
    }
    Ok(())
}

pub(crate) fn appkit_view(window: &Arc<Window>) -> Result<Retained<NSView>, String> {
    MainThreadMarker::new().ok_or_else(|| {
        "native window state must be changed on the AppKit main thread".to_owned()
    })?;
    let handle = window
        .window_handle()
        .map_err(|error| format!("could not access the AppKit window handle: {error}"))?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("the active window does not expose an AppKit view".to_owned());
    };
    unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
        .ok_or_else(|| "the AppKit content view pointer is null".to_owned())
}

pub(super) fn appkit_window(window: &Arc<Window>) -> Result<Retained<NSWindow>, String> {
    appkit_view(window)?
        .window()
        .ok_or_else(|| "the AppKit content view is not attached to a window".to_owned())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MacWindowTabAction {
    SelectNext,
    SelectPrevious,
    Select(usize),
    MergeAll,
    MoveToNewWindow,
    ToggleBar,
    ToggleOverview,
}

/// Configure document-window state before the first hidden surface frame is presented.
pub(crate) fn configure_document_window(
    window: &Arc<Window>,
    represented_file: Option<&Path>,
    document_edited: bool,
    tabbing_identifier: Option<&str>,
) -> Result<(), String> {
    let native = appkit_window(window)?;
    if represented_file.is_some() {
        set_native_represented_file(&native, represented_file)?;
    }
    if document_edited {
        native.setDocumentEdited(true);
    }
    set_native_tabbing_identifier(&native, tabbing_identifier);
    Ok(())
}

pub(crate) fn set_window_represented_file(
    window: &Arc<Window>,
    represented_file: Option<&Path>,
) -> Result<(), String> {
    let native = appkit_window(window)?;
    set_native_represented_file(&native, represented_file)
}

fn set_native_represented_file(
    window: &NSWindow,
    represented_file: Option<&Path>,
) -> Result<(), String> {
    let url = represented_file
        .map(|path| native_file_url(path, false))
        .transpose()?;
    unsafe {
        window.setRepresentedURL(url.as_deref());
    }
    Ok(())
}

pub(crate) fn set_window_document_edited(window: &Arc<Window>, edited: bool) -> Result<(), String> {
    appkit_window(window)?.setDocumentEdited(edited);
    Ok(())
}

pub(crate) fn show_character_palette(window: &Arc<Window>) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or_else(|| {
        "the character palette must be presented on the AppKit main thread".to_owned()
    })?;
    let native = appkit_window(window)?;
    NSApplication::sharedApplication(mtm).orderFrontCharacterPalette(Some(native.as_ref()));
    Ok(())
}

pub(crate) fn set_window_tabbing_identifier(
    window: &Arc<Window>,
    identifier: Option<&str>,
) -> Result<(), String> {
    let native = appkit_window(window)?;
    set_native_tabbing_identifier(&native, identifier);
    Ok(())
}

fn set_native_tabbing_identifier(window: &NSWindow, identifier: Option<&str>) {
    match identifier {
        Some(identifier) => {
            window.setTabbingMode(NSWindowTabbingMode::Preferred);
            window.setTabbingIdentifier(&NSString::from_str(identifier));
        }
        None => {
            window.setTabbingMode(NSWindowTabbingMode::Disallowed);
            window.setTabbingIdentifier(&NSString::from_str(""));
        }
    }
}

pub(crate) fn perform_window_tab_action(
    window: &Arc<Window>,
    action: MacWindowTabAction,
) -> Result<(), String> {
    let native = appkit_window(window)?;
    match action {
        MacWindowTabAction::SelectNext => native.selectNextTab(None),
        MacWindowTabAction::SelectPrevious => unsafe { native.selectPreviousTab(None) },
        MacWindowTabAction::Select(index) => {
            if let Some(group) = native.tabGroup() {
                let windows = group.windows();
                if index < windows.count() {
                    let selected = unsafe { windows.objectAtIndex(index) };
                    group.setSelectedWindow(Some(&selected));
                }
            }
        }
        MacWindowTabAction::MergeAll => unsafe { native.mergeAllWindows(None) },
        MacWindowTabAction::MoveToNewWindow => unsafe { native.moveTabToNewWindow(None) },
        MacWindowTabAction::ToggleBar => unsafe { native.toggleTabBar(None) },
        MacWindowTabAction::ToggleOverview => unsafe { native.toggleTabOverview(None) },
    }
    Ok(())
}

pub(crate) fn window_tab_state(window: &Arc<Window>) -> Result<WindowTabState, String> {
    let native = appkit_window(window)?;
    let Some(group) = native.tabGroup() else {
        return Ok(WindowTabState::default());
    };
    bounded_window_tab_state(&group)
}

fn bounded_window_tab_state(group: &NSWindowTabGroup) -> Result<WindowTabState, String> {
    let windows = group.windows();
    let total = windows.count();
    if total == 0 {
        return Ok(WindowTabState::default());
    }
    let count = total.min(MAX_SYSTEM_WINDOW_TABS);
    let selected = unsafe { group.selectedWindow() };
    let selected_index = selected.as_ref().and_then(|selected| {
        (0..count).find(|index| {
            let candidate = unsafe { windows.objectAtIndex(*index) };
            Retained::as_ptr(&candidate) == Retained::as_ptr(selected)
        })
    });
    Ok(WindowTabState {
        count,
        selected_index,
        tab_bar_visible: unsafe { group.isTabBarVisible() },
        overview_visible: unsafe { group.isOverviewVisible() },
        truncated: total > count,
    })
}

pub(super) fn deepest_appkit_sheet(window: &Arc<Window>) -> Result<Retained<NSWindow>, String> {
    let mut parent = appkit_window(window)?;
    while let Some(sheet) = unsafe { parent.attachedSheet() } {
        parent = sheet;
    }
    Ok(parent)
}

/// One bounded native sheet retained until AppKit invokes its completion handler.
pub(crate) enum MacPlatformDialog {
    Prompt(Retained<NSAlert>),
    Open(Retained<NSOpenPanel>),
    Save(Retained<NSSavePanel>),
}

#[derive(Clone)]
pub(crate) struct MacPlatformDialogContext {
    owner: Option<WindowHandle>,
    id: PlatformDialogId,
    open: Arc<AtomicBool>,
    proxy: EventLoopProxy<RuntimeEvent>,
}

#[derive(Clone)]
pub(crate) struct MacPlatformDialogFocus {
    window: Retained<NSWindow>,
    responder: Retained<NSResponder>,
}

impl MacPlatformDialogFocus {
    pub(crate) fn capture(window: Option<&Arc<Window>>) -> Result<Option<Self>, String> {
        let Some(window) = window else {
            return Ok(None);
        };
        let window = deepest_appkit_sheet(window)?;
        Ok(window
            .firstResponder()
            .map(|responder| Self { window, responder }))
    }

    pub(crate) fn restore(&self) -> bool {
        if !self.window.isKeyWindow() {
            self.window.makeKeyAndOrderFront(None);
        }
        self.window.makeFirstResponder(Some(&self.responder))
    }
}

impl MacPlatformDialogContext {
    pub(crate) fn new(
        owner: Option<WindowHandle>,
        id: PlatformDialogId,
        open: Arc<AtomicBool>,
        proxy: EventLoopProxy<RuntimeEvent>,
    ) -> Self {
        Self {
            owner,
            id,
            open,
            proxy,
        }
    }
}

impl MacPlatformDialog {
    /// End a still-active native sheet before its owning QuickGUI window is released.
    pub(crate) fn cancel(&self) {
        match self {
            Self::Prompt(alert) => unsafe {
                let sheet = alert.window();
                if let Some(parent) = sheet.sheetParent() {
                    parent.endSheet_returnCode(&sheet, NSModalResponseCancel);
                } else {
                    sheet.close();
                }
            },
            Self::Open(panel) => unsafe { panel.cancel(None) },
            Self::Save(panel) => unsafe { panel.cancel(None) },
        }
    }
}

pub(crate) fn native_file_url(path: &Path, is_directory: bool) -> Result<Retained<NSURL>, String> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "a native path cannot contain NUL".to_owned())?;
    let path = NonNull::new(path.as_ptr().cast_mut())
        .expect("CString always exposes a non-null filesystem representation");
    Ok(unsafe {
        NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(
            path,
            is_directory,
            None,
        )
    })
}

pub(super) fn native_file_path(url: &NSURL) -> Result<PathBuf, PlatformError> {
    if !unsafe { url.isFileURL() } {
        return Err(PlatformError::Platform(
            "the native panel returned a non-file URL".into(),
        ));
    }
    let path = unsafe { CStr::from_ptr(url.fileSystemRepresentation().as_ptr()) }.to_bytes();
    if path.is_empty() || path.len() > MAX_PLATFORM_PATH_BYTES {
        return Err(PlatformError::SelectionTooLarge);
    }
    Ok(PathBuf::from(std::ffi::OsString::from_vec(path.to_vec())))
}

pub(super) fn finish_native_dialog<T>(
    context: &MacPlatformDialogContext,
    responder: &PlatformResponder<T>,
    result: Result<T, PlatformError>,
) {
    context.open.store(false, Ordering::Release);
    let _ = context.proxy.send_event(RuntimeEvent::PlatformDialogClosed(
        context.owner,
        context.id,
    ));
    responder.complete(result);
}

pub(crate) fn present_native_prompt(
    window: Option<&Arc<Window>>,
    context: MacPlatformDialogContext,
    level: PromptLevel,
    message: &str,
    detail: Option<&str>,
    buttons: &[PromptButton],
    responder: PlatformResponder<usize>,
) -> Result<MacPlatformDialog, String> {
    let parent = window.map(deepest_appkit_sheet).transpose()?;
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "native prompts must start on the AppKit main thread".to_owned())?;
    let alert = unsafe { NSAlert::new(mtm) };
    unsafe {
        alert.setAlertStyle(match level {
            PromptLevel::Info => NSAlertStyle::Informational,
            PromptLevel::Warning => NSAlertStyle::Warning,
            PromptLevel::Critical => NSAlertStyle::Critical,
        });
        alert.setMessageText(&NSString::from_str(message));
        if let Some(detail) = detail {
            alert.setInformativeText(&NSString::from_str(detail));
        }
    }

    let initial_focus_index = buttons
        .iter()
        .enumerate()
        .rev()
        .find(|(_, button)| !button.is_cancel())
        .map(|(index, _)| index)
        .filter(|index| *index > 0);
    let mut initial_focus = None;
    for (index, button) in buttons.iter().enumerate() {
        let native = unsafe { alert.addButtonWithTitle(&NSString::from_str(button.label())) };
        if button.is_cancel() {
            unsafe { native.setKeyEquivalent(&NSString::from_str("\u{1b}")) };
        } else if Some(index) == initial_focus_index {
            initial_focus = Some(native);
        }
    }
    if let Some(button) = initial_focus {
        unsafe { alert.window() }.setInitialFirstResponder(Some(&button));
    }

    let button_count = buttons.len();
    let finish_response = move |response: NSModalResponse| {
        let result = response
            .checked_sub(NSAlertFirstButtonReturn)
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < button_count)
            .ok_or_else(|| {
                PlatformError::Platform("the native prompt closed without an answer".into())
            });
        finish_native_dialog(&context, &responder, result);
    };
    if let Some(parent) = parent {
        let completion = RcBlock::new(finish_response);
        unsafe {
            alert.beginSheetModalForWindow_completionHandler(&parent, Some(&completion));
        }
    } else {
        // NSAlert has no asynchronous application-modal API. Its native modal session still
        // dispatches AppKit events, and this branch is used only when the caller omits a parent.
        let response = unsafe { alert.runModal() };
        finish_response(response);
    }
    Ok(MacPlatformDialog::Prompt(alert))
}

pub(crate) fn shell_open_url(url: &str) -> Result<(), String> {
    MainThreadMarker::new()
        .ok_or_else(|| "URL shell actions must run on the AppKit main thread".to_owned())?;
    let url = unsafe { NSURL::initWithString(NSURL::alloc(), &NSString::from_str(url)) }
        .ok_or_else(|| "the URL is not valid for NSWorkspace".to_owned())?;
    if unsafe { NSWorkspace::sharedWorkspace().openURL(&url) } {
        Ok(())
    } else {
        Err("NSWorkspace rejected the URL".to_owned())
    }
}

pub(crate) fn shell_open_path(path: &Path) -> Result<(), String> {
    MainThreadMarker::new()
        .ok_or_else(|| "path shell actions must run on the AppKit main thread".to_owned())?;
    let url = native_file_url(path, false)?;
    if unsafe { NSWorkspace::sharedWorkspace().openURL(&url) } {
        Ok(())
    } else {
        Err("NSWorkspace rejected the path".to_owned())
    }
}

pub(crate) fn shell_reveal_path(path: &Path) -> Result<(), String> {
    MainThreadMarker::new()
        .ok_or_else(|| "path shell actions must run on the AppKit main thread".to_owned())?;
    let url = native_file_url(path, false)?;
    let urls = NSArray::from_id_slice(&[url]);
    unsafe {
        NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
    }
    Ok(())
}

pub(crate) fn shell_trash_path(path: &Path) -> Result<(), String> {
    MainThreadMarker::new()
        .ok_or_else(|| "trash actions must run on the AppKit main thread".to_owned())?;
    let url = native_file_url(path, false)?;
    unsafe {
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|error| error.localizedDescription().to_string())
    }
}

/// Route the default Cmd-W fallback through AppKit so Winit emits `CloseRequested` normally.
pub(crate) fn perform_window_close(window: &Arc<Window>) -> Result<(), String> {
    unsafe { appkit_window(window)?.performClose(None) };
    Ok(())
}

fn top_left_screen_rect(rect: NSRect, main_screen_height: f32) -> Rect {
    Rect::new(
        rect.origin.x as f32,
        main_screen_height - (rect.origin.y + rect.size.height) as f32,
        rect.size.width as f32,
        rect.size.height as f32,
    )
}

fn appkit_main_screen_height(
    mtm: MainThreadMarker,
) -> Result<(Retained<NSArray<NSScreen>>, f32), String> {
    let screens = NSScreen::screens(mtm);
    let primary = screens
        .iter()
        .find(|screen| {
            let origin = screen.frame().origin;
            origin.x == 0.0 && origin.y == 0.0
        })
        .or_else(|| screens.iter().next())
        .ok_or_else(|| "AppKit reported no screens".to_owned())?;
    Ok((screens.clone(), primary.frame().size.height as f32))
}

/// Read the hardware pointer in QuickGUI's top-left global logical desktop coordinates.
pub(crate) fn current_cursor_screen_position() -> Option<Point> {
    let mtm = MainThreadMarker::new()?;
    let (_, main_screen_height) = appkit_main_screen_height(mtm).ok()?;
    // SAFETY: AppKit's process-wide mouse location is a value-only main-thread query.
    let point = unsafe { NSEvent::mouseLocation() };
    let point = Point::new(point.x as f32, main_screen_height - point.y as f32);
    (point.x.is_finite() && point.y.is_finite()).then_some(point)
}

/// Resolve and apply parent-relative popover geometry while both native windows remain hidden.
pub(crate) fn position_system_popover(
    window: &Arc<Window>,
    parent: &Arc<Window>,
    options: &PopoverOptions,
) -> Result<Rect, String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "popover placement must be resolved on the AppKit main thread".to_owned())?;
    let child = appkit_window(window)?;
    let parent = appkit_window(parent)?;
    let (screens, main_screen_height) = appkit_main_screen_height(mtm)?;

    let parent_content = parent.contentRectForFrameRect(parent.frame());
    let parent_content = top_left_screen_rect(parent_content, main_screen_height);
    let anchor_rect = Rect::new(
        parent_content.x + options.anchor_rect.x,
        parent_content.y + options.anchor_rect.y,
        options.anchor_rect.width,
        options.anchor_rect.height,
    );
    let anchor_center = Point::new(
        anchor_rect.x + anchor_rect.width * 0.5,
        anchor_rect.y + anchor_rect.height * 0.5,
    );
    let screen = screens
        .iter()
        .find(|screen| {
            top_left_screen_rect(screen.frame(), main_screen_height).contains(anchor_center)
        })
        .map(|screen| screen.retain())
        .or_else(|| parent.screen())
        .or_else(|| NSScreen::mainScreen(mtm))
        .ok_or_else(|| "AppKit could not resolve the popover's target screen".to_owned())?;
    let visible = top_left_screen_rect(screen.visibleFrame(), main_screen_height);
    if visible.is_empty() {
        return Err("AppKit reported an empty popover work area".to_owned());
    }

    let child_content = child.contentRectForFrameRect(child.frame());
    let size = Size::new(
        child_content.size.width as f32,
        child_content.size.height as f32,
    );
    let resolved = crate::popover::place_popover(anchor_rect, size, visible, options);
    let content_frame = NSRect::new(
        NSPoint::new(
            resolved.x as f64,
            (main_screen_height - resolved.bottom()) as f64,
        ),
        NSSize::new(resolved.width as f64, resolved.height as f64),
    );
    let frame = unsafe { child.frameRectForContentRect(content_frame) };
    child.setFrame_display(frame, false);
    Ok(resolved)
}

/// Order a native window without accidentally making a `focus(false)` window key.
pub(crate) fn set_window_visibility(
    window: &Arc<Window>,
    visible: bool,
    focus: bool,
) -> Result<(), String> {
    let window = appkit_window(window)?;
    if visible {
        if focus {
            window.makeKeyAndOrderFront(None);
        } else {
            window.orderFront(None);
        }
    } else {
        window.orderOut(None);
    }
    Ok(())
}

/// Change whether one Winit-owned AppKit window may become key.
pub(crate) fn set_window_focusable(window: &Arc<Window>, focusable: bool) -> Result<(), String> {
    if window.set_can_become_key_window(focusable) {
        Ok(())
    } else {
        Err("the native window did not expose key-window policy".to_owned())
    }
}

/// Apply whole-window alpha without changing the retained GPU scene.
pub(crate) fn set_window_opacity(window: &Arc<Window>, opacity: f32) -> Result<(), String> {
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return Err("window opacity must be finite and between zero and one".to_owned());
    }
    let window = appkit_window(window)?;
    // SAFETY: QuickGUI owns this live main-thread AppKit window and the bounded scalar carries no
    // borrowed Objective-C state.
    unsafe { window.setAlphaValue(f64::from(opacity)) };
    Ok(())
}

/// Apply an exact `NSWindowLevel` for one Winit-owned AppKit window.
///
/// Winit only understands three portable levels, so QuickGUI applies the winit hint first and
/// then narrows to the precise AppKit constant named by [`WindowLevel`].
pub(crate) fn set_window_level(window: &Arc<Window>, level: WindowLevel) -> Result<(), String> {
    let window = appkit_window(window)?;
    window.setLevel(level.macos_level() as isize);
    Ok(())
}

/// Raise one Winit-owned AppKit window to the front of its stacking level.
///
/// This is an ordering change only: it never activates the application and never makes the window
/// key, so a floating panel can surface without stealing focus.
pub(crate) fn order_window_front(window: &Arc<Window>) -> Result<(), String> {
    let window = appkit_window(window)?;
    window.orderFront(None);
    Ok(())
}

/// Order one AppKit window immediately above another window owned by this process.
pub(crate) fn order_window_above(window: &Arc<Window>, above: &Arc<Window>) -> Result<(), String> {
    let window = appkit_window(window)?;
    let above = appkit_window(above)?;
    // SAFETY: the sibling is a live main-thread AppKit window owned by QuickGUI.
    let relative = unsafe { above.windowNumber() };
    // SAFETY: both windows are live main-thread AppKit windows owned by QuickGUI, and the
    // relative window number was just read from the sibling.
    unsafe { window.orderWindow_relativeTo(NSWindowOrderingMode::NSWindowAbove, relative) };
    Ok(())
}

/// Let clicks fall through one AppKit window, optionally keeping mouse-moved delivery.
///
/// `forward` sets `acceptsMouseMovedEvents`, so the window keeps receiving the event-driven
/// `mouseMoved:` stream AppKit already delivers while every press reaches the window below. No
/// timer, tracking loop, or polling pass is installed.
pub(crate) fn set_window_ignores_mouse_events(
    window: &Arc<Window>,
    ignore: bool,
    forward: bool,
) -> Result<(), String> {
    let window = appkit_window(window)?;
    window.setIgnoresMouseEvents(ignore);
    window.setAcceptsMouseMovedEvents(!ignore || forward);
    Ok(())
}

/// Block or restore every native input event for one AppKit window.
///
/// AppKit has no `EnableWindow`, so QuickGUI expresses "visible but inert" with the documented
/// combination of ignoring mouse events and refusing key-window status.
pub(crate) fn set_window_input_enabled(window: &Arc<Window>, enabled: bool) -> Result<(), String> {
    let appkit = appkit_window(window)?;
    appkit.setIgnoresMouseEvents(!enabled);
    appkit.setAcceptsMouseMovedEvents(enabled);
    if !window.set_can_become_key_window(enabled) {
        return Err("the native window did not expose key-window policy".to_owned());
    }
    if !enabled {
        // SAFETY: QuickGUI owns this live main-thread AppKit window.
        unsafe { appkit.resignKeyWindow() };
    }
    Ok(())
}

/// Apply or clear AppKit's `contentAspectRatio` for one window.
pub(crate) fn set_window_aspect_ratio(
    window: &Arc<Window>,
    ratio: Option<Size>,
) -> Result<(), String> {
    let window = appkit_window(window)?;
    // SAFETY: QuickGUI owns this live main-thread AppKit window and passes validated scalars.
    match ratio {
        Some(ratio) => unsafe {
            window.setContentAspectRatio(NSSize::new(
                f64::from(ratio.width),
                f64::from(ratio.height),
            ));
        },
        None => window.setContentResizeIncrements(NSSize::new(1.0, 1.0)),
    }
    Ok(())
}

/// Show or hide the macOS traffic-light buttons without removing the titlebar.
pub(crate) fn set_window_button_visibility(
    window: &Arc<Window>,
    visible: bool,
) -> Result<(), String> {
    let window = appkit_window(window)?;
    for button in [
        NSWindowButton::NSWindowCloseButton,
        NSWindowButton::NSWindowMiniaturizeButton,
        NSWindowButton::NSWindowZoomButton,
    ] {
        if let Some(button) = window.standardWindowButton(button) {
            button.setHidden(!visible);
        }
    }
    Ok(())
}

/// Whether one AppKit window is currently miniaturized into the Dock.
pub(crate) fn is_window_miniaturized(window: &Arc<Window>) -> Result<bool, String> {
    let window = appkit_window(window)?;
    Ok(window.isMiniaturized())
}

/// Toggle AppKit's all-spaces collection behavior while preserving role-specific flags.
pub(crate) fn set_window_visible_on_all_workspaces(
    window: &Arc<Window>,
    visible: bool,
) -> Result<(), String> {
    let window = appkit_window(window)?;
    // SAFETY: QuickGUI owns this live main-thread AppKit window for both synchronous calls.
    let mut behavior = unsafe { window.collectionBehavior() };
    if visible {
        behavior |= NSWindowCollectionBehavior::CanJoinAllSpaces;
    } else {
        behavior &= !NSWindowCollectionBehavior::CanJoinAllSpaces;
    }
    unsafe { window.setCollectionBehavior(behavior) };
    Ok(())
}

/// Apply native z-level, space, and animation semantics for one GPUI-shaped window role.
pub(crate) fn configure_window_kind(
    window: &Arc<Window>,
    kind: WindowKind,
    focus: bool,
    accepts_key_focus: bool,
) -> Result<(), String> {
    if matches!(kind, WindowKind::Popover | WindowKind::SystemPopover)
        && !window.set_panel_can_become_key_window(accepts_key_focus)
    {
        return Err("a popover panel did not expose key-window policy".to_owned());
    }
    let window = appkit_window(window)?;
    match kind {
        WindowKind::Normal | WindowKind::Dialog => window.setLevel(NSNormalWindowLevel),
        WindowKind::Floating => window.setLevel(NSFloatingWindowLevel),
        WindowKind::Popover | WindowKind::SystemPopover => unsafe {
            if !window.isKindOfClass(NSPanel::class()) {
                return Err("a popover was not allocated as an AppKit NSPanel".to_owned());
            }
            let panel: Retained<NSPanel> = Retained::cast(window.clone());
            panel.setFloatingPanel(true);
            panel.setBecomesKeyOnlyIfNeeded(!focus);
            window.setLevel(NSPopUpMenuWindowLevel);
            window.setHidesOnDeactivate(true);
            window.setAnimationBehavior(NSWindowAnimationBehavior::UtilityWindow);
            window.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary
                    | NSWindowCollectionBehavior::Transient,
            );
        },
    }
    Ok(())
}

/// Present a parent-owned role after QuickGUI has completed its hidden first surface frame.
pub(crate) fn present_window_relation(
    window: &Arc<Window>,
    parent: Option<&Arc<Window>>,
    kind: WindowKind,
) -> Result<bool, String> {
    let Some(parent) = parent else {
        return Ok(false);
    };
    let child = appkit_window(window)?;
    if kind == WindowKind::SystemPopover {
        let parent = appkit_window(parent)?;
        if let Some(previous) = unsafe { child.parentWindow() }
            && Retained::as_ptr(&previous) != Retained::as_ptr(&parent)
        {
            unsafe { previous.removeChildWindow(&child) };
        }
        if unsafe { child.parentWindow() }.is_none() {
            unsafe {
                parent.addChildWindow_ordered(&child, NSWindowOrderingMode::NSWindowAbove);
            }
        }
        return Ok(true);
    }
    if kind != WindowKind::Dialog {
        return Ok(false);
    }
    let mut parent = appkit_window(parent)?;
    while let Some(sheet) = unsafe { parent.attachedSheet() } {
        parent = sheet;
    }
    unsafe {
        parent.beginSheet_completionHandler(&child, None);
    }
    Ok(true)
}

/// End an AppKit parent-owned presentation before hiding or releasing the Winit window.
pub(crate) fn dismiss_window_relation(
    window: &Arc<Window>,
    kind: WindowKind,
) -> Result<(), String> {
    let child = appkit_window(window)?;
    if kind == WindowKind::SystemPopover {
        if let Some(parent) = unsafe { child.parentWindow() } {
            unsafe { parent.removeChildWindow(&child) };
        }
        return Ok(());
    }
    if kind != WindowKind::Dialog {
        return Ok(());
    }
    if let Some(parent) = unsafe { child.sheetParent() } {
        unsafe {
            parent.endSheet(&child);
        }
    }
    Ok(())
}

pub(crate) fn is_window_fullscreen(window: &Arc<Window>) -> Result<bool, String> {
    Ok(appkit_window(window)?
        .styleMask()
        .contains(NSWindowStyleMask::FullScreen))
}

/// Query AppKit without Winit's temporary style-mask mutation.
///
/// Winit adds `Titled | Resizable` around `isZoomed` for borderless windows. That mutation emits
/// resize and move callbacks, so asking from one of those callbacks can recurse indefinitely.
/// QuickGUI tracks programmatic borderless maximization itself and uses this direct query only for
/// native titled windows.
pub(crate) fn is_window_maximized(window: &Arc<Window>) -> Result<bool, String> {
    Ok(appkit_window(window)?.isZoomed())
}

/// Read the current pointer in the Winit content view's logical, top-left coordinate space.
///
/// Winit's file-hover events do not carry a position on macOS, and AppKit does not synthesize
/// ordinary mouse-move events during a native drag. Reading the window's current location at the
/// enter and submit boundaries keeps element-level file drops exact without polling.
pub(crate) fn current_pointer_position(window: &Arc<Window>) -> Option<Point> {
    MainThreadMarker::new()?;
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    let view = unsafe { handle.ns_view.as_ptr().cast::<NSView>().as_ref() }?;
    let window = view.window()?;
    let window_point = unsafe { window.mouseLocationOutsideOfEventStream() };
    let point = view.convertPoint_fromView(window_point, None);
    (point.x.is_finite() && point.y.is_finite()).then(|| Point::new(point.x as f32, point.y as f32))
}

/// Start AppKit's native window drag for the current mouse-down event.
///
/// AppKit's implicit movability is enabled only for the synchronous native drag and restored
/// immediately afterward, keeping hidden-inset windows restricted to declared app regions.
pub(crate) fn perform_window_drag(
    window: &Arc<Window>,
    restore_immovable: bool,
) -> Result<(), String> {
    if restore_immovable {
        set_window_movable(window, true)?;
    }
    let result = window
        .drag_window()
        .map_err(|error| format!("could not start the native window drag: {error}"));
    if restore_immovable {
        set_window_movable(window, false)?;
    }
    result
}

/// Move the standard AppKit window controls while preserving their native spacing and behavior.
///
/// QuickGUI positions the close button from the top-left in logical points, matching web-style
/// window APIs. AppKit lays the standard buttons out inside a private titlebar container and can
/// reset their frames when that container is resized. Keep the container pinned to the top of the
/// window and place the buttons inside it so the configured inset survives AppKit layout passes.
pub(crate) fn position_traffic_lights(window: &Arc<Window>, position: Point) -> Result<(), String> {
    MainThreadMarker::new()
        .ok_or_else(|| "traffic lights must be positioned on the AppKit main thread".to_owned())?;
    if !position.x.is_finite() || !position.y.is_finite() || position.x < 0.0 || position.y < 0.0 {
        return Err("traffic-light coordinates must be finite and non-negative".to_owned());
    }

    let handle = window
        .window_handle()
        .map_err(|error| format!("could not access the AppKit window handle: {error}"))?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("the active window does not expose an AppKit view".to_owned());
    };
    let view = unsafe { handle.ns_view.as_ptr().cast::<NSView>().as_ref() }
        .ok_or_else(|| "the AppKit content view pointer is null".to_owned())?;
    let window = view
        .window()
        .ok_or_else(|| "the AppKit content view is not attached to a window".to_owned())?;
    if window.styleMask().contains(NSWindowStyleMask::FullScreen) {
        return Ok(());
    }

    let close = window
        .standardWindowButton(NSWindowButton::NSWindowCloseButton)
        .ok_or_else(|| "the AppKit close button is unavailable".to_owned())?;
    let minimize = window
        .standardWindowButton(NSWindowButton::NSWindowMiniaturizeButton)
        .ok_or_else(|| "the AppKit minimize button is unavailable".to_owned())?;
    let zoom = window.standardWindowButton(NSWindowButton::NSWindowZoomButton);

    let close_frame = NSView::frame(&close);
    let minimize_frame = NSView::frame(&minimize);
    let button_step = minimize_frame.origin.x - close_frame.origin.x;
    if !button_step.is_finite() || button_step <= 0.0 {
        return Err("AppKit returned invalid traffic-light spacing".to_owned());
    }
    if !close_frame.size.height.is_finite() || close_frame.size.height <= 0.0 {
        return Err("AppKit returned an invalid traffic-light height".to_owned());
    }

    let button_container = unsafe { close.superview() }
        .ok_or_else(|| "AppKit returned no traffic-light button container".to_owned())?;
    let titlebar_container = unsafe { button_container.superview() }
        .ok_or_else(|| "AppKit returned no traffic-light titlebar container".to_owned())?;
    let window_height = window.frame().size.height;
    if !window_height.is_finite() || window_height <= 0.0 {
        return Err("AppKit returned an invalid window height".to_owned());
    }
    let current_titlebar_frame = titlebar_container.frame();
    let (titlebar_frame, button_origins) = traffic_light_layout(
        position,
        window_height,
        current_titlebar_frame,
        close_frame.size.height,
        button_step,
    );
    let buttons = [Some(close), Some(minimize), zoom];
    // The layout is re-checked on every window update, so leave AppKit's views untouched when
    // they already match; only a titlebar AppKit has laid out again pays for the restore.
    let unchanged = rects_match(current_titlebar_frame, titlebar_frame)
        && buttons.iter().zip(button_origins).all(|(button, origin)| {
            button
                .as_ref()
                .is_none_or(|button| points_match(NSView::frame(button).origin, origin))
        });
    if unchanged {
        return Ok(());
    }

    unsafe {
        titlebar_container.setFrame(titlebar_frame);
    }
    for (button, origin) in buttons
        .into_iter()
        .zip(button_origins)
        .filter_map(|(button, origin)| button.map(|button| (button, origin)))
    {
        let button_frame = NSView::frame(&button);
        if (button_frame.origin.x - origin.x).abs() > 0.01
            || (button_frame.origin.y - origin.y).abs() > 0.01
        {
            unsafe {
                NSView::setFrameOrigin(&button, origin);
            }
        }
        unsafe {
            button.updateTrackingAreas();
        }
    }
    unsafe {
        titlebar_container.updateTrackingAreas();
    }
    Ok(())
}

fn rects_match(a: NSRect, b: NSRect) -> bool {
    points_match(a.origin, b.origin)
        && (a.size.width - b.size.width).abs() <= 0.01
        && (a.size.height - b.size.height).abs() <= 0.01
}

fn points_match(a: NSPoint, b: NSPoint) -> bool {
    (a.x - b.x).abs() <= 0.01 && (a.y - b.y).abs() <= 0.01
}

/// Own the native window callbacks that keep one window's custom traffic-light inset stable.
///
/// Winit queues `WindowEvent::Resized` from its content view's frame-change callback. AppKit can
/// perform another private titlebar layout after that callback, so restoring the controls from the
/// queued event alone is not authoritative. Observe the native window notifications, matching
/// GPUI's `windowDidResize:` lifecycle, and perform the titlebar layout before each notification
/// finishes. AppKit also lays the titlebar out again in the display pass after the window first
/// comes onscreen, which resets the controls to their default inset until the next resize; a window
/// shown inactive never becomes key to trigger a restore, so the window-update notification that
/// follows every event cycle re-checks the layout as well, at the cost of a few frame reads when
/// nothing changed.
pub(crate) struct MacTrafficLightHost {
    notifications: Retained<NSNotificationCenter>,
    observers: Vec<Retained<NSObject>>,
}

impl MacTrafficLightHost {
    pub(crate) fn new(window: &Arc<Window>, position: Point) -> Result<Self, String> {
        position_traffic_lights(window, position)?;
        let native_window = appkit_window(window)?;
        let weak_window = Arc::downgrade(window);
        let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
            let Some(window) = weak_window.upgrade() else {
                return;
            };
            if let Err(error) = position_traffic_lights(&window, position) {
                tracing::warn!(%error, "could not restore the configured traffic-light position from a native window callback");
            }
        });
        let notifications = unsafe { NSNotificationCenter::defaultCenter() };
        let observers = unsafe {
            [
                NSWindowDidResizeNotification,
                NSWindowDidChangeOcclusionStateNotification,
                NSWindowDidBecomeKeyNotification,
                NSWindowDidExitFullScreenNotification,
                NSWindowDidUpdateNotification,
            ]
            .into_iter()
            .map(|name| {
                notifications.addObserverForName_object_queue_usingBlock(
                    Some(name),
                    Some(native_window.as_ref()),
                    None,
                    &block,
                )
            })
            .collect()
        };
        Ok(Self {
            notifications,
            observers,
        })
    }
}

impl Drop for MacTrafficLightHost {
    fn drop(&mut self) {
        for observer in &self.observers {
            unsafe {
                self.notifications.removeObserver(observer.as_ref());
            }
        }
    }
}

pub(super) fn traffic_light_layout(
    position: Point,
    window_height: f64,
    mut titlebar_frame: NSRect,
    button_height: f64,
    button_step: f64,
) -> (NSRect, [NSPoint; 3]) {
    let x = f64::from(position.x);
    let y = f64::from(position.y);
    let titlebar_height = button_height + y * 2.0;
    titlebar_frame.size.height = titlebar_height;
    titlebar_frame.origin.y = window_height - titlebar_height;
    (
        titlebar_frame,
        [
            NSPoint::new(x, y),
            NSPoint::new(x + button_step, y),
            NSPoint::new(x + button_step * 2.0, y),
        ],
    )
}

/// Covers an ordered-on-screen AppKit window until WGPU presents its first frame.
///
/// A native child view can otherwise participate in AppKit composition immediately while the
/// matching GPU scene is still being prepared, briefly exposing the child over an empty window.
pub(crate) struct MacFirstFrameGuard {
    parent: Retained<NSView>,
    window: Retained<NSWindow>,
    shield: Retained<NSBox>,
    revealed: bool,
}

/// Temporarily removes the AppKit content root from its still-hidden window.
///
/// WGPU's Metal backend deliberately skips drawable acquisition when the hosting `NSWindow` is
/// occluded. A detached layer has no hosting window, so it can receive and retain the complete first
/// drawable without ever ordering a partial window onscreen. The root is normally the Winit view;
/// when vibrancy is active it is the `NSVisualEffectView` that owns that same Winit view. Drop always
/// reattaches the exact retained root to the same window and leaves the window frame untouched.
pub(crate) struct MacDetachedContent {
    window: Retained<NSWindow>,
    content: Retained<NSView>,
}

impl MacFirstFrameGuard {
    pub fn new(window: &Arc<Window>, background: crate::Color) -> Result<Self, String> {
        let handle = window
            .window_handle()
            .map_err(|error| format!("could not access the AppKit window handle: {error}"))?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Err("the active window does not expose an AppKit view".to_owned());
        };
        let parent = unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
            .ok_or_else(|| "the AppKit content view could not be retained".to_owned())?;
        let window = parent
            .window()
            .ok_or_else(|| "the AppKit content view is not attached to a window".to_owned())?;
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            "the first-frame shield must be initialized on the main thread".to_owned()
        })?;
        let shield = unsafe { NSBox::initWithFrame(mtm.alloc(), parent.bounds()) };
        let [red, green, blue, _] = background.to_srgba8();
        let color = unsafe {
            NSColor::colorWithSRGBRed_green_blue_alpha(
                f64::from(red) / 255.0,
                f64::from(green) / 255.0,
                f64::from(blue) / 255.0,
                1.0,
            )
        };
        unsafe {
            shield.setBoxType(NSBoxType::NSBoxCustom);
            shield.setBorderWidth(0.0);
            shield.setTitlePosition(NSTitlePosition::NSNoTitle);
            shield.setFillColor(&color);
            shield.setTransparent(false);
            shield.setAutoresizingMask(
                NSAutoresizingMaskOptions::NSViewWidthSizable
                    | NSAutoresizingMaskOptions::NSViewHeightSizable,
            );
            parent.addSubview(&shield);
        }
        Ok(Self {
            parent,
            window,
            shield,
            revealed: false,
        })
    }

    pub fn detach_content_for_first_present(&self) -> Result<MacDetachedContent, String> {
        let content = self
            .window
            .contentView()
            .ok_or_else(|| "the AppKit window has no content view to pre-present".to_owned())?;
        if self
            .parent
            .window()
            .as_ref()
            .is_none_or(|window| !std::ptr::eq(window.as_ref(), self.window.as_ref()))
        {
            return Err("the Winit view is no longer attached to its AppKit window".to_owned());
        }
        self.window.setContentView(None);
        if self.parent.window().is_some() {
            self.window.setContentView(Some(&content));
            return Err("AppKit did not detach the hidden Winit content view".to_owned());
        }
        Ok(MacDetachedContent {
            window: self.window.clone(),
            content,
        })
    }

    /// Whether Winit can safely route APIs that look up the window's current content view.
    pub fn content_attached(&self) -> bool {
        self.parent.window().is_some()
    }

    /// Keep the shield above native children mounted while preparing the same frame.
    pub fn cover(&self) {
        unsafe {
            self.parent.addSubview_positioned_relativeTo(
                &self.shield,
                NSWindowOrderingMode::NSWindowAbove,
                None,
            );
        }
    }

    /// Reveal only after the renderer has completed a full frame for presentation.
    pub fn reveal(mut self) {
        self.restore();
    }

    fn restore(&mut self) {
        if self.revealed {
            return;
        }
        unsafe {
            self.shield.removeFromSuperview();
        }
        self.revealed = true;
    }
}

impl Drop for MacFirstFrameGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

impl Drop for MacDetachedContent {
    fn drop(&mut self) {
        self.window.setContentView(Some(&self.content));
    }
}
