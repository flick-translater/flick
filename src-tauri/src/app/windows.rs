//! Window creation and visibility helpers.

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use super::{AppState, platform};

const MAIN_WINDOW_LABEL: &str = "main";
const TRANSLATE_WINDOW_LABEL: &str = "translate";

pub fn show_main_window(app: &AppHandle) -> tauri::Result<()> {
    platform::show_main_window_before_focus(app);
    let window = ensure_main_window(app)?;
    let _ = window.center();
    let _ = window.set_visible_on_all_workspaces(true);
    window.show()?;
    window.unminimize()?;
    platform::show_translate_window_after_show(app);
    window.set_focus()?;
    let _ = window.set_visible_on_all_workspaces(false);

    Ok(())
}

pub fn ensure_main_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        return Ok(window);
    }

    let builder =
        WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("index.html".into()))
            .title("Flick")
            .devtools(false)
            .inner_size(1240.0, 800.0)
            .min_inner_size(1040.0, 680.0)
            .resizable(true)
            .visible(false)
            .focused(false)
            .center();

    platform::configure_main_window_builder(builder).build()
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
    platform::show_translate_window_before_focus(app);
    let window = ensure_translate_window(app)?;
    let pinned = window.is_always_on_top().unwrap_or(false);

    #[cfg(not(target_os = "linux"))]
    if !pinned {
        let _ = window.center();
    }

    let _ = window.set_visible_on_all_workspaces(true);
    window.show()?;
    window.unminimize()?;
    platform::show_translate_window_after_show(app);
    window.set_focus()?;
    let _ = window.set_visible_on_all_workspaces(false);
    Ok(())
}

pub fn refresh_previous_frontmost_app(app: &AppHandle) {
    platform::refresh_previous_frontmost_app(app);
}

pub fn hide_translate_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(state) = app.try_state::<AppState>() {
        let _ = state.tts_service.stop();
        if let Ok(mut pinned) = state.translate_window_pinned.lock() {
            *pinned = false;
        }
    }

    platform::hide_translate_window_before_hide(app);

    if let Some(window) = app.get_webview_window(TRANSLATE_WINDOW_LABEL) {
        #[cfg(target_os = "linux")]
        {
            window.close()?;
        }

        #[cfg(not(target_os = "linux"))]
        {
            window.hide()?;
        }
    }

    platform::hide_translate_window_after_hide(app);

    Ok(())
}

pub fn emit_capture_status(app: &AppHandle, event: &str, payload: impl serde::Serialize + Clone) {
    let _ = app.emit(event, payload);
}
