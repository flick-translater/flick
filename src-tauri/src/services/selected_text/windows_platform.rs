use std::{mem::size_of, thread, time::Duration};

use anyhow::{Context, anyhow};
use arboard::Clipboard;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_CONTROL,
};

const COPY_KEY: VIRTUAL_KEY = b'C' as VIRTUAL_KEY;

pub fn read_selected_text() -> anyhow::Result<String> {
    let mut clipboard = Clipboard::new().context("无法访问剪贴板")?;
    let previous_text = clipboard.get_text().ok();

    simulate_copy_shortcut().context("无法触发系统复制快捷键")?;
    thread::sleep(Duration::from_millis(150));

    let text = clipboard.get_text().context("剪贴板中没有文本内容")?;
    let normalized = sanitize_selected_text(&text);

    if let Some(previous_text) = previous_text {
        let _ = clipboard.set_text(previous_text);
    }

    if normalized.is_empty() {
        return Err(anyhow!("当前没有可翻译的选中文本"));
    }

    Ok(normalized)
}

fn sanitize_selected_text(text: &str) -> String {
    text.chars()
        .filter(|ch| *ch == '\n' || *ch == '\r' || *ch == '\t' || !ch.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

fn simulate_copy_shortcut() -> anyhow::Result<()> {
    let inputs = [
        keyboard_input(VK_CONTROL, false),
        keyboard_input(COPY_KEY, false),
        keyboard_input(COPY_KEY, true),
        keyboard_input(VK_CONTROL, true),
    ];

    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        return Err(anyhow!("发送复制快捷键失败"));
    }

    Ok(())
}

fn keyboard_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if key_up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
