use objc2_app_kit::{NSButton, NSControlStateValueOff, NSControlStateValueOn};

use super::*;
use crate::platform::{MessageBoxOptions, MessageBoxResponse};

/// Present an `NSAlert` carrying an optional suppression checkbox and a custom icon.
///
/// The alert is retained by the caller until AppKit invokes the completion handler, exactly like
/// [`super::windowing::present_native_prompt`]. Every string was validated and bounded before the
/// request was queued, so this function only maps already-checked values onto AppKit.
pub(crate) fn present_native_message_box(
    window: Option<&Arc<Window>>,
    context: MacPlatformDialogContext,
    options: &MessageBoxOptions,
    responder: PlatformResponder<MessageBoxResponse>,
) -> Result<MacPlatformDialog, String> {
    let parent = window.map(deepest_appkit_sheet).transpose()?;
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "native message boxes must start on the AppKit main thread".to_owned())?;
    let alert = unsafe { NSAlert::new(mtm) };
    unsafe {
        alert.setAlertStyle(match options.level.unwrap_or(PromptLevel::Info) {
            PromptLevel::Info => NSAlertStyle::Informational,
            PromptLevel::Warning => NSAlertStyle::Warning,
            PromptLevel::Critical => NSAlertStyle::Critical,
        });
        alert.setMessageText(&NSString::from_str(&options.message));
        if let Some(detail) = &options.detail {
            alert.setInformativeText(&NSString::from_str(detail));
        }
        if let Some(icon) = &options.icon {
            let icon =
                crate::macos_shell::native_image(mtm, icon).map_err(|error| error.to_string())?;
            alert.setIcon(Some(&icon));
        }
    }

    let buttons = options.resolved_buttons();
    let default_index = options.default_button.unwrap_or(0);
    let cancel_index = options
        .cancel_button
        .or_else(|| buttons.iter().position(PromptButton::is_cancel));
    let mut natives = Vec::with_capacity(buttons.len());
    for button in &buttons {
        natives.push(unsafe { alert.addButtonWithTitle(&NSString::from_str(button.label())) });
    }
    // AppKit gives the first added button the Return key equivalent. Reassign every button so an
    // explicit default or cancel index wins, and so no button keeps a stale equivalent.
    for (index, native) in natives.iter().enumerate() {
        let key = if index == default_index {
            "\r"
        } else if Some(index) == cancel_index {
            "\u{1b}"
        } else {
            ""
        };
        unsafe { native.setKeyEquivalent(&NSString::from_str(key)) };
    }
    if let Some(native) = natives.get(default_index) {
        unsafe { alert.window() }.setInitialFirstResponder(Some(native));
    }

    if let Some(checkbox) = &options.checkbox {
        unsafe { alert.setShowsSuppressionButton(true) };
        let Some(button) = (unsafe { alert.suppressionButton() }) else {
            return Err("AppKit refused to create the message-box checkbox".to_owned());
        };
        unsafe {
            button.setTitle(&NSString::from_str(&checkbox.label));
            button.setState(if checkbox.checked {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        }
    }

    let button_count = buttons.len();
    let has_checkbox = options.checkbox.is_some();
    let completion_alert = alert.clone();
    let finish_response = move |response: NSModalResponse| {
        let result = response
            .checked_sub(NSAlertFirstButtonReturn)
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < button_count)
            .map(|button| MessageBoxResponse {
                button,
                checkbox_checked: has_checkbox
                    && suppression_state(&completion_alert) == NSControlStateValueOn,
            })
            .ok_or_else(|| {
                PlatformError::Platform("the native message box closed without an answer".into())
            });
        finish_native_dialog(&context, &responder, result);
    };
    if let Some(parent) = parent {
        let completion = RcBlock::new(finish_response);
        unsafe {
            alert.beginSheetModalForWindow_completionHandler(&parent, Some(&completion));
        }
    } else {
        // NSAlert has no asynchronous application-modal API; its modal session still pumps AppKit
        // events. This branch only runs when the caller owns no parent window.
        let response = unsafe { alert.runModal() };
        finish_response(response);
    }
    Ok(MacPlatformDialog::Prompt(alert))
}

fn suppression_state(alert: &NSAlert) -> objc2_app_kit::NSControlStateValue {
    unsafe { alert.suppressionButton() }
        .as_deref()
        .map_or(NSControlStateValueOff, |button: &NSButton| unsafe {
            button.state()
        })
}
