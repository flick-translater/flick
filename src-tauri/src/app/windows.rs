//! Window creation and visibility helpers.

use tauri::{
    ActivationPolicy, AppHandle, Emitter, LogicalPosition, Manager, TitleBarStyle, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

use super::AppState;

const MAIN_WINDOW_LABEL: &str = "main";
const WIDGET_WINDOW_LABEL: &str = "widget";

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

pub fn ensure_widget_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window(WIDGET_WINDOW_LABEL) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(
        app,
        WIDGET_WINDOW_LABEL,
        WebviewUrl::App("translation-window.html".into()),
    )
    .title("Flick Widget")
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

pub fn show_widget_window(app: &AppHandle) -> tauri::Result<()> {
    let window = ensure_widget_window(app)?;
    let _ = window.center();
    let _ = window.set_visible_on_all_workspaces(true);
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    let _ = window.set_visible_on_all_workspaces(false);
    Ok(())
}

pub fn hide_widget_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut suppress) = state.suppress_next_reopen.lock() {
            *suppress = true;
        }
    }

    if let Some(window) = app.get_webview_window(WIDGET_WINDOW_LABEL) {
        window.hide()?;
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(main_window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
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

    Ok(())
}

pub fn emit_capture_status(app: &AppHandle, event: &str, payload: impl serde::Serialize + Clone) {
    let _ = app.emit(event, payload);
}
