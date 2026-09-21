use std::{
    mem,
    ptr::{null, null_mut},
    sync::{OnceLock, mpsc},
    thread::{self, JoinHandle},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::{
        LibraryLoader::GetModuleHandleW,
        RemoteDesktop::{
            NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
            WTSUnRegisterSessionNotification,
        },
    },
    UI::WindowsAndMessaging::{
        CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        GWLP_USERDATA, GetMessageW, MSG, PBT_APMPOWERSTATUSCHANGE, PBT_APMRESUMEAUTOMATIC,
        PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, PostMessageW, PostQuitMessage, RegisterClassW,
        SetWindowLongPtrW, TranslateMessage, WM_CLOSE, WM_DESTROY, WM_NCCREATE, WM_POWERBROADCAST,
        WM_QUERYENDSESSION, WM_WTSSESSION_CHANGE, WNDCLASSW, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
    },
};
use winit::event_loop::EventLoopProxy;

use super::{PowerEvent, RuntimeEvent};

const WINDOW_CLASS: &[u16] = &[
    b'Q' as u16,
    b'u' as u16,
    b'i' as u16,
    b'c' as u16,
    b'k' as u16,
    b'G' as u16,
    b'u' as u16,
    b'i' as u16,
    b'P' as u16,
    b'o' as u16,
    b'w' as u16,
    b'e' as u16,
    b'r' as u16,
    b'M' as u16,
    b'o' as u16,
    b'n' as u16,
    b'i' as u16,
    b't' as u16,
    b'o' as u16,
    b'r' as u16,
    0,
];

static WINDOW_CLASS_REGISTRATION: OnceLock<Result<(), String>> = OnceLock::new();

struct WindowState {
    proxy: EventLoopProxy<RuntimeEvent>,
    suspended: bool,
    locked: bool,
    power_source: quickgui_system::PowerSource,
}

pub(crate) struct WindowsPowerMonitor {
    window: isize,
    thread: Option<JoinHandle<()>>,
}

impl WindowsPowerMonitor {
    pub(crate) fn start(proxy: EventLoopProxy<RuntimeEvent>) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("quickgui-power-monitor".to_owned())
            .spawn(move || run_message_window(proxy, sender))
            .map_err(|error| error.to_string())?;
        match receiver.recv() {
            Ok(Ok(window)) => Ok(Self {
                window,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(error) => {
                let _ = thread.join();
                Err(error.to_string())
            }
        }
    }
}

impl Drop for WindowsPowerMonitor {
    fn drop(&mut self) {
        if self.window != 0 {
            unsafe {
                PostMessageW(self.window as HWND, WM_CLOSE, 0, 0);
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_message_window(
    proxy: EventLoopProxy<RuntimeEvent>,
    ready: mpsc::SyncSender<Result<isize, String>>,
) {
    unsafe {
        let instance = GetModuleHandleW(null());
        let registration = WINDOW_CLASS_REGISTRATION.get_or_init(|| {
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: WINDOW_CLASS.as_ptr(),
                ..mem::zeroed()
            };
            if RegisterClassW(&class) == 0 {
                Err(format!(
                    "could not register the Windows power-monitor class: {}",
                    std::io::Error::last_os_error()
                ))
            } else {
                Ok(())
            }
        });
        if let Err(error) = registration {
            let _ = ready.send(Err(error.clone()));
            return;
        }
        let state = Box::new(WindowState {
            proxy,
            suspended: false,
            locked: false,
            power_source: quickgui_system::PowerMonitor::snapshot()
                .map(|snapshot| snapshot.source())
                .unwrap_or(quickgui_system::PowerSource::Unknown),
        });
        let state = Box::into_raw(state);
        // This must be an invisible top-level window. `HWND_MESSAGE` windows do not receive the
        // WM_POWERBROADCAST and WM_QUERYENDSESSION broadcasts this monitor exists to observe.
        let window = CreateWindowExW(
            0,
            WINDOW_CLASS.as_ptr(),
            WINDOW_CLASS.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            state.cast(),
        );
        if window.is_null() {
            drop(Box::from_raw(state));
            let _ = ready.send(Err(
                "could not create the Windows power-monitor window".to_owned()
            ));
            return;
        }
        if WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) == 0 {
            DestroyWindow(window);
            let _ = ready.send(Err(
                "could not register Windows session notifications".to_owned()
            ));
            return;
        }
        if ready.send(Ok(window as isize)).is_err() {
            DestroyWindow(window);
            return;
        }
        let mut message: MSG = mem::zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        return 1;
    }
    let state = unsafe {
        let pointer =
            windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(window, GWLP_USERDATA)
                as *mut WindowState;
        pointer.as_mut()
    };
    match message {
        WM_POWERBROADCAST => {
            if let Some(state) = state {
                let event = match wparam as u32 {
                    PBT_APMPOWERSTATUSCHANGE => quickgui_system::PowerMonitor::snapshot()
                        .ok()
                        .map(|snapshot| snapshot.source())
                        .filter(|source| *source != state.power_source)
                        .map(|source| {
                            state.power_source = source;
                            PowerEvent::PowerSourceChanged(source)
                        }),
                    PBT_APMSUSPEND if !state.suspended => {
                        state.suspended = true;
                        Some(PowerEvent::Suspend)
                    }
                    PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND if state.suspended => {
                        state.suspended = false;
                        Some(PowerEvent::Resume)
                    }
                    _ => None,
                };
                if let Some(event) = event {
                    let _ = state.proxy.send_event(RuntimeEvent::Power(event));
                }
            }
            1
        }
        WM_QUERYENDSESSION => {
            if let Some(state) = state {
                let _ = state
                    .proxy
                    .send_event(RuntimeEvent::Power(PowerEvent::ShutdownRequested));
            }
            1
        }
        WM_WTSSESSION_CHANGE => {
            if let Some(state) = state {
                let event = match wparam as u32 {
                    WTS_SESSION_LOCK if !state.locked => {
                        state.locked = true;
                        Some(PowerEvent::LockScreen)
                    }
                    WTS_SESSION_UNLOCK if state.locked => {
                        state.locked = false;
                        Some(PowerEvent::UnlockScreen)
                    }
                    _ => None,
                };
                if let Some(event) = event {
                    let _ = state.proxy.send_event(RuntimeEvent::Power(event));
                }
            }
            0
        }
        WM_CLOSE => {
            unsafe { DestroyWindow(window) };
            0
        }
        WM_DESTROY => {
            unsafe {
                WTSUnRegisterSessionNotification(window);
                let pointer = windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                    window,
                    GWLP_USERDATA,
                ) as *mut WindowState;
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                if !pointer.is_null() {
                    drop(Box::from_raw(pointer));
                }
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
