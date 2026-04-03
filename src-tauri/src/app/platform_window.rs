use tauri::{AppHandle, Manager, Runtime, WebviewWindowBuilder};

#[cfg(target_os = "macos")]
use tauri::{ActivationPolicy, LogicalPosition, TitleBarStyle};

#[cfg(target_os = "macos")]
use crate::app::AppState;

#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

pub fn on_main_window_close(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.hide();
        let _ = app.set_activation_policy(ActivationPolicy::Accessory);
    }

    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

pub fn show_main_window_before_focus(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(ActivationPolicy::Regular);
        let _ = app.show();
        let _ = app.set_dock_visibility(true);
    }

    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

pub fn configure_main_window_builder<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    #[cfg(target_os = "macos")]
    {
        return builder
            .hidden_title(true)
            .title_bar_style(TitleBarStyle::Overlay)
            .traffic_light_position(LogicalPosition::new(16.0, 18.0));
    }

    #[cfg(not(target_os = "macos"))]
    {
        builder
    }
}

pub fn show_translate_window_before_focus(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    remember_previous_frontmost_app(app);

    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

pub fn refresh_previous_frontmost_app(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    remember_previous_frontmost_app(app);

    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

pub fn hide_translate_window_before_hide(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        if let Some(state) = app.try_state::<AppState>() {
            if let Ok(mut suppress) = state.suppress_next_reopen.lock() {
                *suppress = true;
            }
        }
        restore_previous_frontmost_app(app);
    }

    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

pub fn hide_translate_window_after_hide(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        if let Some(main_window) = app.get_webview_window("main") {
            let is_visible = main_window.is_visible().unwrap_or(false);
            if !is_visible {
                let _ = app.set_activation_policy(ActivationPolicy::Accessory);
                let _ = app.hide();
            }
        } else {
            let _ = app.set_activation_policy(ActivationPolicy::Accessory);
            let _ = app.hide();
        }
    }

    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

#[cfg(target_os = "macos")]
fn remember_previous_frontmost_app(app: &AppHandle) {
    let workspace = NSWorkspace::sharedWorkspace();
    let current_app = NSRunningApplication::currentApplication();
    let current_pid = current_app.processIdentifier();
    let previous_pid = workspace
        .frontmostApplication()
        .map(|frontmost| frontmost.processIdentifier())
        .filter(|pid| *pid > 0 && *pid != current_pid);

    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut stored_pid) = state.previous_frontmost_app_pid.lock() {
            *stored_pid = previous_pid;
        }
    }
}

#[cfg(target_os = "macos")]
fn restore_previous_frontmost_app(app: &AppHandle) {
    let previous_pid = app.try_state::<AppState>().and_then(|state| {
        state
            .previous_frontmost_app_pid
            .lock()
            .ok()
            .and_then(|mut stored_pid| stored_pid.take())
    });

    let Some(previous_pid) = previous_pid else {
        return;
    };

    let current_app = NSRunningApplication::currentApplication();
    let current_pid = current_app.processIdentifier();
    let frontmost_pid = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|frontmost| frontmost.processIdentifier());

    if frontmost_pid.is_some_and(|pid| pid > 0 && pid != current_pid) {
        return;
    }

    if let Some(previous_app) =
        NSRunningApplication::runningApplicationWithProcessIdentifier(previous_pid)
    {
        let _ = previous_app.activateWithOptions(NSApplicationActivationOptions(0));
    }
}
