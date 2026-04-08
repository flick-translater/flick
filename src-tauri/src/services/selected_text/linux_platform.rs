use anyhow::{Context, anyhow};
use arboard::{Clipboard, GetExtLinux, LinuxClipboardKind};
use std::{process::Command, thread, time::Duration};

pub fn read_selected_text() -> anyhow::Result<String> {
    let mut clipboard = Clipboard::new().context("无法访问系统剪贴板")?;

    read_clipboard_text(&mut clipboard, LinuxClipboardKind::Primary).or_else(|primary_error| {
        read_clipboard_text(&mut clipboard, LinuxClipboardKind::Clipboard).map_err(
            |clipboard_error| {
                anyhow!("读取主选区失败: {primary_error}; 读取普通剪贴板也失败: {clipboard_error}")
            },
        )
    })
}

pub fn replace_selected_text(text: &str) -> anyhow::Result<bool> {
    let mut clipboard = Clipboard::new().context("无法访问系统剪贴板")?;
    let previous_text = clipboard.get_text().ok();
    clipboard
        .set_text(text.to_string())
        .context("无法写入剪贴板")?;
    thread::sleep(Duration::from_millis(40));
    trigger_paste_shortcut().context("无法触发系统粘贴快捷键")?;
    thread::sleep(Duration::from_millis(120));

    if let Some(previous_text) = previous_text {
        let _ = clipboard.set_text(previous_text);
    }

    Ok(true)
}

fn read_clipboard_text(
    clipboard: &mut Clipboard,
    kind: LinuxClipboardKind,
) -> anyhow::Result<String> {
    let text = clipboard
        .get()
        .clipboard(kind)
        .text()
        .with_context(|| format!("{}中没有可用文本", clipboard_kind_label(kind)))?;
    let normalized = text.trim().to_string();
    if normalized.is_empty() {
        return Err(anyhow!("{}中的文本为空", clipboard_kind_label(kind)));
    }

    Ok(normalized)
}

fn clipboard_kind_label(kind: LinuxClipboardKind) -> &'static str {
    match kind {
        LinuxClipboardKind::Primary => "主选区",
        LinuxClipboardKind::Clipboard => "普通剪贴板",
        LinuxClipboardKind::Secondary => "次选区",
    }
}

fn trigger_paste_shortcut() -> anyhow::Result<()> {
    let is_wayland = std::env::var("XDG_SESSION_TYPE")
        .map(|value| value.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
        || std::env::var_os("WAYLAND_DISPLAY").is_some();

    if is_wayland {
        if Command::new("wtype").arg("-M").arg("ctrl").arg("-k").arg("v").arg("-m").arg("ctrl").status().is_ok_and(|status| status.success()) {
            return Ok(());
        }

        anyhow::bail!("Wayland 会话下未找到可用的 wtype，无法执行替换");
    }

    if Command::new("xdotool")
        .args(["key", "--clearmodifiers", "ctrl+v"])
        .status()
        .is_ok_and(|status| status.success())
    {
        return Ok(());
    }

    anyhow::bail!("X11 会话下未找到可用的 xdotool，无法执行替换")
}
