use std::{ffi::c_void, mem::size_of, os::windows::ffi::OsStrExt, path::Path};

use windows::{
    Win32::{
        Foundation::PROPERTYKEY,
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
            DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HGDIOBJ, ReleaseDC, SelectObject,
        },
        Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES,
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, StructuredStorage::PROPVARIANT},
        UI::{
            Shell::{
                Common::{IObjectArray, IObjectCollection},
                DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW,
                PropertiesSystem::IPropertyStore,
                SHARD_PATHW, SHAddToRecentDocs, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
                SHGFI_SMALLICON, SHGetFileInfoW, SetCurrentProcessExplicitAppUserModelID,
                ShellAboutW, ShellLink,
            },
            WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx, SW_SHOWNORMAL},
        },
    },
    core::{GUID, Interface, PCWSTR},
};

use crate::{AboutPanelOptions, FileIconSize, Image, PlatformError, UserTask};

const PKEY_TITLE: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xf29f85e0_4ff9_1068_ab91_08002b27b3d9),
    pid: 2,
};

pub(super) fn add_recent_document(path: &Path) {
    let wide = wide_path(path);
    // SAFETY: The NUL-terminated path remains alive through the synchronous shell call.
    unsafe {
        SHAddToRecentDocs(SHARD_PATHW.0 as u32, Some(wide.as_ptr().cast::<c_void>()));
    }
}

pub(super) fn clear_recent_documents() {
    // SAFETY: A null payload with SHARD_PATHW is the documented request to clear the list.
    unsafe { SHAddToRecentDocs(SHARD_PATHW.0 as u32, None) };
}

pub(super) fn set_current_app_id(app_id: &str) -> Result<(), PlatformError> {
    validate_app_id(app_id)?;
    let app_id = wide_text(app_id);
    // SAFETY: The NUL-terminated identifier remains alive through the synchronous shell call.
    unsafe { SetCurrentProcessExplicitAppUserModelID(PCWSTR(app_id.as_ptr())) }
        .map_err(platform_error)
}

pub(super) fn validate_app_id(app_id: &str) -> Result<(), PlatformError> {
    // Windows limits explicit application user model identifiers to 128 characters. AppInfo
    // identifiers are ASCII, so their UTF-8 byte and UTF-16 code-unit lengths are identical.
    if app_id.len() > 128 {
        return Err(PlatformError::Platform(
            "the Windows application user model identifier exceeds 128 characters".into(),
        ));
    }
    Ok(())
}

