use std::sync::MutexGuard;

use chrono::Utc;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::GlobalShortcutExt as _;
use uuid::Uuid;

use crate::{
    AppState, CAPTURE_WINDOW_LABEL, ensure_capture_window, emit_capture_status,
    error::FlickError,
    models::{
        AppSettings, AutostartStatus, CaptureContext, CaptureRecord, OcrRequest, OcrResponse, SelectionRect,
        TranslateRequest, TranslateResponse,
    },
    services::{OcrService, TranslationService},
};

#[tauri::command]
pub fn start_capture(app: AppHandle, state: State<'_, AppState>) -> Result<(), FlickError> {
    begin_capture_session(&app, &state)
}

#[tauri::command]
pub fn cancel_capture(app: AppHandle) -> Result<(), FlickError> {
    if let Some(window) = app.get_webview_window(CAPTURE_WINDOW_LABEL) {
        window.hide()?;
    }

    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut guard) = state.capture_snapshots.lock() {
            guard.clear();
        }
    }

    emit_capture_status(&app, "capture-cancelled", "cancelled");
    Ok(())
}

#[tauri::command]
pub fn complete_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    selection: SelectionRect,
) -> Result<CaptureRecord, FlickError> {
    #[cfg(target_os = "macos")]
    let image = {
        use objc2_app_kit::NSWindow;

        let overlay = app
            .get_webview_window(CAPTURE_WINDOW_LABEL)
            .ok_or_else(|| FlickError::Message("capture overlay window not found".into()))?;
        let ns_window = overlay.ns_window()?;
        let window: &NSWindow = unsafe { &*ns_window.cast() };
        let window_number = window.windowNumber() as u32;
        let image = state
            .capture_service
            .capture_selection_below_window(&selection, window_number)?;
        overlay.hide()?;
        image
    };

    #[cfg(not(target_os = "macos"))]
    let image = {
        if let Some(window) = app.get_webview_window(CAPTURE_WINDOW_LABEL) {
            window.hide()?;
        }

        let image = {
            let snapshots = state
                .capture_snapshots
                .lock()
                .map_err(|_| FlickError::Message("capture snapshots mutex poisoned".into()))?;
            state.capture_service.capture_selection(&selection, &snapshots)?
        };

        if let Ok(mut guard) = state.capture_snapshots.lock() {
            guard.clear();
        }

        image
    };
    state.capture_service.copy_to_clipboard(&image)?;

    let id = Uuid::new_v4().to_string();
    let created_at = Utc::now();
    let path = state
        .screenshot_dir
        .join(format!("{}-{}.png", created_at.format("%Y%m%d-%H%M%S"), &id[..8]));

    state.capture_service.save_png(&image, &path)?;

    let record = CaptureRecord {
        id,
        created_at,
        width: image.width(),
        height: image.height(),
        path: path.display().to_string(),
    };

    let mut history = state.history.lock().map_err(|_| FlickError::Message("history mutex poisoned".into()))?;
    history.push_front(record.clone());
    history.truncate(20);

    emit_capture_status(&app, "capture-finished", &record);

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
    }

    Ok(record)
}

#[tauri::command]
pub fn get_capture_context(state: State<'_, AppState>) -> Result<CaptureContext, FlickError> {
    Ok(state
        .capture_context
        .lock()
        .map_err(|_| FlickError::Message("capture context mutex poisoned".into()))?
        .clone())
}

#[tauri::command]
pub fn list_capture_history(state: State<'_, AppState>) -> Result<Vec<CaptureRecord>, FlickError> {
    let history = state
        .history
        .lock()
        .map_err(|_| FlickError::Message("history mutex poisoned".into()))?;

    Ok(history.iter().cloned().collect())
}

#[tauri::command]
pub fn get_autostart_status(app: AppHandle) -> Result<AutostartStatus, FlickError> {
    let enabled = app.autolaunch().is_enabled().unwrap_or(false);

    Ok(AutostartStatus {
        enabled,
        supported: cfg!(desktop),
    })
}

#[tauri::command]
pub fn set_autostart_enabled(app: AppHandle, enabled: bool) -> Result<(), FlickError> {
    if enabled {
        app.autolaunch().enable()?;
    } else {
        app.autolaunch().disable()?;
    }

    Ok(())
}

