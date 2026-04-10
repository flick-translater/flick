use std::{ffi::c_void, thread, time::Duration};

use anyhow::{Context, anyhow};
use arboard::Clipboard;
use core_foundation::{
    base::{CFRelease, CFTypeRef, TCFType},
    boolean::CFBoolean,
    string::{CFString, CFStringRef},
};
use core_graphics::{
    event::{CGEvent, CGEventFlags, CGEventTapLocation},
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

pub fn replace_selected_text(text: &str) -> anyhow::Result<bool> {
    match focused_ui_element() {
        Ok(focused) => {
            let is_editable = is_focused_element_editable(focused.cast()).unwrap_or(true);
            unsafe {
                CFRelease(focused);
            }

            if !is_editable {
                return Ok(false);
            }
        }
        Err(_error) => {
            // Some apps transiently stop exposing a focused accessibility element after the
            // global shortcut fires. Fall back to paste-based replacement in that case.
        }
    }

    replace_selected_text_via_paste(text)?;
    Ok(true)
}

fn read_selected_text_via_accessibility() -> anyhow::Result<String> {
    let focused = focused_ui_element().context("无法读取当前焦点元素")?;
    let result = (|| -> anyhow::Result<String> {
        let selected = copy_attribute_value(focused.cast(), "AXSelectedText")
            .context("无法读取当前选中文本")?;

        let text = unsafe {
            let selected_text = CFString::wrap_under_create_rule(selected.cast());
            selected_text.to_string()
        };

        let normalized = text.trim().to_string();
        if normalized.is_empty() {
            return Err(anyhow!("当前没有可翻译的选中文本"));
        }

        Ok(normalized)
    })();

    unsafe {
        CFRelease(focused);
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
    simulate_shortcut(0x08, CGEventFlags::CGEventFlagCommand)
}

fn replace_selected_text_via_paste(text: &str) -> anyhow::Result<()> {
    let mut clipboard = Clipboard::new().context("无法访问剪贴板")?;
    let previous_text = clipboard.get_text().ok();
    clipboard
        .set_text(text.to_string())
        .context("无法写入剪贴板")?;
    thread::sleep(Duration::from_millis(40));
    simulate_shortcut(0x09, CGEventFlags::CGEventFlagCommand).context("无法触发系统粘贴快捷键")?;
    thread::sleep(Duration::from_millis(120));

    if let Some(previous_text) = previous_text {
        let _ = clipboard.set_text(previous_text);
    }

    Ok(())
}

fn simulate_shortcut(key_code: u16, modifiers: CGEventFlags) -> anyhow::Result<()> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| anyhow!("无法创建键盘事件源"))?;
    let key_down = CGEvent::new_keyboard_event(source.clone(), key_code, true)
        .map_err(|_| anyhow!("无法创建快捷键按下事件"))?;
    key_down.set_flags(modifiers);

    let key_up = CGEvent::new_keyboard_event(source, key_code, false)
        .map_err(|_| anyhow!("无法创建快捷键抬起事件"))?;
    key_up.set_flags(modifiers);

    key_down.post(CGEventTapLocation::HID);
    key_up.post(CGEventTapLocation::HID);
    Ok(())
}

fn focused_ui_element() -> anyhow::Result<AXUIElementRef> {
    let system_wide = unsafe { AXUIElementCreateSystemWide() };
    if system_wide.is_null() {
        return Err(anyhow!("无法创建系统辅助功能对象"));
    }

    let focused = copy_attribute_value(system_wide, "AXFocusedUIElement");
    unsafe {
        CFRelease(system_wide.cast());
    }
    focused.map(|value| value.cast())
}

fn is_focused_element_editable(element: AXUIElementRef) -> anyhow::Result<bool> {
    let editable =
        copy_attribute_value(element, "AXEditable").context("无法判断当前选区是否可编辑")?;
    let result = unsafe {
        let editable_ref = editable.cast::<std::ffi::c_void>();
        CFBoolean::wrap_under_get_rule(editable_ref.cast()).into()
    };
    unsafe {
        CFRelease(editable);
    }
    Ok(result)
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
