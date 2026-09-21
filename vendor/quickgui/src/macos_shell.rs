use std::path::Path;

use objc2::{ClassType, rc::Retained, runtime::AnyObject};
use objc2_app_kit::{
    NSAboutPanelOptionApplicationIcon, NSAboutPanelOptionApplicationName,
    NSAboutPanelOptionApplicationVersion, NSAboutPanelOptionCredits, NSAboutPanelOptionKey,
    NSAboutPanelOptionVersion, NSApplication, NSApplicationActivationPolicy, NSBeep,
    NSDocumentController, NSImage, NSRequestUserAttentionType, NSWorkspace,
};
use objc2_foundation::{MainThreadMarker, NSAttributedString, NSDictionary, NSString};

use crate::{
    AboutPanelOptions, ActivationPolicy, ApplicationsFolderSupport, DockAttention,
    DockAttentionRequest, FileIconSize, Image, PlatformError,
};

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn EnableSecureEventInput() -> i32;
    fn DisableSecureEventInput() -> i32;
    fn IsSecureEventInputEnabled() -> bool;
}

/// Apply an `NSApplicationActivationPolicy`.
pub(crate) fn set_activation_policy(policy: ActivationPolicy) -> Result<(), PlatformError> {
    let mtm = main_thread()?;
    let native = match policy {
        ActivationPolicy::Regular => NSApplicationActivationPolicy::Regular,
        ActivationPolicy::Accessory => NSApplicationActivationPolicy::Accessory,
        ActivationPolicy::Prohibited => NSApplicationActivationPolicy::Prohibited,
    };
    if NSApplication::sharedApplication(mtm).setActivationPolicy(native) {
        Ok(())
    } else {
        Err(PlatformError::Platform(
            "AppKit refused the requested activation policy".into(),
        ))
    }
}

/// Bring the application forward. `force` uses AppKit's ignore-other-apps activation.
pub(crate) fn activate_application(force: bool) -> Result<(), PlatformError> {
    let mtm = main_thread()?;
    let application = NSApplication::sharedApplication(mtm);
    if force {
        // `activateIgnoringOtherApps:` is the only API that steals focus from the frontmost
        // application, which is exactly what `force` promises.
        #[allow(deprecated)]
        application.activateIgnoringOtherApps(true);
    } else {
        // SAFETY: a main-thread AppKit activation request taking no arguments.
        unsafe { application.activate() };
    }
    Ok(())
}

pub(crate) fn hide_application() -> Result<(), PlatformError> {
    let mtm = main_thread()?;
    NSApplication::sharedApplication(mtm).hide(None);
    Ok(())
}

pub(crate) fn unhide_application() -> Result<(), PlatformError> {
    let mtm = main_thread()?;
    // SAFETY: `unhide:` is a main-thread AppKit action taking an optional sender.
    unsafe { NSApplication::sharedApplication(mtm).unhide(None) };
    Ok(())
}

pub(crate) fn request_dock_attention(
    attention: DockAttention,
) -> Result<DockAttentionRequest, PlatformError> {
    let mtm = main_thread()?;
    let native = match attention {
        DockAttention::Critical => NSRequestUserAttentionType::NSCriticalRequest,
        DockAttention::Informational => NSRequestUserAttentionType::NSInformationalRequest,
    };
    let id = NSApplication::sharedApplication(mtm).requestUserAttention(native);
    Ok(DockAttentionRequest::new(id as i64))
}

pub(crate) fn cancel_dock_attention(request: DockAttentionRequest) -> Result<(), PlatformError> {
    let mtm = main_thread()?;
    // SAFETY: cancelling an unknown identifier is a documented AppKit no-op.
    unsafe {
        NSApplication::sharedApplication(mtm).cancelUserAttentionRequest(request.get() as isize);
    }
    Ok(())
}

/// Show or hide the Dock tile by switching between the Regular and Accessory policies.
pub(crate) fn set_dock_visible(visible: bool) -> Result<(), PlatformError> {
    set_activation_policy(if visible {
        ActivationPolicy::Regular
    } else {
        ActivationPolicy::Accessory
    })
}

/// Route every keystroke straight to this process, bypassing input monitoring.
pub(crate) fn set_secure_keyboard_entry(enabled: bool) -> Result<(), PlatformError> {
    main_thread()?;
    // SAFETY: both Carbon entry points take no arguments and are safe to call repeatedly; the
    // enabled query keeps the enable/disable counter balanced.
    let status = unsafe {
        if enabled {
            if IsSecureEventInputEnabled() {
                return Ok(());
            }
            EnableSecureEventInput()
        } else {
            if !IsSecureEventInputEnabled() {
                return Ok(());
            }
            DisableSecureEventInput()
        }
    };
    if status == 0 {
        Ok(())
    } else {
        Err(PlatformError::Platform(
            format!("secure keyboard entry failed with status {status}").into(),
        ))
    }
}

pub(crate) fn beep() -> Result<(), PlatformError> {
    main_thread()?;
    // SAFETY: `NSBeep` takes no arguments and has no failure mode.
    unsafe { NSBeep() };
    Ok(())
}

/// Whether this process can relocate its bundle into an `/Applications` directory.
pub(crate) fn applications_folder_support() -> ApplicationsFolderSupport {
    let Some(bundle) = current_application_bundle() else {
        return ApplicationsFolderSupport::default();
    };
    let already_installed = bundle
        .parent()
        .is_some_and(|parent| parent.ends_with("Applications"));
    ApplicationsFolderSupport {
        supported: true,
        already_installed,
    }
}

