use std::{thread, time::Duration};

use core_foundation::{
    base::{Boolean, TCFType},
    boolean::CFBoolean,
    dictionary::{CFDictionary, CFDictionaryRef},
    string::{CFString, CFStringRef},
};
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> Boolean;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> Boolean;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
    fn CGPreflightPostEventAccess() -> bool;
    fn CGRequestPostEventAccess() -> bool;
}

pub fn current_permission_status() -> PermissionStatus {
    PermissionStatus::detect()
}

pub fn request_startup_permissions() -> PermissionStatus {
    let status = PermissionStatus::detect();
    if status.is_ready() {
        return status;
    }

    if !status.screen_recording {
        let _ = unsafe { CGRequestScreenCaptureAccess() };
        thread::sleep(Duration::from_millis(200));
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
