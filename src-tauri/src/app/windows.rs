//! Window creation and visibility helpers.

use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::models::SelectionRect;

use super::{platform, AppState};

const MAIN_WINDOW_LABEL: &str = "main";
const TRANSLATE_WINDOW_LABEL: &str = "translate";
const SCREENSHOT_EDITOR_WINDOW_PREFIX: &str = "screenshot-editor";
const PRELOADED_SCREENSHOT_EDITOR_WINDOW_LABEL: &str = "screenshot-editor-preload";

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

    let window = platform::configure_main_window_builder(builder).build()?;
    platform::configure_built_window(&window);
    Ok(window)
}

pub fn ensure_translate_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window(TRANSLATE_WINDOW_LABEL) {
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(
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
    .build()?;
    platform::configure_built_window(&window);
    Ok(window)
}

pub fn show_screenshot_editor_window(
    app: &AppHandle,
    session_id: &str,
    selection: &SelectionRect,
    _image_width: u32,
    _image_height: u32,
    editor_color: &str,
) -> tauri::Result<WebviewWindow> {
    let label = format!("{SCREENSHOT_EDITOR_WINDOW_PREFIX}-{session_id}");
    if let Some(window) = app.get_webview_window(&label) {
        window.show()?;
        window.set_focus()?;
        return Ok(window);
    }

    let (desktop_x, desktop_y, desktop_width, desktop_height) = selection_primary_monitor_bounds(app, selection)
        .unwrap_or((
            selection.x as f64,
            selection.y as f64,
            selection.width as f64,
            selection.height as f64 + 72.0,
        ));
    let selection_left = selection.x as f64 - desktop_x;
    let selection_top = selection.y as f64 - desktop_y;
    let toolbar_width = desktop_width.min(680.0).max(1.0);
    let toolbar_height = 58.0;
    let toolbar_top_below = selection_top + selection.height as f64 + 8.0;
    let toolbar_top = if toolbar_top_below + toolbar_height <= desktop_height - 8.0 {
        toolbar_top_below
    } else {
        (selection_top - toolbar_height - 8.0).max(8.0)
    };
    let toolbar_left = if selection_left + toolbar_width <= desktop_width - 8.0 {
        selection_left.max(8.0)
    } else {
        (selection_left + selection.width as f64 - toolbar_width)
            .max(8.0)
            .min((desktop_width - toolbar_width - 8.0).max(8.0))
    };

    let color_param = {
        let color = editor_color.trim().trim_start_matches('#');
        if color.len() == 6 && color.chars().all(|char| char.is_ascii_hexdigit()) {
            color.to_ascii_lowercase()
        } else {
            "ef4444".into()
        }
    };
    let url = format!(
        "screenshot-editor.html?session_id={session_id}&display_width={}&display_height={}&selection_left={selection_left}&selection_top={selection_top}&toolbar_left={toolbar_left}&toolbar_top={toolbar_top}&color={color_param}",
        selection.width, selection.height
    );

    if let Some(window) = app.get_webview_window(PRELOADED_SCREENSHOT_EDITOR_WINDOW_LABEL) {
        let _ = window.set_size(LogicalSize::new(desktop_width, desktop_height));
        let _ = window.set_position(LogicalPosition::new(desktop_x, desktop_y));
        match window.url() {
            Ok(mut current_url) => {
                current_url.set_query(Some(url.split_once('?').map(|(_, query)| query).unwrap_or("")));
                current_url.set_path("screenshot-editor.html");
                window.navigate(current_url)?;
                platform::configure_screenshot_editor_window(&window);
                window.show()?;
                window.set_focus()?;
                return Ok(window);
            }
            Err(_) => {
                let _ = window.close();
            }
        }
    }

    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App(url.into()))
        .title("Flick Screenshot Editor")
        .devtools(false)
        .inner_size(desktop_width, desktop_height)
        .position(desktop_x, desktop_y)
        .resizable(false)
        .visible(true)
        .focused(true)
        .always_on_top(true)
        .accept_first_mouse(true)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .build()?;
    platform::configure_built_window(&window);
    platform::configure_screenshot_editor_window(&window);
    let _ = window.set_position(LogicalPosition::new(desktop_x, desktop_y));
    let _ = window.set_focus();
    Ok(window)
}

