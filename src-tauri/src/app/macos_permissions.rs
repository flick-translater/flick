use std::{process::Command, thread, time::Duration};

use core_foundation::{
    base::{Boolean, TCFType},
    boolean::CFBoolean,
    dictionary::{CFDictionary, CFDictionaryRef},
    string::{CFString, CFStringRef},
};
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
};
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use tauri::{ActivationPolicy, AppHandle, Manager};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> Boolean;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> Boolean;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
    fn CGPreflightPostEventAccess() -> bool;
    fn CGRequestPostEventAccess() -> bool;
}

pub fn current_permission_status() -> PermissionStatus {
    PermissionStatus::detect()
}

pub fn ensure_screen_recording_permission(app: &AppHandle) -> bool {
    if unsafe { CGPreflightScreenCaptureAccess() } {
        return true;
    }

    show_screen_recording_permission_prompt(app);
    false
}

pub fn request_startup_permissions() -> PermissionStatus {
    let status = PermissionStatus::detect();
    if status.is_ready() {
        return status;
    }

    if !status.accessibility {
        let _ = request_accessibility_permission();
        thread::sleep(Duration::from_millis(200));
    }

    if !status.input_monitoring {
        let _ = unsafe { CGRequestListenEventAccess() };
        thread::sleep(Duration::from_millis(200));
    }

    if !status.event_posting {
        let _ = unsafe { CGRequestPostEventAccess() };
    }

    PermissionStatus::detect()
}

#[derive(Debug, Clone, Copy)]
pub struct PermissionStatus {
    pub screen_recording: bool,
    pub accessibility: bool,
    pub input_monitoring: bool,
    pub event_posting: bool,
}

impl PermissionStatus {
    fn detect() -> Self {
        let screen_recording = unsafe { CGPreflightScreenCaptureAccess() };
        let accessibility = unsafe { AXIsProcessTrusted() != 0 };
        let input_monitoring =
            unsafe { CGPreflightListenEventAccess() } || probe_input_monitoring();
        let event_posting = unsafe { CGPreflightPostEventAccess() };

        Self {
            screen_recording,
            accessibility,
            input_monitoring,
            event_posting,
        }
    }

    pub fn is_ready(self) -> bool {
        self.screen_recording && self.accessibility && self.input_monitoring && self.event_posting
    }

    pub fn hotkeys_ready(self) -> bool {
        self.accessibility && self.input_monitoring
    }
}

fn request_accessibility_permission() -> bool {
    let option_key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let options: CFDictionary<CFString, CFBoolean> =
        CFDictionary::from_CFType_pairs(&[(option_key, CFBoolean::true_value())]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) != 0 }
}

fn show_screen_recording_permission_prompt(app: &AppHandle) {
    let should_restore_accessory = app
        .get_webview_window("main")
        .map(|window| !window.is_visible().unwrap_or(false))
        .unwrap_or(true);

    let _ = app.set_activation_policy(ActivationPolicy::Regular);
    let _ = app.show();
    let _ = app.set_dock_visibility(true);

    let result = MessageDialog::new()
        .set_level(MessageLevel::Warning)
        .set_title("需要录屏权限")
        .set_description(
            "Flick 需要录屏权限才能截图。请在系统设置中为 Flick 开启“屏幕与系统音频录制”权限，然后重新开始截图。",
        )
        .set_buttons(MessageButtons::OkCancelCustom(
            "前往设置".into(),
            "拒绝".into(),
        ))
        .show();

    if matches!(result, MessageDialogResult::Custom(label) if label == "前往设置") {
        open_screen_recording_settings();
    }

    if should_restore_accessory {
        let _ = app.hide();
        let _ = app.set_dock_visibility(false);
        let _ = app.set_activation_policy(ActivationPolicy::Accessory);
    }
}

fn open_screen_recording_settings() {
    let _ = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        .spawn();
}

fn probe_input_monitoring() -> bool {
    let tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![CGEventType::MouseMoved],
        |_proxy, _event_type, _event| core_graphics::event::CallbackResult::Keep,
    );

    tap.is_ok()
}
