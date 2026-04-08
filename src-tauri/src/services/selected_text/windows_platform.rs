use std::{mem::size_of, thread, time::Duration};

use anyhow::{Context, anyhow};
use arboard::Clipboard;
use windows_sys::Win32::{
    System::DataExchange::GetClipboardSequenceNumber,
    UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
        VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    },
};

const COPY_KEY: VIRTUAL_KEY = b'C' as VIRTUAL_KEY;
const PASTE_KEY: VIRTUAL_KEY = b'V' as VIRTUAL_KEY;
const CLIPBOARD_POLL_INTERVAL: Duration = Duration::from_millis(25);
const CLIPBOARD_POLL_ATTEMPTS: usize = 12;
const MODIFIER_RELEASE_POLL_INTERVAL: Duration = Duration::from_millis(20);
const MODIFIER_RELEASE_ATTEMPTS: usize = 15;

pub fn read_selected_text() -> anyhow::Result<String> {
    let mut clipboard = Clipboard::new().context("failed to access clipboard")?;
    let previous_text = clipboard.get_text().ok();
    let previous_sequence = unsafe { GetClipboardSequenceNumber() };

    wait_for_modifier_keys_release();
    simulate_copy_shortcut().context("failed to trigger system copy shortcut")?;

    let text = wait_for_copied_text(&mut clipboard, previous_sequence, previous_text.as_deref())
        .context("clipboard does not contain newly copied text")?;
    let normalized = sanitize_selected_text(&text);

    if let Some(previous_text) = previous_text {
        let _ = clipboard.set_text(previous_text);
    }

    if normalized.is_empty() {
        return Err(anyhow!("there is no translatable selected text"));
    }

    Ok(normalized)
}

pub fn replace_selected_text(text: &str) -> anyhow::Result<bool> {
    let mut clipboard = Clipboard::new().context("failed to access clipboard")?;
    let previous_text = clipboard.get_text().ok();

    wait_for_modifier_keys_release();
    clipboard
        .set_text(text.to_string())
        .context("failed to write translated text to clipboard")?;
    thread::sleep(Duration::from_millis(40));
    simulate_paste_shortcut().context("failed to trigger system paste shortcut")?;
    thread::sleep(Duration::from_millis(120));

    if let Some(previous_text) = previous_text {
        let _ = clipboard.set_text(previous_text);
    }

    Ok(true)
}

fn wait_for_copied_text(
    clipboard: &mut Clipboard,
    previous_sequence: u32,
    previous_text: Option<&str>,
) -> anyhow::Result<String> {
    for _ in 0..CLIPBOARD_POLL_ATTEMPTS {
        thread::sleep(CLIPBOARD_POLL_INTERVAL);

        let current_sequence = unsafe { GetClipboardSequenceNumber() };
        let text = match clipboard.get_text() {
            Ok(text) => text,
            Err(_) => continue,
        };

        if current_sequence != previous_sequence {
            return Ok(text);
        }

        if previous_text.is_none_or(|previous| previous != text) {
            return Ok(text);
        }
    }

    Err(anyhow!("there is no translatable selected text"))
}

fn sanitize_selected_text(text: &str) -> String {
    text.chars()
        .filter(|ch| *ch == '\n' || *ch == '\r' || *ch == '\t' || !ch.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

fn wait_for_modifier_keys_release() {
    for _ in 0..MODIFIER_RELEASE_ATTEMPTS {
        if !modifier_key_pressed(VK_CONTROL)
            && !modifier_key_pressed(VK_MENU)
            && !modifier_key_pressed(VK_SHIFT)
            && !modifier_key_pressed(VK_LWIN)
            && !modifier_key_pressed(VK_RWIN)
        {
            return;
        }

        thread::sleep(MODIFIER_RELEASE_POLL_INTERVAL);
    }
}

fn modifier_key_pressed(vk: VIRTUAL_KEY) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

fn simulate_copy_shortcut() -> anyhow::Result<()> {
    simulate_control_shortcut(COPY_KEY)
}

fn simulate_paste_shortcut() -> anyhow::Result<()> {
    simulate_control_shortcut(PASTE_KEY)
}

fn simulate_control_shortcut(key: VIRTUAL_KEY) -> anyhow::Result<()> {
    let inputs = [
        keyboard_input(VK_CONTROL, false),
        keyboard_input(key, false),
        keyboard_input(key, true),
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
        return Err(anyhow!("failed to send copy shortcut"));
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
