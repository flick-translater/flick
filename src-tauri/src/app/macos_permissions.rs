use std::{thread, time::Duration};

use core_foundation::base::Boolean;
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
};
use rfd::{MessageDialog, MessageLevel};
use tauri::AppHandle;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> Boolean;
    fn CGPreflightScreenCaptureAccess() -> bool;
}

pub fn launch_startup_permission_check(_app: &AppHandle) {
    thread::spawn(move || {
        // Let the main window finish presenting before showing a native warning dialog.
        thread::sleep(Duration::from_millis(600));

        let status = PermissionStatus::detect();
        if status.is_ready() {
            return;
        }

        let mut missing = Vec::new();
        if !status.screen_recording {
            missing.push("屏幕录制");
        }
        if !status.accessibility {
            missing.push("辅助功能");
        }
        if !status.input_monitoring {
            missing.push("输入监控");
        }

        let details = [
            "Flick 缺少截图所需的 macOS 权限。",
            "",
            &format!("缺失权限：{}", missing.join("、")),
            "",
            "请到：系统设置 -> 隐私与安全性",
            "- 屏幕录制：允许 Flick",
            "- 辅助功能：允许 Flick",
            "- 输入监控：允许 Flick",
            "",
            "权限修改后，请彻底退出 Flick 再重新打开。",
        ]
        .join("\n");

        let _ = MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title("Flick 权限未就绪")
            .set_description(&details)
            .show();
    });
}

#[derive(Debug, Clone, Copy)]
struct PermissionStatus {
    screen_recording: bool,
    accessibility: bool,
    input_monitoring: bool,
}

impl PermissionStatus {
    fn detect() -> Self {
        let accessibility = unsafe { AXIsProcessTrusted() != 0 };
        let screen_recording = unsafe { CGPreflightScreenCaptureAccess() };
        let input_monitoring = probe_input_monitoring();

        Self {
            screen_recording,
            accessibility,
            input_monitoring,
        }
    }

    fn is_ready(self) -> bool {
        self.screen_recording && self.accessibility && self.input_monitoring
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
