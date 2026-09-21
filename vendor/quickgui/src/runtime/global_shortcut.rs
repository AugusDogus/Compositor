use super::*;

use std::sync::{Mutex, Once};

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, hotkey::HotKey};

/// Maximum global shortcuts retained by one QuickGUI application.
pub const MAX_GLOBAL_SHORTCUTS: usize = 256;

/// Maximum UTF-8 bytes accepted for one global-shortcut accelerator.
pub const MAX_GLOBAL_SHORTCUT_ACCELERATOR_BYTES: usize = 256;

/// One registered system-wide keyboard shortcut was pressed.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GlobalShortcutEvent {
    pub registration_id: u32,
}

pub(super) enum GlobalShortcutCommand {
    Register {
        registration_id: u32,
        accelerator: String,
        responder: crate::platform::PlatformResponder<()>,
    },
    Unregister {
        registration_id: u32,
        responder: crate::platform::PlatformResponder<()>,
    },
    UnregisterAll {
        responder: crate::platform::PlatformResponder<()>,
    },
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub(super) enum GlobalShortcutState {
    Pending,
    Ready(GlobalHotKeyManager),
    Unavailable(Arc<str>),
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub(super) enum GlobalShortcutState {
    Unavailable,
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
#[derive(Clone, Copy)]
pub(super) struct RegisteredGlobalShortcut {
    pub hotkey: HotKey,
    pub registration_id: u32,
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
static GLOBAL_SHORTCUT_PROXY: Mutex<Option<EventLoopProxy<RuntimeEvent>>> = Mutex::new(None);
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
static INSTALL_GLOBAL_SHORTCUT_HANDLER: Once = Once::new();

#[cfg(not(target_arch = "wasm32"))]
impl AppRunner {
    pub const fn global_shortcuts_supported(&self) -> bool {
        DesktopIntegrationSupport::current().global_shortcuts
    }

    /// Whether a completed registration with this application id is currently owned.
    pub fn is_global_shortcut_registered(&self, registration_id: u32) -> bool {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        return self
            .runtime
            .global_shortcuts
            .values()
            .any(|shortcut| shortcut.registration_id == registration_id);
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        false
    }

    /// Sorted ids of completed global-shortcut registrations.
    pub fn global_shortcut_registration_ids(&self) -> Vec<u32> {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        {
            let mut ids = self
                .runtime
                .global_shortcuts
                .values()
                .map(|shortcut| shortcut.registration_id)
                .collect::<Vec<_>>();
            ids.sort_unstable();
            ids
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        Vec::new()
    }

    /// Register one system-wide keyboard shortcut.
    ///
    /// Accelerator strings use the `global-hotkey` grammar, for example
    /// `shift+alt+KeyQ`. Modifiers must precede the final physical key code.
    pub fn register_global_shortcut(
        &mut self,
        registration_id: u32,
        accelerator: impl Into<String>,
    ) -> Result<PlatformResponse<()>, PlatformError> {
        if registration_id == 0 || !matches!(self.status, AppRunStatus::Continue) {
            return Err(PlatformError::Unavailable);
        }
        if self.runtime.pending_global_shortcut_commands.len() == MAX_GLOBAL_SHORTCUTS {
            return Err(PlatformError::PendingQueueFull);
        }
        let accelerator = accelerator.into();
        validate_global_shortcut_accelerator(&accelerator)?;
        let (responder, response) = crate::platform::response_channel();
        self.runtime
            .event_proxy
            .send_event(RuntimeEvent::ExternalCommandsReady)
            .map_err(|_| PlatformError::Unavailable)?;
        self.runtime
            .pending_global_shortcut_commands
            .push_back(GlobalShortcutCommand::Register {
                registration_id,
                accelerator,
                responder,
            });
        Ok(response)
    }

    /// Unregister one system-wide keyboard shortcut by its application-owned id.
    pub fn unregister_global_shortcut(
        &mut self,
        registration_id: u32,
    ) -> Result<PlatformResponse<()>, PlatformError> {
        if registration_id == 0 || !matches!(self.status, AppRunStatus::Continue) {
            return Err(PlatformError::Unavailable);
        }
        self.queue_global_shortcut_command(|responder| GlobalShortcutCommand::Unregister {
            registration_id,
            responder,
        })
    }

    /// Unregister all system-wide keyboard shortcuts owned by this application.
    pub fn unregister_all_global_shortcuts(
        &mut self,
    ) -> Result<PlatformResponse<()>, PlatformError> {
        if !matches!(self.status, AppRunStatus::Continue) {
            return Err(PlatformError::Unavailable);
        }
        self.queue_global_shortcut_command(|responder| GlobalShortcutCommand::UnregisterAll {
            responder,
        })
    }

    fn queue_global_shortcut_command(
        &mut self,
        command: impl FnOnce(crate::platform::PlatformResponder<()>) -> GlobalShortcutCommand,
    ) -> Result<PlatformResponse<()>, PlatformError> {
        if self.runtime.pending_global_shortcut_commands.len() == MAX_GLOBAL_SHORTCUTS {
            return Err(PlatformError::PendingQueueFull);
        }
        let (responder, response) = crate::platform::response_channel();
        self.runtime
            .event_proxy
            .send_event(RuntimeEvent::ExternalCommandsReady)
            .map_err(|_| PlatformError::Unavailable)?;
        self.runtime
            .pending_global_shortcut_commands
            .push_back(command(responder));
        Ok(response)
    }
}

fn validate_global_shortcut_accelerator(accelerator: &str) -> Result<(), PlatformError> {
    if accelerator.is_empty()
        || accelerator.len() > MAX_GLOBAL_SHORTCUT_ACCELERATOR_BYTES
        || accelerator.contains('\0')
    {
        return Err(PlatformError::Platform(Arc::from(
            "a global-shortcut accelerator must be nonempty, NUL-free, and at most 256 UTF-8 bytes",
        )));
    }
    Ok(())
}

impl Runtime {
    pub(super) fn initialize_global_shortcuts(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        {
            if !matches!(self.global_shortcut_state, GlobalShortcutState::Pending) {
                return;
            }
            install_global_shortcut_handler(self.event_proxy.clone());
            self.global_shortcut_state = match GlobalHotKeyManager::new() {
                Ok(manager) => GlobalShortcutState::Ready(manager),
                Err(error) => GlobalShortcutState::Unavailable(Arc::from(error.to_string())),
            };
        }
    }

    pub(super) fn process_global_shortcut_commands(&mut self) {
        while let Some(command) = self.pending_global_shortcut_commands.pop_front() {
            self.process_global_shortcut_command(command);
        }
    }

    fn process_global_shortcut_command(&mut self, command: GlobalShortcutCommand) {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        {
            match command {
                GlobalShortcutCommand::Register {
                    registration_id,
                    accelerator,
                    responder,
                } => {
                    let result = self.register_global_shortcut_now(registration_id, &accelerator);
                    responder.complete(result);
                }
                GlobalShortcutCommand::Unregister {
                    registration_id,
                    responder,
                } => {
                    let result = self.unregister_global_shortcut_now(registration_id);
                    responder.complete(result);
                }
                GlobalShortcutCommand::UnregisterAll { responder } => {
                    let result = self.unregister_all_global_shortcuts_now();
                    responder.complete(result);
                }
            }
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            let responder = match command {
                GlobalShortcutCommand::Register { responder, .. }
                | GlobalShortcutCommand::Unregister { responder, .. }
                | GlobalShortcutCommand::UnregisterAll { responder } => responder,
            };
            responder.complete(Err(PlatformError::Unsupported));
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn register_global_shortcut_now(
        &mut self,
        registration_id: u32,
        accelerator: &str,
    ) -> Result<(), PlatformError> {
        if self
            .global_shortcuts
            .values()
            .any(|shortcut| shortcut.registration_id == registration_id)
        {
            return Err(platform_global_shortcut_error(
                "the global-shortcut registration id is already in use",
            ));
        }
        if self.global_shortcuts.len() == MAX_GLOBAL_SHORTCUTS {
            return Err(PlatformError::PendingQueueFull);
        }
        let hotkey = accelerator
            .parse::<HotKey>()
            .map_err(|error| platform_global_shortcut_error(error.to_string()))?;
        if self.global_shortcuts.contains_key(&hotkey.id()) {
            return Err(platform_global_shortcut_error(
                "this global shortcut is already registered",
            ));
        }
        let manager = global_shortcut_manager(&self.global_shortcut_state)?;
        manager
            .register(hotkey)
            .map_err(|error| platform_global_shortcut_error(error.to_string()))?;
        self.global_shortcuts.insert(
            hotkey.id(),
            RegisteredGlobalShortcut {
                hotkey,
                registration_id,
            },
        );
        Ok(())
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn unregister_global_shortcut_now(
        &mut self,
        registration_id: u32,
    ) -> Result<(), PlatformError> {
        let Some((&hotkey_id, shortcut)) = self
            .global_shortcuts
            .iter()
            .find(|(_, shortcut)| shortcut.registration_id == registration_id)
        else {
            return Ok(());
        };
        let hotkey = shortcut.hotkey;
        let manager = global_shortcut_manager(&self.global_shortcut_state)?;
        manager
            .unregister(hotkey)
            .map_err(|error| platform_global_shortcut_error(error.to_string()))?;
        self.global_shortcuts.remove(&hotkey_id);
        Ok(())
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn unregister_all_global_shortcuts_now(&mut self) -> Result<(), PlatformError> {
        if self.global_shortcuts.is_empty() {
            return Ok(());
        }
        let hotkeys = self
            .global_shortcuts
            .values()
            .map(|shortcut| shortcut.hotkey)
            .collect::<Vec<_>>();
        let manager = global_shortcut_manager(&self.global_shortcut_state)?;
        manager
            .unregister_all(&hotkeys)
            .map_err(|error| platform_global_shortcut_error(error.to_string()))?;
        self.global_shortcuts.clear();
        Ok(())
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    pub(super) fn invoke_global_shortcut(&mut self, event_loop: &ActiveEventLoop, hotkey_id: u32) {
        let Some(shortcut) = self.global_shortcuts.get(&hotkey_id) else {
            return;
        };
        let Some(mut callback) = self.application_callbacks.global_shortcut.take() else {
            return;
        };
        let event = GlobalShortcutEvent {
            registration_id: shortcut.registration_id,
        };
        let mut context = self.event_context();
        callback(event, &mut context);
        self.application_callbacks.global_shortcut = Some(callback);
        self.apply_application_context(event_loop, context);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn global_shortcut_manager(
    state: &GlobalShortcutState,
) -> Result<&GlobalHotKeyManager, PlatformError> {
    match state {
        GlobalShortcutState::Ready(manager) => Ok(manager),
        GlobalShortcutState::Pending => Err(PlatformError::Unavailable),
        GlobalShortcutState::Unavailable(error) => {
            Err(platform_global_shortcut_error(error.as_ref()))
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn platform_global_shortcut_error(error: impl Into<Arc<str>>) -> PlatformError {
    PlatformError::Platform(error.into())
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn install_global_shortcut_handler(proxy: EventLoopProxy<RuntimeEvent>) {
    *GLOBAL_SHORTCUT_PROXY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(proxy);
    INSTALL_GLOBAL_SHORTCUT_HANDLER.call_once(|| {
        GlobalHotKeyEvent::set_event_handler(Some(|event: GlobalHotKeyEvent| {
            if event.state != global_hotkey::HotKeyState::Pressed {
                return;
            }
            let proxy = GLOBAL_SHORTCUT_PROXY
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            if let Some(proxy) = proxy {
                let _ = proxy.send_event(RuntimeEvent::GlobalShortcut(event.id));
            }
        }));
    });
}

pub(super) fn clear_global_shortcut_handler_proxy() {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        *GLOBAL_SHORTCUT_PROXY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerator_bounds_are_enforced_before_platform_work() {
        assert!(validate_global_shortcut_accelerator("shift+alt+KeyQ").is_ok());
        assert!(validate_global_shortcut_accelerator("").is_err());
        assert!(validate_global_shortcut_accelerator("KeyQ\0").is_err());
        assert!(
            validate_global_shortcut_accelerator(
                &"x".repeat(MAX_GLOBAL_SHORTCUT_ACCELERATOR_BYTES + 1)
            )
            .is_err()
        );
    }
}
