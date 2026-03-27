use std::{path::PathBuf, sync::MutexGuard, thread};

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
    services::{OcrService, ScreenCaptureService, TranslationService},
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
) -> Result<(), FlickError> {
    let screenshot_dir = state.screenshot_dir.clone();

    #[cfg(not(target_os = "macos"))]
    let cached_screens = {
        let snapshots = state
            .capture_snapshots
            .lock()
            .map_err(|_| FlickError::Message("capture snapshots mutex poisoned".into()))?;
        snapshots.clone()
    };

    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWindow;

        let overlay = app
            .get_webview_window(CAPTURE_WINDOW_LABEL)
            .ok_or_else(|| FlickError::Message("capture overlay window not found".into()))?;
        let ns_window = overlay.ns_window()? as usize;
        app.run_on_main_thread(move || {
            let window: &NSWindow = unsafe { &*(ns_window as *mut std::ffi::c_void).cast() };
            window.orderOut(None);
        })?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        if let Some(window) = app.get_webview_window(CAPTURE_WINDOW_LABEL) {
            window.hide()?;
        }
        if let Ok(mut guard) = state.capture_snapshots.lock() {
            guard.clear();
        }
    }

    let app_handle = app.clone();
    thread::spawn(move || {
        let run = || -> Result<(), FlickError> {
            let capture_service = ScreenCaptureService::default();

            #[cfg(target_os = "macos")]
            let image = capture_service.capture_selection_on_screen(&selection)?;

            #[cfg(not(target_os = "macos"))]
            let image = capture_service.capture_selection(&selection, &cached_screens)?;

            capture_service.copy_to_clipboard(&image)?;

            let id = Uuid::new_v4().to_string();
            let created_at = Utc::now();
            let path = screenshot_dir.join(format!(
                "{}-{}.png",
                created_at.format("%Y%m%d-%H%M%S"),
                &id[..8]
            ));

            let record = CaptureRecord {
                id,
                created_at,
                width: image.width(),
                height: image.height(),
                path: path.display().to_string(),
            };

            let state = app_handle.state::<AppState>();
            let mut history = state
                .history
                .lock()
                .map_err(|_| FlickError::Message("history mutex poisoned".into()))?;
            history.push_front(record.clone());
            history.truncate(3);
            drop(history);

            emit_capture_status(&app_handle, "capture-finished", &record);
            save_capture_async(image, path);
            Ok(())
        };

        if let Err(error) = run() {
            emit_capture_status(&app_handle, "capture-error", error.to_string());
        }
    });

    Ok(())
}

#[tauri::command]
pub fn get_capture_context(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CaptureContext, FlickError> {
    if let Some(window) = app.get_webview_window(CAPTURE_WINDOW_LABEL) {
        let scale = window.scale_factor()?;
        let position = window.inner_position()?;
        let size = window.inner_size()?;

        let context = CaptureContext {
            x: position.x as f64 / scale,
            y: position.y as f64 / scale,
            width: size.width as f64 / scale,
            height: size.height as f64 / scale,
        };

        let mut guard = state
            .capture_context
            .lock()
            .map_err(|_| FlickError::Message("capture context mutex poisoned".into()))?;
        *guard = context.clone();
        return Ok(context);
    }

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
    eprintln!("[shortcut] update request received: raw='{}' normalized='{}'", shortcut, normalized);
    if normalized.is_empty() {
        eprintln!("[shortcut] rejected: empty shortcut");
        return Err(FlickError::Message("快捷键不能为空".into()));
    }

    let current = {
        let settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        settings.capture_shortcut.clone()
    };
    eprintln!("[shortcut] current shortcut: {}", current);

    if current == normalized {
        eprintln!("[shortcut] no-op update, shortcut unchanged");
        return get_app_settings(state);
    }

    let global_shortcut = app.global_shortcut();
    eprintln!(
        "[shortcut] is current registered? {}",
        global_shortcut.is_registered(current.as_str())
    );
    if global_shortcut.is_registered(current.as_str()) {
        eprintln!("[shortcut] unregister current: {}", current);
        global_shortcut.unregister(current.as_str())?;
        eprintln!("[shortcut] unregister success: {}", current);
    }

    eprintln!("[shortcut] register new: {}", normalized);
    if let Err(error) = global_shortcut.on_shortcut(normalized.as_str(), |app, _, event| {
        eprintln!(
            "[shortcut] dynamic handler fired for updated shortcut: state={:?}",
            event.state
        );
        if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
            let state = app.state::<AppState>();
            let _ = begin_capture_session(app, &state);
        }
    }) {
        eprintln!("[shortcut] register failed for {}: {}", normalized, error);
        if !current.is_empty() {
            eprintln!("[shortcut] restoring previous shortcut: {}", current);
            let _ = global_shortcut.on_shortcut(current.as_str(), |app, _, event| {
                eprintln!(
                    "[shortcut] restored handler fired: state={:?}",
                    event.state
                );
                if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                    let state = app.state::<AppState>();
                    let _ = begin_capture_session(app, &state);
                }
            });
        }
        return Err(FlickError::Message(format!("快捷键注册失败，可能已被其他应用占用: {error}")));
    }
    eprintln!("[shortcut] register success: {}", normalized);

    let updated = {
        let mut settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        settings.capture_shortcut = normalized.clone();
        settings.clone()
    };
    eprintln!("[shortcut] in-memory settings updated: {}", updated.capture_shortcut);

    state.settings_store.save_settings(&updated)?;
    eprintln!("[shortcut] persisted settings to store: {}", updated.capture_shortcut);
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
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
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

fn save_capture_async(
    image: image::ImageBuffer<image::Rgba<u8>, Vec<u8>>,
    path: PathBuf,
) {
    thread::spawn(move || {
        if let Err(error) = image.save(&path) {
            eprintln!("failed to save screenshot to {}: {}", path.display(), error);
        }
    });
}
