use std::{ffi::c_void, thread, time::Duration};

use anyhow::{Context, anyhow};
use arboard::Clipboard;
use core_foundation::{
    base::{CFRelease, CFTypeRef, TCFType},
    string::{CFString, CFStringRef},
};
use core_graphics::{
    event::{CGEvent, CGEventFlags, CGEventTapLocation, KeyCode},
    event_source::{CGEventSource, CGEventSourceStateID},
};

type AXUIElementRef = *const c_void;
type AXError = i32;

const AX_ERROR_SUCCESS: AXError = 0;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
}

pub fn read_selected_text() -> anyhow::Result<String> {
    read_selected_text_via_accessibility().or_else(|ax_error| {
        read_selected_text_via_copy_shortcut().map_err(|copy_error| {
            anyhow!("辅助功能读取失败: {ax_error}; 剪贴板回退也失败: {copy_error}")
        })
    })
}

fn read_selected_text_via_accessibility() -> anyhow::Result<String> {
    let system_wide = unsafe { AXUIElementCreateSystemWide() };
    if system_wide.is_null() {
        return Err(anyhow!("无法创建系统辅助功能对象"));
    }

    let result = (|| -> anyhow::Result<String> {
        let focused = copy_attribute_value(system_wide, "AXFocusedUIElement")
            .context("无法读取当前焦点元素")?;
        let selected = copy_attribute_value(focused.cast(), "AXSelectedText")
            .context("无法读取当前选中文本")?;

        let text = unsafe {
            let selected_text = CFString::wrap_under_create_rule(selected.cast());
            selected_text.to_string()
        };

        unsafe {
            CFRelease(focused);
        }

        let normalized = text.trim().to_string();
        if normalized.is_empty() {
            return Err(anyhow!("当前没有可翻译的选中文本"));
        }

        Ok(normalized)
    })();

    unsafe {
        CFRelease(system_wide.cast());
    }

    result
}

fn read_selected_text_via_copy_shortcut() -> anyhow::Result<String> {
    simulate_copy_shortcut().context("无法触发系统复制快捷键")?;
    thread::sleep(Duration::from_millis(140));

    let mut clipboard = Clipboard::new().context("无法访问剪贴板")?;
    let text = clipboard.get_text().context("剪贴板中没有文本内容")?;
    let normalized = text.trim().to_string();
    if normalized.is_empty() {
        return Err(anyhow!("剪贴板文本为空"));
    }

    Ok(normalized)
}

fn simulate_copy_shortcut() -> anyhow::Result<()> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| anyhow!("无法创建键盘事件源"))?;
    let key_down = CGEvent::new_keyboard_event(source.clone(), KeyCode::ANSI_C, true)
        .map_err(|_| anyhow!("无法创建复制按下事件"))?;
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);

    let key_up = CGEvent::new_keyboard_event(source, KeyCode::ANSI_C, false)
        .map_err(|_| anyhow!("无法创建复制抬起事件"))?;
    key_up.set_flags(CGEventFlags::CGEventFlagCommand);

    key_down.post(CGEventTapLocation::HID);
    key_up.post(CGEventTapLocation::HID);
    Ok(())
}

fn copy_attribute_value(element: AXUIElementRef, attribute: &str) -> anyhow::Result<CFTypeRef> {
    let attribute = CFString::new(attribute);
    let mut value: CFTypeRef = std::ptr::null_mut();
    let error = unsafe {
        AXUIElementCopyAttributeValue(
            element,
            attribute.as_concrete_TypeRef(),
            &mut value as *mut CFTypeRef,
        )
    };

    if error != AX_ERROR_SUCCESS {
        return Err(anyhow!("辅助功能接口返回错误代码 {error}"));
    }

    if value.is_null() {
        return Err(anyhow!("辅助功能接口未返回内容"));
    }

    Ok(value)
}
