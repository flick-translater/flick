//! Window creation and visibility helpers.

use tauri::{
    ActivationPolicy, AppHandle, Emitter, LogicalPosition, Manager, TitleBarStyle, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

use super::AppState;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

const MAIN_WINDOW_LABEL: &str = "main";
const TRANSLATE_WINDOW_LABEL: &str = "translate";

pub fn show_main_window(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(ActivationPolicy::Regular);
        let _ = app.show();
        let _ = app.set_dock_visibility(true);
    }

    let window = ensure_main_window(app)?;
    let _ = window.center();
    let _ = window.set_visible_on_all_workspaces(true);
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    let _ = window.set_visible_on_all_workspaces(false);

    Ok(())
}

pub fn ensure_main_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("index.html".into()))
        .title("Flick")
        .devtools(false)
        .inner_size(1240.0, 800.0)
        .min_inner_size(1040.0, 680.0)
        .resizable(true)
        .visible(false)
        .focused(false)
        .center()
        .hidden_title(true)
        .title_bar_style(TitleBarStyle::Overlay)
        .traffic_light_position(LogicalPosition::new(16.0, 18.0))
        .build()
}

pub fn ensure_translate_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window(TRANSLATE_WINDOW_LABEL) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(
        app,
        TRANSLATE_WINDOW_LABEL,
        WebviewUrl::App("translation-window.html".into()),
    )
    .title("Flick Translate")
    .devtools(false)
    .inner_size(480.0, 640.0)
    .min_inner_size(360.0, 480.0)
    .resizable(true)
    .visible(false)
    .focused(false)
    .always_on_top(false)
    .accept_first_mouse(true)
    .transparent(true)
    .decorations(false)
    .shadow(true)
    .build()
}

pub fn show_translate_window(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    remember_previous_frontmost_app(app);

    let window = ensure_translate_window(app)?;
    if !window.is_always_on_top().unwrap_or(false) {
        let _ = window.center();
    }
    let _ = window.set_visible_on_all_workspaces(true);
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    let _ = window.set_visible_on_all_workspaces(false);
    Ok(())
}

pub fn refresh_previous_frontmost_app(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    remember_previous_frontmost_app(app);

    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

pub fn hide_translate_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut suppress) = state.suppress_next_reopen.lock() {
            *suppress = true;
        }
        let _ = state.tts_service.stop();
    }

    if let Some(window) = app.get_webview_window(TRANSLATE_WINDOW_LABEL) {
        window.hide()?;
    }

    #[cfg(target_os = "macos")]
    {
        if should_keep_app_active_after_translate_close(app) {
            return Ok(());
        }

        restore_previous_frontmost_app(app);

        if app.get_webview_window(MAIN_WINDOW_LABEL).is_none() {
            let _ = app.set_activation_policy(ActivationPolicy::Accessory);
            let _ = app.hide();
            return Ok(());
        }

        let _ = app.set_activation_policy(ActivationPolicy::Accessory);
        let _ = app.hide();
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn should_keep_app_active_after_translate_close(app: &AppHandle) -> bool {
    let Some(main_window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return false;
    };

    main_window.is_visible().unwrap_or(false) || main_window.is_focused().unwrap_or(false)
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

pub fn emit_capture_status(app: &AppHandle, event: &str, payload: impl serde::Serialize + Clone) {
    let _ = app.emit(event, payload);
}
