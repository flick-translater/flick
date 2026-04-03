//! Thin platform facade that routes to OS-specific app and window helpers.

#[cfg(target_os = "linux")]
#[path = "platform_linux.rs"]
mod platform_linux;
#[cfg(target_os = "macos")]
#[path = "platform_macos.rs"]
mod platform_macos;
#[cfg(target_os = "windows")]
#[path = "platform_windows.rs"]
mod platform_windows;

use tauri::{App, AppHandle, Manager, RunEvent, Runtime, State, WebviewWindowBuilder};

use crate::{
    app::{AppState, ShortcutAction},
    error::FlickError,
    models::AppSettings,
};

pub fn configure_app_setup(app: &mut App) {
    #[cfg(target_os = "macos")]
    platform_macos::configure_app_setup(app);

    #[cfg(target_os = "windows")]
    platform_windows::configure_app_setup(app);

    #[cfg(target_os = "linux")]
    platform_linux::configure_app_setup(app);
}

pub fn handle_run_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    #[cfg(target_os = "macos")]
    platform_macos::handle_run_event(app, event);

    #[cfg(target_os = "windows")]
    platform_windows::handle_run_event(app, event);

    #[cfg(target_os = "linux")]
    platform_linux::handle_run_event(app, event);
}

pub fn register_platform_shortcuts(app: &AppHandle) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        return platform_macos::register_platform_shortcuts(app);
    }

    #[cfg(target_os = "windows")]
    {
        return platform_windows::register_platform_shortcuts(app);
    }

    #[cfg(target_os = "linux")]
    {
        platform_linux::register_platform_shortcuts(app)
    }
}

pub fn apply_shortcut_bindings(app: &AppHandle, settings: &AppSettings) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        return platform_macos::apply_shortcut_bindings(app, settings);
    }

    #[cfg(target_os = "windows")]
    {
        return platform_windows::apply_shortcut_bindings(app, settings);
    }

    #[cfg(target_os = "linux")]
    {
        platform_linux::apply_shortcut_bindings(app, settings)
    }
}

pub fn trigger_shortcut_action(app: &AppHandle, action: ShortcutAction) {
    #[cfg(target_os = "macos")]
    platform_macos::trigger_shortcut_action(app, action);

    #[cfg(target_os = "windows")]
    platform_windows::trigger_shortcut_action(app, action);

    #[cfg(target_os = "linux")]
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

    #[cfg(target_os = "windows")]
    {
        return platform_windows::set_shortcut_recording(app, state, recording);
    }

    #[cfg(target_os = "linux")]
    {
        platform_linux::set_shortcut_recording(app, state, recording)
    }
}

pub fn on_main_window_close(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    platform_macos::on_main_window_close(app);

    #[cfg(target_os = "windows")]
    platform_windows::on_main_window_close(app);

    #[cfg(target_os = "linux")]
    platform_linux::on_main_window_close(app);
}

pub fn show_main_window_before_focus(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    platform_macos::show_main_window_before_focus(app);

    #[cfg(target_os = "windows")]
    platform_windows::show_main_window_before_focus(app);

    #[cfg(target_os = "linux")]
    platform_linux::show_main_window_before_focus(app);
}

pub fn configure_main_window_builder<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    #[cfg(target_os = "macos")]
    {
        return platform_macos::configure_main_window_builder(builder);
    }

    #[cfg(target_os = "windows")]
    {
        return platform_windows::configure_main_window_builder(builder);
    }

    #[cfg(target_os = "linux")]
    {
        platform_linux::configure_main_window_builder(builder)
    }
}

pub fn show_translate_window_before_focus(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    platform_macos::show_translate_window_before_focus(app);

    #[cfg(target_os = "windows")]
    platform_windows::show_translate_window_before_focus(app);

    #[cfg(target_os = "linux")]
    platform_linux::show_translate_window_before_focus(app);
}

pub fn refresh_previous_frontmost_app(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    platform_macos::refresh_previous_frontmost_app(app);

    #[cfg(target_os = "windows")]
    platform_windows::refresh_previous_frontmost_app(app);

    #[cfg(target_os = "linux")]
    platform_linux::refresh_previous_frontmost_app(app);
}

pub fn hide_translate_window_before_hide(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    platform_macos::hide_translate_window_before_hide(app);

    #[cfg(target_os = "windows")]
    platform_windows::hide_translate_window_before_hide(app);

    #[cfg(target_os = "linux")]
    platform_linux::hide_translate_window_before_hide(app);
}

pub fn hide_translate_window_after_hide(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    platform_macos::hide_translate_window_after_hide(app);

    #[cfg(target_os = "windows")]
    platform_windows::hide_translate_window_after_hide(app);

    #[cfg(target_os = "linux")]
    platform_linux::hide_translate_window_after_hide(app);
}
