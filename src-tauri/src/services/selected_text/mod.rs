#[cfg(target_os = "linux")]
mod linux_platform;
#[cfg(target_os = "macos")]
mod macos_platform;
#[cfg(target_os = "windows")]
mod windows_platform;

#[cfg(target_os = "linux")]
pub fn read_selected_text() -> anyhow::Result<String> {
    linux_platform::read_selected_text()
}

#[cfg(target_os = "macos")]
pub fn read_selected_text() -> anyhow::Result<String> {
    macos_platform::read_selected_text()
}

#[cfg(target_os = "windows")]
pub fn read_selected_text() -> anyhow::Result<String> {
    windows_platform::read_selected_text()
}

#[cfg(target_os = "linux")]
pub fn replace_selected_text(text: &str) -> anyhow::Result<bool> {
    linux_platform::replace_selected_text(text)
}

#[cfg(target_os = "macos")]
pub fn replace_selected_text(text: &str) -> anyhow::Result<bool> {
    macos_platform::replace_selected_text(text)
}

#[cfg(target_os = "windows")]
pub fn replace_selected_text(text: &str) -> anyhow::Result<bool> {
    windows_platform::replace_selected_text(text)
}