pub(super) fn show_about_panel(options: &AboutPanelOptions) -> Result<(), PlatformError> {
    let name = options
        .application_name
        .as_deref()
        .unwrap_or(env!("CARGO_PKG_NAME"));
    let app = match options
        .application_version
        .as_deref()
        .or(options.version.as_deref())
    {
        Some(version) if !version.is_empty() => format!("{name}#{version}"),
        _ => name.to_owned(),
    };
    let mut detail = Vec::new();
    if let Some(copyright) = options.copyright.as_deref() {
        detail.push(copyright);
    }
    if let Some(credits) = options.credits.as_deref() {
        detail.push(credits);
    }
    let app = wide_text(&app);
    let detail = wide_text(&detail.join("\n\n"));
    let icon = options
        .icon
        .as_ref()
        .map(super::windows_window::create_native_icon)
        .transpose()
        .map_err(|error| PlatformError::Platform(error.into()))?;
    // SAFETY: The UTF-16 buffers and optional HICON outlive the synchronous modal shell call.
    let shown = unsafe {
        ShellAboutW(
            None,
            PCWSTR(app.as_ptr()),
            PCWSTR(detail.as_ptr()),
            icon.as_ref().map(super::windows_window::OwnedIcon::handle),
        )
    };
    if shown == 0 {
        Err(PlatformError::Platform(
            std::io::Error::last_os_error().to_string().into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn file_icon(path: &Path, size: FileIconSize) -> Result<Image, PlatformError> {
    let path = wide_path(path);
    let mut info = SHFILEINFOW::default();
    let flags = SHGFI_ICON
        | if size == FileIconSize::Small {
            SHGFI_SMALLICON
        } else {
            SHGFI_LARGEICON
        };
    // SAFETY: `info` is writable for its full declared size and the path is NUL-terminated.
    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR(path.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES::default(),
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    };
    if result == 0 || info.hIcon.is_invalid() {
        return Err(PlatformError::Platform(
            std::io::Error::last_os_error().to_string().into(),
        ));
    }
    let rendered = render_icon(info.hIcon, size.pixels());
    // SAFETY: SHGetFileInfoW transferred ownership of this HICON to the caller.
    let _ = unsafe { DestroyIcon(info.hIcon) };
    rendered
}

pub(super) fn set_user_tasks(
    tasks: &[UserTask],
    app_id: Option<&str>,
) -> Result<(), PlatformError> {
    // SAFETY: Winit initializes OLE on this application thread before platform requests run.
    let destination: ICustomDestinationList = unsafe {
        CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER).map_err(platform_error)?
    };
    if let Some(app_id) = app_id {
        set_current_app_id(app_id)?;
    }
    let app_id = app_id.map(wide_text);
    if let Some(app_id) = &app_id {
        unsafe { destination.SetAppID(PCWSTR(app_id.as_ptr())) }.map_err(platform_error)?;
    }
    if tasks.is_empty() {
        unsafe {
            destination.DeleteList(
                app_id
                    .as_ref()
                    .map_or_else(PCWSTR::null, |value| PCWSTR(value.as_ptr())),
            )
        }
        .map_err(platform_error)?;
        return Ok(());
    }

    let mut minimum_slots = 0_u32;
    // BeginList also supplies the user's removed destinations. User tasks are commands rather than
    // destinations, so retaining the returned array through this transaction is sufficient.
    let _removed: IObjectArray =
        unsafe { destination.BeginList(&mut minimum_slots) }.map_err(platform_error)?;
    let result = (|| -> Result<(), PlatformError> {
        let collection: IObjectCollection = unsafe {
            CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)
                .map_err(platform_error)?
        };
        for task in tasks {
            let program = task
                .program
                .clone()
                .map_or_else(std::env::current_exe, Ok)
                .map_err(|error| PlatformError::Platform(error.to_string().into()))?;
            let link: IShellLinkW = unsafe {
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(platform_error)?
            };
            let program = wide_path(&program);
            let arguments = wide_text(&task.arguments);
            unsafe { link.SetPath(PCWSTR(program.as_ptr())) }.map_err(platform_error)?;
            unsafe { link.SetArguments(PCWSTR(arguments.as_ptr())) }.map_err(platform_error)?;
            unsafe { link.SetShowCmd(SW_SHOWNORMAL) }.map_err(platform_error)?;
            if let Some(description) = task.description.as_deref() {
                let description = wide_text(description);
                unsafe { link.SetDescription(PCWSTR(description.as_ptr())) }
                    .map_err(platform_error)?;
            }
            if let Some(directory) = task.working_directory.as_deref() {
                let directory = wide_path(directory);
                unsafe { link.SetWorkingDirectory(PCWSTR(directory.as_ptr())) }
                    .map_err(platform_error)?;
            }
            if let Some(icon) = task.icon_path.as_deref() {
                let icon = wide_path(icon);
                unsafe { link.SetIconLocation(PCWSTR(icon.as_ptr()), task.icon_index) }
                    .map_err(platform_error)?;
            }

            let store: IPropertyStore = link.cast().map_err(platform_error)?;
            let title = PROPVARIANT::from(task.title.as_ref());
            unsafe { store.SetValue(&PKEY_TITLE, &title) }.map_err(platform_error)?;
            unsafe { store.Commit() }.map_err(platform_error)?;
            unsafe { collection.AddObject(&link) }.map_err(platform_error)?;
        }
        let array: IObjectArray = collection.cast().map_err(platform_error)?;
        unsafe { destination.AddUserTasks(&array) }.map_err(platform_error)?;
        unsafe { destination.CommitList() }.map_err(platform_error)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = unsafe { destination.AbortList() };
        return Err(error);
    }
    Ok(())
}

fn render_icon(
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
    pixels: u32,
) -> Result<Image, PlatformError> {
    let pixels_i32 = i32::try_from(pixels).map_err(|_| PlatformError::Unsupported)?;
    let bytes = usize::try_from(pixels)
        .ok()
        .and_then(|side| side.checked_mul(side))
        .and_then(|area| area.checked_mul(4))
        .ok_or(PlatformError::Unsupported)?;
    let screen = unsafe { GetDC(None) };
    if screen.is_invalid() {
        return Err(last_platform_error());
    }
    let memory = unsafe { CreateCompatibleDC(Some(screen)) };
    if memory.is_invalid() {
        unsafe { ReleaseDC(None, screen) };
        return Err(last_platform_error());
    }
    let mut bitmap_info = BITMAPINFO::default();
    bitmap_info.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: pixels_i32,
        // A negative height requests a top-down pixel buffer.
        biHeight: -pixels_i32,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: bytes as u32,
        ..BITMAPINFOHEADER::default()
    };
    let mut bits = std::ptr::null_mut::<c_void>();
    let bitmap = match unsafe {
        CreateDIBSection(
            Some(screen),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )
    } {
        Ok(bitmap) => bitmap,
        Err(error) => {
            unsafe {
                let _ = DeleteDC(memory);
                ReleaseDC(None, screen);
            }
            return Err(platform_error(error));
        }
    };
    let previous = unsafe { SelectObject(memory, HGDIOBJ(bitmap.0)) };
    let draw_result = unsafe {
        DrawIconEx(
            memory, 0, 0, icon, pixels_i32, pixels_i32, 0, None, DI_NORMAL,
        )
    };
    let mut rgba = vec![0_u8; bytes];
    if draw_result.is_ok() && !bits.is_null() {
        // SAFETY: CreateDIBSection allocated exactly `bytes` writable bytes and retains them until
        // the bitmap is deleted below.
        let bgra = unsafe { std::slice::from_raw_parts(bits.cast::<u8>(), bytes) };
        for (source, target) in bgra.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
            let alpha = source[3];
            let unpremultiply = |value: u8| {
                if alpha == 0 {
                    value
                } else {
                    ((u32::from(value) * 255 + u32::from(alpha) / 2) / u32::from(alpha)).min(255)
                        as u8
                }
            };
            target[0] = unpremultiply(source[2]);
            target[1] = unpremultiply(source[1]);
            target[2] = unpremultiply(source[0]);
            target[3] = if alpha == 0 && source[..3].iter().any(|channel| *channel != 0) {
                255
            } else {
                alpha
            };
        }
    }
    unsafe {
        SelectObject(memory, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
    }
    draw_result.map_err(platform_error)?;
    Image::from_rgba(pixels, pixels, rgba)
        .map_err(|error| PlatformError::Platform(error.to_string().into()))
}

fn wide_text(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn platform_error(error: windows::core::Error) -> PlatformError {
    PlatformError::Platform(error.to_string().into())
}

fn last_platform_error() -> PlatformError {
    PlatformError::Platform(std::io::Error::last_os_error().to_string().into())
}