#[tauri::command]
pub fn get_app_settings(state: State<'_, AppState>) -> Result<AppSettings, FlickError> {
    Ok(state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .clone())
}

#[tauri::command]
pub fn update_capture_shortcut(
    app: AppHandle,
    state: State<'_, AppState>,
    shortcut: String,
) -> Result<AppSettings, FlickError> {
    let normalized = shortcut.trim().to_string();
    if normalized.is_empty() {
        return Err(FlickError::Message("快捷键不能为空".into()));
    }

    let current = {
        let settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        settings.capture_shortcut.clone()
    };

    if current == normalized {
        return get_app_settings(state);
    }

    let global_shortcut = app.global_shortcut();
    if global_shortcut.is_registered(current.as_str()) {
        global_shortcut.unregister(current.as_str())?;
    }
    global_shortcut.on_shortcut(normalized.as_str(), |app, _, event| {
        if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
            let _ = open_capture_overlay(app);
        }
    })?;

    let updated = {
        let mut settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        settings.capture_shortcut = normalized.clone();
        settings.clone()
    };

    state.settings_store.save_settings(&updated)?;
    Ok(updated)
}

#[tauri::command]
pub fn mock_ocr(state: State<'_, AppState>, request: OcrRequest) -> Result<OcrResponse, FlickError> {
    state.ocr_service.run(request).map_err(Into::into)
}

#[tauri::command]
pub fn mock_translate(
    state: State<'_, AppState>,
    request: TranslateRequest,
) -> Result<TranslateResponse, FlickError> {
    state.translation_service.translate(request).map_err(Into::into)
}

pub fn open_capture_overlay(app: &AppHandle) -> Result<(), FlickError> {
    let window = ensure_capture_window(app)?;
    window.show()?;
    window.set_focus()?;
    Ok(())
}

pub fn begin_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    prepare_capture_context(app, state)?;
    #[cfg(not(target_os = "macos"))]
    let snapshots = state.capture_service.capture_all_screens()?;
    #[cfg(not(target_os = "macos"))]
    {
        let mut guard = state
            .capture_snapshots
            .lock()
            .map_err(|_| FlickError::Message("capture snapshots mutex poisoned".into()))?;
        *guard = snapshots;
    }
    open_capture_overlay(app)?;
    Ok(())
}

fn prepare_capture_context(app: &AppHandle, state: &State<'_, AppState>) -> Result<(), FlickError> {
    let monitors = app.available_monitors()?;
    let logical_monitors = monitors
        .iter()
        .map(|monitor| {
            let scale = monitor.scale_factor();
            let x = monitor.position().x as f64 / scale;
            let y = monitor.position().y as f64 / scale;
            let width = monitor.size().width as f64 / scale;
            let height = monitor.size().height as f64 / scale;

            (x, y, width, height)
        })
        .collect::<Vec<_>>();

    let min_x = logical_monitors.iter().map(|(x, _, _, _)| *x).reduce(f64::min).unwrap_or(0.0);
    let min_y = logical_monitors.iter().map(|(_, y, _, _)| *y).reduce(f64::min).unwrap_or(0.0);
    let max_x = logical_monitors
        .iter()
        .map(|(x, _, width, _)| x + width)
        .reduce(f64::max)
        .unwrap_or(1440.0);
    let max_y = logical_monitors
        .iter()
        .map(|(_, y, _, height)| y + height)
        .reduce(f64::max)
        .unwrap_or(900.0);

    let width = (max_x - min_x).max(1.0);
    let height = (max_y - min_y).max(1.0);

    let context = CaptureContext {
        x: min_x,
        y: min_y,
        width,
        height,
    };

    {
        let mut guard = lock_capture_context(state)?;
        *guard = context.clone();
    }

    let window = ensure_capture_window(app)?;
    window.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(min_x, min_y)))?;
    window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(width, height)))?;
    window.set_always_on_top(true)?;

    Ok(())
}

fn lock_capture_context<'a>(
    state: &'a State<'_, AppState>,
) -> Result<MutexGuard<'a, CaptureContext>, FlickError> {
    state
        .capture_context
        .lock()
        .map_err(|_| FlickError::Message("capture context mutex poisoned".into()))
}
