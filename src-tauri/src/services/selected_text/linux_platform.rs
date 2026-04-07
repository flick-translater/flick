use anyhow::{Context, anyhow};
use arboard::{Clipboard, GetExtLinux, LinuxClipboardKind};

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
