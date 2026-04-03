//! Thin platform facade that routes to OS-specific app and window helpers.

#[path = "platform_linux.rs"]
mod platform_linux;
#[cfg(target_os = "macos")]
#[path = "platform_macos.rs"]
mod platform_macos;
#[path = "platform_window.rs"]
mod platform_window;

use tauri::{App, AppHandle, Manager, RunEvent, Runtime, State, WebviewWindowBuilder};

use crate::{
    app::{AppState, ShortcutAction},
    error::FlickError,
    models::AppSettings,
};

pub fn configure_app_setup(app: &mut App) {
    #[cfg(target_os = "macos")]
    platform_macos::configure_app_setup(app);

    #[cfg(not(target_os = "macos"))]
    platform_linux::configure_app_setup(app);
}

pub fn handle_run_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    #[cfg(target_os = "macos")]
    platform_macos::handle_run_event(app, event);

    #[cfg(not(target_os = "macos"))]
    platform_linux::handle_run_event(app, event);
}

pub fn register_platform_shortcuts(app: &AppHandle) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        return platform_macos::register_platform_shortcuts(app);
    }

    #[cfg(not(target_os = "macos"))]
    {
        platform_linux::register_platform_shortcuts(app)
    }
}

pub fn apply_shortcut_bindings(app: &AppHandle, settings: &AppSettings) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        return platform_macos::apply_shortcut_bindings(app, settings);
    }

    #[cfg(not(target_os = "macos"))]
    {
        platform_linux::apply_shortcut_bindings(app, settings)
    }
}

pub fn trigger_shortcut_action(app: &AppHandle, action: ShortcutAction) {
    #[cfg(target_os = "macos")]
    platform_macos::trigger_shortcut_action(app, action);

    #[cfg(not(target_os = "macos"))]
    platform_linux::trigger_shortcut_action(app, action);
}

pub fn set_shortcut_recording(
    app: &AppHandle,
    state: &State<'_, AppState>,
    recording: bool,
) -> Result<(), FlickError> {
    #[cfg(target_os = "macos")]
    {
        return platform_macos::set_shortcut_recording(app, state, recording);
    }

    #[cfg(not(target_os = "macos"))]
    {
        platform_linux::set_shortcut_recording(app, state, recording)
    }
}

pub fn on_main_window_close(app: &AppHandle) {
    platform_window::on_main_window_close(app);
}

pub fn show_main_window_before_focus(app: &AppHandle) {
    platform_window::show_main_window_before_focus(app);
}

pub fn configure_main_window_builder<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    platform_window::configure_main_window_builder(builder)
}

pub fn show_translate_window_before_focus(app: &AppHandle) {
    platform_window::show_translate_window_before_focus(app);
}

pub fn refresh_previous_frontmost_app(app: &AppHandle) {
    platform_window::refresh_previous_frontmost_app(app);
}

pub fn hide_translate_window_before_hide(app: &AppHandle) {
    platform_window::hide_translate_window_before_hide(app);
}

pub fn hide_translate_window_after_hide(app: &AppHandle) {
    platform_window::hide_translate_window_after_hide(app);
}
