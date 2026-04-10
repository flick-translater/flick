//! Thin platform facade that routes to OS-specific app and window helpers.

#[cfg(target_os = "linux")]
#[path = "linux_platform.rs"]
mod linux_platform;
#[cfg(target_os = "macos")]
#[path = "macos_platform.rs"]
mod macos_platform;
#[cfg(target_os = "windows")]
#[path = "windows_platform.rs"]
mod windows_platform;

use tauri::{
    App, AppHandle, Manager, RunEvent, Runtime, State, WebviewWindow, WebviewWindowBuilder,
};

use crate::{
    app::{AppState, ShortcutAction},
    error::FlickError,
    models::AppSettings,
};

pub fn configure_app_setup(app: &mut App) {
    #[cfg(target_os = "macos")]
    macos_platform::configure_app_setup(app);

    #[cfg(target_os = "windows")]
    windows_platform::configure_app_setup(app);

    #[cfg(target_os = "linux")]
    linux_platform::configure_app_setup(app);
}

pub fn handle_run_event(app: &AppHandle, event: &RunEvent) {
    #[cfg(target_os = "macos")]
    macos_platform::handle_run_event(app, event);

    #[cfg(target_os = "windows")]
    windows_platform::handle_run_event(app, event);

    #[cfg(target_os = "linux")]
    linux_platform::handle_run_event(app, event);
}

pub fn register_platform_shortcuts(app: &AppHandle) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        return macos_platform::register_platform_shortcuts(app);
    }

    #[cfg(target_os = "windows")]
    {
        return windows_platform::register_platform_shortcuts(app);
    }

    #[cfg(target_os = "linux")]
    {
        linux_platform::register_platform_shortcuts(app)
    }
}

pub fn apply_shortcut_bindings(app: &AppHandle, settings: &AppSettings) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        return macos_platform::apply_shortcut_bindings(app, settings);
    }

    #[cfg(target_os = "windows")]
    {
        return windows_platform::apply_shortcut_bindings(app, settings);
    }

    #[cfg(target_os = "linux")]
    {
        linux_platform::apply_shortcut_bindings(app, settings)
    }
}

pub fn trigger_shortcut_action(app: &AppHandle, action: ShortcutAction) {
    #[cfg(target_os = "macos")]
    macos_platform::trigger_shortcut_action(app, action);

    #[cfg(target_os = "windows")]
    windows_platform::trigger_shortcut_action(app, action);

    #[cfg(target_os = "linux")]
    linux_platform::trigger_shortcut_action(app, action);
}

pub fn set_shortcut_recording(
    app: &AppHandle,
    state: &State<'_, AppState>,
    recording: bool,
) -> Result<(), FlickError> {
    #[cfg(target_os = "macos")]
    {
        return macos_platform::set_shortcut_recording(app, state, recording);
    }

    #[cfg(target_os = "windows")]
    {
        return windows_platform::set_shortcut_recording(app, state, recording);
    }

    #[cfg(target_os = "linux")]
    {
        linux_platform::set_shortcut_recording(app, state, recording)
    }
}

pub fn on_main_window_close(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos_platform::on_main_window_close(app);

    #[cfg(target_os = "windows")]
    windows_platform::on_main_window_close(app);

    #[cfg(target_os = "linux")]
    linux_platform::on_main_window_close(app);
}

pub fn show_main_window_before_focus(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos_platform::show_main_window_before_focus(app);

    #[cfg(target_os = "windows")]
    windows_platform::show_main_window_before_focus(app);

    #[cfg(target_os = "linux")]
    linux_platform::show_main_window_before_focus(app);
}

pub fn configure_main_window_builder<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    #[cfg(target_os = "macos")]
    {
        return macos_platform::configure_main_window_builder(builder);
    }

    #[cfg(target_os = "windows")]
    {
        return windows_platform::configure_main_window_builder(builder);
    }

    #[cfg(target_os = "linux")]
    {
        linux_platform::configure_main_window_builder(builder)
    }
}

pub fn show_translate_window_before_focus(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos_platform::show_translate_window_before_focus(app);

    #[cfg(target_os = "windows")]
    windows_platform::show_translate_window_before_focus(app);

    #[cfg(target_os = "linux")]
    linux_platform::show_translate_window_before_focus(app);
}

pub fn show_translate_window_after_show(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos_platform::show_translate_window_after_show(app);

    #[cfg(target_os = "windows")]
    windows_platform::show_translate_window_after_show(app);

    #[cfg(target_os = "linux")]
    linux_platform::show_translate_window_after_show(app);
}

pub fn configure_built_window(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    windows_platform::configure_built_window(window);

    #[cfg(target_os = "macos")]
    let _ = window;

    #[cfg(target_os = "linux")]
    let _ = window;
}

pub fn refresh_previous_frontmost_app(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos_platform::refresh_previous_frontmost_app(app);

    #[cfg(target_os = "windows")]
    windows_platform::refresh_previous_frontmost_app(app);

    #[cfg(target_os = "linux")]
    linux_platform::refresh_previous_frontmost_app(app);
}

pub fn hide_translate_window_before_hide(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos_platform::hide_translate_window_before_hide(app);

    #[cfg(target_os = "windows")]
    windows_platform::hide_translate_window_before_hide(app);

    #[cfg(target_os = "linux")]
    linux_platform::hide_translate_window_before_hide(app);
}

pub fn hide_translate_window_after_hide(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos_platform::hide_translate_window_after_hide(app);

    #[cfg(target_os = "windows")]
    windows_platform::hide_translate_window_after_hide(app);

    #[cfg(target_os = "linux")]
    linux_platform::hide_translate_window_after_hide(app);
}

pub fn translate_window_pinning_supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        true
    }

    #[cfg(target_os = "windows")]
    {
        true
    }

    #[cfg(target_os = "linux")]
    {
        linux_platform::translate_window_pinning_supported()
    }
}
