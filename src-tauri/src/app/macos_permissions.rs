use core_foundation::base::Boolean;
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> Boolean;
}

pub fn current_permission_status() -> PermissionStatus {
    PermissionStatus::detect()
}

#[derive(Debug, Clone, Copy)]
pub struct PermissionStatus {
    pub accessibility: bool,
    pub input_monitoring: bool,
}

impl PermissionStatus {
    fn detect() -> Self {
        let accessibility = unsafe { AXIsProcessTrusted() != 0 };
        let input_monitoring = probe_input_monitoring();

        Self {
            accessibility,
            input_monitoring,
        }
    }

    pub fn hotkeys_ready(self) -> bool {
        self.accessibility && self.input_monitoring
    }
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
