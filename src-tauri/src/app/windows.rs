//! Window creation and visibility helpers.

use tauri::{
    ActivationPolicy, AppHandle, Emitter, LogicalPosition, Manager, TitleBarStyle, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

const MAIN_WINDOW_LABEL: &str = "main";
const CAPTURE_WINDOW_LABEL_PREFIX: &str = "capture";
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
    // Lazily create the main window so setup and reopen paths can share the same entry.
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("index.html".into()))
        .title("Flick")
        .devtools(false)
        .inner_size(1240.0, 800.0)
        .min_inner_size(1040.0, 680.0)
        .resizable(true)
        .visible(true)
        .focused(true)
        .center()
        .hidden_title(true)
        .title_bar_style(TitleBarStyle::Overlay)
        .traffic_light_position(LogicalPosition::new(16.0, 18.0))
        .build()
}

pub fn capture_window_label(index: usize) -> String {
    format!("{CAPTURE_WINDOW_LABEL_PREFIX}-{index}")
}

pub fn is_capture_window_label(label: &str) -> bool {
    label.starts_with(&format!("{CAPTURE_WINDOW_LABEL_PREFIX}-"))
}

pub fn ensure_capture_window(app: &AppHandle, label: &str) -> tauri::Result<WebviewWindow> {
    // Capture windows are transparent overlays, one per monitor.
    if let Some(window) = app.get_webview_window(label) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(app, label, WebviewUrl::App("capture.html".into()))
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible(false)
        .resizable(false)
        .build()
}

pub fn initialize_capture_windows(app: &AppHandle) -> tauri::Result<()> {
    let monitors = app.available_monitors()?;
    for index in 0..monitors.len() {
        let label = capture_window_label(index);
        ensure_capture_window(app, &label)?;
    }
    Ok(())
}

pub fn ensure_widget_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    // The translation widget is persistent and reused across captures.
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

pub fn emit_capture_status(app: &AppHandle, event: &str, payload: impl serde::Serialize + Clone) {
    let _ = app.emit(event, payload);
}
