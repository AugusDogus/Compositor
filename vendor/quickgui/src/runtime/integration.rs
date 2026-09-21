/// Compile-time availability of QuickGUI's renderer-independent desktop integrations.
///
/// A `true` value means the current target has a native backend. Runtime services, application
/// packaging, and user permissions can still reject an individual operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DesktopIntegrationSupport {
    pub system_notifications: bool,
    pub scheduled_notifications: bool,
    pub notification_replies: bool,
    pub native_application_menus: bool,
    pub native_popup_menus: bool,
    pub tray_icons: bool,
    pub programmable_tray_popup: bool,
    pub global_shortcuts: bool,
    pub single_instance: bool,
    pub dynamic_protocol_registration: bool,
    pub autostart: bool,
    pub window_icons: bool,
    pub window_focusability: bool,
    pub window_opacity: bool,
    pub skip_taskbar: bool,
    pub visible_on_all_workspaces: bool,
    pub cursor_control: bool,
    pub cursor_screen_position: bool,
    pub taskbar_progress: bool,
    pub taskbar_overlay_icons: bool,
    pub dock_badges: bool,
    pub dock_icons: bool,
    pub dock_menus: bool,
    pub recent_documents: bool,
    pub file_icons: bool,
    pub native_about_panel: bool,
    pub user_tasks: bool,
    /// Message boxes accept a suppression checkbox and a custom icon.
    pub message_box_checkboxes: bool,
    /// The operating system exposes a file preview panel.
    pub file_previews: bool,
    /// A system color panel can be shown and observed.
    pub color_panel: bool,
    /// A system font panel can be shown and observed.
    pub font_panel: bool,
    /// A native share sheet can be anchored to the current window.
    pub share_sheet: bool,
    /// The operating system can authenticate the current user with biometrics.
    pub biometric_authentication: bool,
}

impl DesktopIntegrationSupport {
    pub const fn current() -> Self {
        Self {
            system_notifications: cfg!(any(
                target_os = "macos",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "dragonfly",
                target_os = "openbsd",
                target_os = "netbsd"
            )),
            scheduled_notifications: cfg!(any(target_os = "macos", target_os = "windows")),
            notification_replies: cfg!(any(
                target_os = "macos",
                target_os = "windows",
                target_os = "linux"
            )),
            native_application_menus: cfg!(any(target_os = "macos", target_os = "windows")),
            native_popup_menus: cfg!(any(target_os = "macos", target_os = "windows")),
            tray_icons: cfg!(any(
                target_os = "macos",
                target_os = "windows",
                target_os = "linux"
            )),
            programmable_tray_popup: cfg!(any(target_os = "macos", target_os = "windows")),
            global_shortcuts: cfg!(any(
                target_os = "macos",
                target_os = "windows",
                target_os = "linux"
            )),
            single_instance: cfg!(any(
                target_os = "macos",
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "dragonfly",
                target_os = "openbsd",
                target_os = "netbsd"
            )),
            dynamic_protocol_registration: cfg!(any(target_os = "windows", target_os = "linux")),
            autostart: cfg!(any(
                target_os = "macos",
                target_os = "windows",
                target_os = "linux"
            )),
            window_icons: cfg!(not(target_os = "macos")),
            window_focusability: cfg!(any(target_os = "macos", target_os = "windows")),
            window_opacity: cfg!(any(target_os = "macos", target_os = "windows")),
            skip_taskbar: cfg!(target_os = "windows"),
            visible_on_all_workspaces: cfg!(target_os = "macos"),
            cursor_control: cfg!(not(any(target_os = "ios", target_os = "android"))),
            cursor_screen_position: cfg!(any(target_os = "macos", target_os = "windows")),
            taskbar_progress: cfg!(target_os = "windows"),
            taskbar_overlay_icons: cfg!(target_os = "windows"),
            dock_badges: cfg!(target_os = "macos"),
            dock_icons: cfg!(target_os = "macos"),
            dock_menus: cfg!(target_os = "macos"),
            recent_documents: cfg!(any(target_os = "macos", target_os = "windows")),
            file_icons: cfg!(any(target_os = "macos", target_os = "windows")),
            native_about_panel: cfg!(any(target_os = "macos", target_os = "windows")),
            user_tasks: cfg!(target_os = "windows"),
            message_box_checkboxes: cfg!(target_os = "macos"),
            file_previews: cfg!(target_os = "macos"),
            color_panel: cfg!(target_os = "macos"),
            font_panel: cfg!(target_os = "macos"),
            share_sheet: cfg!(target_os = "macos"),
            biometric_authentication: cfg!(target_os = "macos"),
        }
    }
}

impl Default for DesktopIntegrationSupport {
    fn default() -> Self {
        Self::current()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_support_matches_compiled_backend_boundaries() {
        let support = DesktopIntegrationSupport::current();
        assert_eq!(
            support.native_popup_menus,
            cfg!(any(target_os = "macos", target_os = "windows"))
        );
        assert_eq!(support.taskbar_progress, cfg!(target_os = "windows"));
        assert_eq!(support.taskbar_overlay_icons, cfg!(target_os = "windows"));
        assert_eq!(support.dock_badges, cfg!(target_os = "macos"));
        assert_eq!(support.dock_icons, cfg!(target_os = "macos"));
        assert_eq!(support.dock_menus, cfg!(target_os = "macos"));
        assert_eq!(
            support.recent_documents,
            cfg!(any(target_os = "macos", target_os = "windows"))
        );
        assert_eq!(
            support.file_icons,
            cfg!(any(target_os = "macos", target_os = "windows"))
        );
        assert_eq!(
            support.native_about_panel,
            cfg!(any(target_os = "macos", target_os = "windows"))
        );
        assert_eq!(support.user_tasks, cfg!(target_os = "windows"));
        assert_eq!(support.message_box_checkboxes, cfg!(target_os = "macos"));
        assert_eq!(support.file_previews, cfg!(target_os = "macos"));
        assert_eq!(support.color_panel, cfg!(target_os = "macos"));
        assert_eq!(support.font_panel, cfg!(target_os = "macos"));
        assert_eq!(support.share_sheet, cfg!(target_os = "macos"));
        assert_eq!(support.biometric_authentication, cfg!(target_os = "macos"));
        assert_eq!(
            support.dynamic_protocol_registration,
            crate::ProtocolRegistration::supports_dynamic_registration()
        );
        assert_eq!(support.autostart, crate::AutoStart::is_supported());
    }
}
