//! Linux capture-session facade that chooses X11 or Wayland behavior at runtime.

#[path = "linux_wayland_platform.rs"]
mod linux_wayland_platform;
#[path = "linux_x11_platform.rs"]
mod linux_x11_platform;

use tauri::{AppHandle, State};

use crate::{app::AppState, error::FlickError, services::CachedScreenCapture};

pub fn begin_interactive_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    match current_desktop_backend() {
        LinuxDesktopBackend::Wayland => {
            linux_wayland_platform::begin_interactive_capture_session(app, state)
        }
        LinuxDesktopBackend::X11 => {
            linux_x11_platform::begin_interactive_capture_session(app, state)
        }
    }
}

pub fn cancel_interactive_capture_session(app: &AppHandle, state: &State<'_, AppState>) {
    match current_desktop_backend() {
        LinuxDesktopBackend::Wayland => {
            linux_wayland_platform::cancel_interactive_capture_session(app, state)
        }
        LinuxDesktopBackend::X11 => {
            linux_x11_platform::cancel_interactive_capture_session(app, state)
        }
    }
}

pub fn prepare_for_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    match current_desktop_backend() {
        LinuxDesktopBackend::Wayland => {
            linux_wayland_platform::prepare_for_capture_session(app, state)
        }
        LinuxDesktopBackend::X11 => linux_x11_platform::prepare_for_capture_session(app, state),
    }
}

pub fn complete_ui_before_capture_processing(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<Vec<CachedScreenCapture>, FlickError> {
    match current_desktop_backend() {
        LinuxDesktopBackend::Wayland => {
            linux_wayland_platform::complete_ui_before_capture_processing(app, state)
        }
        LinuxDesktopBackend::X11 => {
            linux_x11_platform::complete_ui_before_capture_processing(app, state)
        }
    }
}

pub fn finalize_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
    restore_previous_frontmost: bool,
) {
    match current_desktop_backend() {
        LinuxDesktopBackend::Wayland => {
            linux_wayland_platform::finalize_capture_session(app, state, restore_previous_frontmost)
        }
        LinuxDesktopBackend::X11 => {
            linux_x11_platform::finalize_capture_session(app, state, restore_previous_frontmost)
        }
    }
}

pub fn restore_after_failed_capture(
    app: &AppHandle,
    state: &State<'_, AppState>,
    restore_previous_frontmost: bool,
) {
    match current_desktop_backend() {
        LinuxDesktopBackend::Wayland => linux_wayland_platform::restore_after_failed_capture(
            app,
            state,
            restore_previous_frontmost,
        ),
        LinuxDesktopBackend::X11 => {
            linux_x11_platform::restore_after_failed_capture(app, state, restore_previous_frontmost)
        }
    }
}

pub fn cleanup_after_cancel(app: &AppHandle, state: &State<'_, AppState>) {
    match current_desktop_backend() {
        LinuxDesktopBackend::Wayland => linux_wayland_platform::cleanup_after_cancel(app, state),
        LinuxDesktopBackend::X11 => linux_x11_platform::cleanup_after_cancel(app, state),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinuxDesktopBackend {
    Wayland,
    X11,
}

pub(crate) fn current_desktop_backend() -> LinuxDesktopBackend {
    let session_type = std::env::var("XDG_SESSION_TYPE")
        .ok()
        .map(|value| value.to_ascii_lowercase());
    if matches!(session_type.as_deref(), Some("wayland")) {
        return LinuxDesktopBackend::Wayland;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return LinuxDesktopBackend::Wayland;
    }
    LinuxDesktopBackend::X11
}
