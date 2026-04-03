#[cfg(target_os = "linux")]
mod platform_linux;
#[cfg(target_os = "macos")]
mod platform_macos;
#[cfg(target_os = "windows")]
mod platform_windows;

#[cfg(target_os = "linux")]
pub fn read_selected_text() -> anyhow::Result<String> {
    platform_linux::read_selected_text()
}

#[cfg(target_os = "macos")]
pub fn read_selected_text() -> anyhow::Result<String> {
    platform_macos::read_selected_text()
}

#[cfg(target_os = "windows")]
pub fn read_selected_text() -> anyhow::Result<String> {
    platform_windows::read_selected_text()
}