pub fn preload_screenshot_editor_window(app: &AppHandle) -> tauri::Result<()> {
    if app
        .get_webview_window(PRELOADED_SCREENSHOT_EDITOR_WINDOW_LABEL)
        .is_some()
    {
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(
        app,
        PRELOADED_SCREENSHOT_EDITOR_WINDOW_LABEL,
        WebviewUrl::App("screenshot-editor.html?preload=1".into()),
    )
    .title("Flick Screenshot Editor")
    .devtools(false)
    .inner_size(1.0, 1.0)
    .resizable(false)
    .visible(false)
    .focused(false)
    .always_on_top(true)
    .accept_first_mouse(true)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .build()?;
    platform::configure_built_window(&window);
    platform::configure_screenshot_editor_window(&window);
    Ok(())
}

pub fn close_screenshot_editor_window(app: &AppHandle, session_id: &str) {
    let label = format!("{SCREENSHOT_EDITOR_WINDOW_PREFIX}-{session_id}");
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.close();
    }
    if let Some(window) = app.get_webview_window(PRELOADED_SCREENSHOT_EDITOR_WINDOW_LABEL) {
        let _ = window.close();
    }
}

pub fn selection_spans_multiple_monitors(app: &AppHandle, selection: &SelectionRect) -> bool {
    intersecting_monitor_count(app, selection).unwrap_or(1) > 1
}

fn selection_primary_monitor_bounds(app: &AppHandle, selection: &SelectionRect) -> Option<(f64, f64, f64, f64)> {
    let monitors = app.available_monitors().ok()?;
    let mut best_bounds = None;
    let mut best_overlap = 0.0;
    let selection_left = selection.x as f64;
    let selection_top = selection.y as f64;
    let selection_right = selection_left + selection.width as f64;
    let selection_bottom = selection_top + selection.height as f64;

    for monitor in monitors {
        let scale = monitor.scale_factor();
        let x = monitor.position().x as f64 / scale;
        let y = monitor.position().y as f64 / scale;
        let width = monitor.size().width as f64 / scale;
        let height = monitor.size().height as f64 / scale;
        let overlap_width = (selection_right.min(x + width) - selection_left.max(x)).max(0.0);
        let overlap_height = (selection_bottom.min(y + height) - selection_top.max(y)).max(0.0);
        let overlap = overlap_width * overlap_height;
        if overlap > best_overlap {
            best_overlap = overlap;
            best_bounds = Some((x, y, width.max(1.0), height.max(1.0)));
        }
    }

    best_bounds
}

fn intersecting_monitor_count(app: &AppHandle, selection: &SelectionRect) -> Option<usize> {
    let monitors = app.available_monitors().ok()?;
    let selection_left = selection.x as f64;
    let selection_top = selection.y as f64;
    let selection_right = selection_left + selection.width as f64;
    let selection_bottom = selection_top + selection.height as f64;
    let mut count = 0;

    for monitor in monitors {
        let scale = monitor.scale_factor();
        let x = monitor.position().x as f64 / scale;
        let y = monitor.position().y as f64 / scale;
        let width = monitor.size().width as f64 / scale;
        let height = monitor.size().height as f64 / scale;
        let overlap_width = (selection_right.min(x + width) - selection_left.max(x)).max(0.0);
        let overlap_height = (selection_bottom.min(y + height) - selection_top.max(y)).max(0.0);
        if overlap_width * overlap_height > 0.0 {
            count += 1;
        }
    }

    Some(count)
}

pub fn show_translate_window(app: &AppHandle) -> tauri::Result<()> {
    platform::show_translate_window_before_focus(app);
    let window = ensure_translate_window(app)?;

    #[cfg(not(target_os = "linux"))]
    {
        let pinned = window.is_always_on_top().unwrap_or(false);
        if !pinned {
            let _ = window.center();
        }
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