/// Move the running application bundle into `/Applications`.
///
/// Returns `false` when the bundle is already installed there. The caller relaunches; QuickGUI
/// never restarts the process behind the application's back.
pub(crate) fn move_to_applications_folder() -> Result<bool, PlatformError> {
    let support = applications_folder_support();
    if !support.supported {
        return Err(PlatformError::Unsupported);
    }
    if support.already_installed {
        return Ok(false);
    }
    let bundle = current_application_bundle().ok_or(PlatformError::Unsupported)?;
    let name = bundle
        .file_name()
        .ok_or_else(|| PlatformError::Platform("the application bundle has no name".into()))?;
    let destination = Path::new("/Applications").join(name);
    if destination.exists() {
        return Err(PlatformError::Platform(
            "an application with the same name is already installed".into(),
        ));
    }
    std::fs::rename(&bundle, &destination)
        .map_err(|error| PlatformError::Platform(error.to_string().into()))?;
    Ok(true)
}

/// Path of the `.app` bundle containing the running executable, when there is one.
fn current_application_bundle() -> Option<std::path::PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let bundle = executable.parent()?.parent()?.parent()?;
    let is_bundle = bundle
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
        && executable
            .parent()
            .is_some_and(|parent| parent.ends_with("MacOS"));
    is_bundle.then(|| bundle.to_path_buf())
}

pub(crate) fn set_dock_badge(value: Option<&str>) -> Result<(), PlatformError> {
    let mtm = main_thread()?;
    let application = NSApplication::sharedApplication(mtm);
    let dock_tile = unsafe { application.dockTile() };
    let value = value.map(NSString::from_str);
    unsafe {
        dock_tile.setBadgeLabel(value.as_deref());
        dock_tile.display();
    }
    Ok(())
}

pub(crate) fn set_dock_icon(icon: Option<&Image>) -> Result<(), PlatformError> {
    let mtm = main_thread()?;
    let icon = icon.map(|icon| native_image(mtm, icon)).transpose()?;
    unsafe {
        NSApplication::sharedApplication(mtm).setApplicationIconImage(icon.as_deref());
    }
    Ok(())
}

pub(crate) fn add_recent_document(path: &Path) -> Result<(), PlatformError> {
    let mtm = main_thread()?;
    let url = crate::macos::native_file_url(path, false)
        .map_err(|error| PlatformError::Platform(error.into()))?;
    unsafe { NSDocumentController::sharedDocumentController(mtm).noteNewRecentDocumentURL(&url) };
    Ok(())
}

pub(crate) fn clear_recent_documents() -> Result<(), PlatformError> {
    let mtm = main_thread()?;
    unsafe { NSDocumentController::sharedDocumentController(mtm).clearRecentDocuments(None) };
    Ok(())
}

pub(crate) fn show_about_panel(options: &AboutPanelOptions) -> Result<(), PlatformError> {
    let mtm = main_thread()?;
    let application = NSApplication::sharedApplication(mtm);
    let mut keys: Vec<&NSAboutPanelOptionKey> = Vec::with_capacity(5);
    let mut values: Vec<Retained<AnyObject>> = Vec::with_capacity(5);

    if let Some(value) = options.application_name.as_deref() {
        keys.push(unsafe { NSAboutPanelOptionApplicationName });
        values.push(unsafe { Retained::cast(NSString::from_str(value)) });
    }
    if let Some(value) = options.application_version.as_deref() {
        keys.push(unsafe { NSAboutPanelOptionApplicationVersion });
        values.push(unsafe { Retained::cast(NSString::from_str(value)) });
    }
    if let Some(value) = options.version.as_deref() {
        keys.push(unsafe { NSAboutPanelOptionVersion });
        values.push(unsafe { Retained::cast(NSString::from_str(value)) });
    }
    let credits = [options.copyright.as_deref(), options.credits.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n\n");
    if !credits.is_empty() {
        keys.push(unsafe { NSAboutPanelOptionCredits });
        let credits = NSAttributedString::initWithString(
            NSAttributedString::alloc(),
            &NSString::from_str(&credits),
        );
        values.push(unsafe { Retained::cast(credits) });
    }
    if let Some(icon) = options.icon.as_ref() {
        keys.push(unsafe { NSAboutPanelOptionApplicationIcon });
        values.push(unsafe { Retained::cast(native_image(mtm, icon)?) });
    }

    unsafe {
        if keys.is_empty() {
            application.orderFrontStandardAboutPanel(None);
        } else {
            let options: Retained<NSDictionary<NSAboutPanelOptionKey, AnyObject>> =
                NSDictionary::from_vec(&keys, values);
            application.orderFrontStandardAboutPanelWithOptions(&options);
        }
        application.activate();
    }
    Ok(())
}

pub(crate) fn file_icon(path: &Path, size: FileIconSize) -> Result<Image, PlatformError> {
    let mtm = main_thread()?;
    let url = crate::macos::native_file_url(path, path.is_dir())
        .map_err(|error| PlatformError::Platform(error.into()))?;
    let path = unsafe { url.path() }.ok_or_else(|| {
        PlatformError::Platform("Foundation could not represent the file path".into())
    })?;
    let native = unsafe { NSWorkspace::sharedWorkspace().iconForFile(&path) };
    let pixels = size.pixels();
    crate::macos::rasterize_native_image(mtm, &native, pixels, pixels)
        .map_err(|error| PlatformError::Platform(error.to_string().into()))
}

/// Convert a QuickGUI image into an `NSImage`, honoring its template flag and any additional
/// backing-scale representations.
pub(crate) fn native_image(
    mtm: MainThreadMarker,
    image: &Image,
) -> Result<Retained<NSImage>, PlatformError> {
    crate::macos::native_image_with_metadata(mtm, image)
}

fn main_thread() -> Result<MainThreadMarker, PlatformError> {
    MainThreadMarker::new().ok_or_else(|| {
        PlatformError::Platform("AppKit shell services require the main thread".into())
    })
}
