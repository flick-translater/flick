use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::MutexGuard,
    thread,
    time::SystemTime,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::GlobalShortcutExt as _;
use uuid::Uuid;

use crate::{
    AppState, CAPTURE_WINDOW_LABEL, CaptureIntent, apply_shortcut_bindings, emit_capture_status,
    ensure_capture_window, ensure_widget_window,
    error::FlickError,
    models::{
        AppSettings, AutostartStatus, CaptureContext, CaptureHistory, CaptureRecord, OcrRequest,
        OcrResponse, SelectionRect, StorageInfo, TranslateRequest, TranslateResponse,
    },
    services::{OcrService, ScreenCaptureService, TranslationService},
    show_widget_window,
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
        #[cfg(target_os = "macos")]
        restore_main_window_after_capture(&app, &state);
        #[cfg(target_os = "macos")]
        restore_previous_frontmost_app(&state);
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
    let screenshot_dir = current_screenshot_dir(&state)?;
    let intent = *state
        .capture_intent
        .lock()
        .map_err(|_| FlickError::Message("capture intent mutex poisoned".into()))?;
    let ocr_service = state.ocr_service.clone();
    let translation_service = state.translation_service.clone();

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
    let should_restore_previous_frontmost = intent == CaptureIntent::Capture;
    thread::spawn(move || {
        let run = || -> Result<(), FlickError> {
            let capture_service = ScreenCaptureService::default();

            #[cfg(target_os = "macos")]
            let image = capture_service.capture_selection_on_screen(&selection)?;

            #[cfg(not(target_os = "macos"))]
            let image = capture_service.capture_selection(&selection, &cached_screens)?;

            capture_service.copy_to_clipboard(&image)?;

            #[cfg(target_os = "macos")]
            {
                let state = app_handle.state::<AppState>();
                restore_main_window_after_capture(&app_handle, &state);
            }

            #[cfg(target_os = "macos")]
            if should_restore_previous_frontmost {
                let state = app_handle.state::<AppState>();
                restore_previous_frontmost_app(&state);
            }

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
            let max_screenshots = state
                .settings
                .lock()
                .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
                .max_screenshots;
            capture_service.save_png(&image, &path)?;
            prune_capture_history(&screenshot_dir, max_screenshots)?;

            let mut history = state
                .history
                .lock()
                .map_err(|_| FlickError::Message("history mutex poisoned".into()))?;
            history.push_front(record.clone());
            history.truncate(max_screenshots as usize);
            drop(history);

            emit_capture_status(&app_handle, "capture-finished", &record);
            if intent == CaptureIntent::Translate {
                let ocr = ocr_service.run(OcrRequest {
                    image_path: record.path.clone(),
                    language_hint: None,
                })?;
                let translation = translation_service.translate(TranslateRequest {
                    text: ocr.text.clone(),
                    source_language: None,
                    target_language: "zh".into(),
                })?;
                let payload = serde_json::json!({
                    "imagePath": record.path,
                    "sourceText": ocr.text,
                    "translatedText": translation.translated_text,
                    "provider": translation.provider,
                    "detectedSourceLanguage": translation.detected_source_language,
                    "targetLanguage": "zh",
                });
                let widget = ensure_widget_window(&app_handle)?;
                show_widget_window(&app_handle)?;
                let _ = widget.emit("translation-ready", payload);
            }
            Ok(())
        };

        if let Err(error) = run() {
            #[cfg(target_os = "macos")]
            {
                let state = app_handle.state::<AppState>();
                restore_main_window_after_capture(&app_handle, &state);
            }
            #[cfg(target_os = "macos")]
            if should_restore_previous_frontmost {
                let state = app_handle.state::<AppState>();
                restore_previous_frontmost_app(&state);
            }
            emit_capture_status(&app_handle, "capture-error", error.to_string());
        }
    });

    Ok(())
}

#[tauri::command]
pub fn get_capture_context(
    state: State<'_, AppState>,
) -> Result<CaptureContext, FlickError> {
    Ok(state
        .capture_context
        .lock()
        .map_err(|_| FlickError::Message("capture context mutex poisoned".into()))?
        .clone())
}

#[tauri::command]
pub fn list_capture_history(state: State<'_, AppState>) -> Result<CaptureHistory, FlickError> {
    let max_screenshots = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .max_screenshots;
    let screenshot_dir = current_screenshot_dir(&state)?;

    Ok(CaptureHistory {
        directory: screenshot_dir.display().to_string(),
        items: prune_capture_history(&screenshot_dir, max_screenshots)?,
    })
}

#[tauri::command]
pub fn get_storage_info(state: State<'_, AppState>) -> Result<StorageInfo, FlickError> {
    let screenshot_dir = current_screenshot_dir(&state)?;
    Ok(StorageInfo {
        data_dir: state.data_dir.display().to_string(),
        screenshot_dir: screenshot_dir.display().to_string(),
    })
}

#[tauri::command]
pub fn pick_screenshot_directory() -> Result<Option<String>, FlickError> {
    Ok(rfd::FileDialog::new()
        .set_title("Select Screenshot Directory")
        .pick_folder()
        .map(|path| path.display().to_string()))
}

#[tauri::command]
pub fn open_file_in_default_app(path: String) -> Result<(), FlickError> {
    if !Path::new(&path).exists() {
        return Err(FlickError::Message("file does not exist".into()));
    }

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(&path);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", &path]);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(&path);
        command
    };

    command
        .spawn()
        .map_err(|error| FlickError::Message(format!("failed to open file: {error}")))?;

    Ok(())
}

#[tauri::command]
pub fn read_image_as_data_url(path: String) -> Result<String, FlickError> {
    let bytes = fs::read(&path)
        .map_err(|error| FlickError::Message(format!("failed to read image: {error}")))?;

    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

#[tauri::command]
pub fn delete_capture(state: State<'_, AppState>, path: String) -> Result<(), FlickError> {
    let capture_path = Path::new(&path);
    let screenshot_dir = current_screenshot_dir(&state)?;

    if !capture_path.starts_with(&screenshot_dir) {
        return Err(FlickError::Message("capture path is outside screenshot directory".into()));
    }

    if !capture_path.exists() {
        return Ok(());
    }

    fs::remove_file(capture_path)
        .map_err(|error| FlickError::Message(format!("failed to delete capture: {error}")))?;

    if let Ok(mut history) = state.history.lock() {
        history.retain(|record| record.path != path);
    }

    Ok(())
}

#[tauri::command]
pub fn clear_all_captures(state: State<'_, AppState>) -> Result<(), FlickError> {
    let screenshot_dir = current_screenshot_dir(&state)?;
    let records = load_capture_history(&screenshot_dir)?;

    for record in records {
        let capture_path = Path::new(&record.path);
        if capture_path.starts_with(&screenshot_dir) && capture_path.exists() {
            fs::remove_file(capture_path)
                .map_err(|error| FlickError::Message(format!("failed to delete capture: {error}")))?;
        }
    }

    if let Ok(mut history) = state.history.lock() {
        history.clear();
    }

    Ok(())
}

fn load_capture_history(screenshot_dir: &Path) -> Result<Vec<CaptureRecord>, FlickError> {
    let mut records = Vec::new();
    let entries = fs::read_dir(screenshot_dir)
        .map_err(|error| FlickError::Message(format!("failed to read screenshot dir: {error}")))?;

    for entry in entries {
        let entry =
            entry.map_err(|error| FlickError::Message(format!("failed to read screenshot entry: {error}")))?;
        let path = entry.path();

        if !matches!(path.extension().and_then(|ext| ext.to_str()), Some("png")) {
            continue;
        }

        let metadata = entry
            .metadata()
            .map_err(|error| FlickError::Message(format!("failed to read screenshot metadata: {error}")))?;
        if !metadata.is_file() {
            continue;
        }

        let (width, height) = image::image_dimensions(&path)
            .map_err(|error| FlickError::Message(format!("failed to read screenshot dimensions: {error}")))?;
        let created_at = metadata
            .modified()
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(|_| DateTime::<Utc>::from(SystemTime::UNIX_EPOCH));
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        records.push(CaptureRecord {
            id,
            created_at,
            width,
            height,
            path: path.display().to_string(),
        });
    }

    records.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(records)
}

fn prune_capture_history(
    screenshot_dir: &Path,
    max_screenshots: u32,
) -> Result<Vec<CaptureRecord>, FlickError> {
    let records = load_capture_history(screenshot_dir)?;
    let keep_count = max_screenshots.max(1) as usize;

    for record in records.iter().skip(keep_count) {
        fs::remove_file(&record.path)
            .map_err(|error| FlickError::Message(format!("failed to remove old screenshot: {error}")))?;
    }

    Ok(records.into_iter().take(keep_count).collect())
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
pub fn show_translation_widget(app: AppHandle) -> Result<(), FlickError> {
    show_widget_window(&app)?;
    Ok(())
}

#[tauri::command]
pub fn get_translation_widget_pinned(app: AppHandle) -> Result<bool, FlickError> {
    Ok(ensure_widget_window(&app)?.is_always_on_top()?)
}

#[tauri::command]
pub fn set_translation_widget_pinned(app: AppHandle, pinned: bool) -> Result<(), FlickError> {
    ensure_widget_window(&app)?.set_always_on_top(pinned)?;
    Ok(())
}

#[tauri::command]
pub fn minimize_translation_widget(app: AppHandle) -> Result<(), FlickError> {
    ensure_widget_window(&app)?.minimize()?;
    Ok(())
}

#[tauri::command]
pub fn close_translation_widget(app: AppHandle) -> Result<(), FlickError> {
    ensure_widget_window(&app)?.close()?;
    Ok(())
}

#[tauri::command]
pub fn set_shortcut_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    recording: bool,
) -> Result<(), FlickError> {
    let settings = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .clone();
    let global_shortcut = app.global_shortcut();

    for shortcut in [&settings.capture_shortcut, &settings.translate_shortcut] {
        if recording {
            if global_shortcut.is_registered(shortcut.as_str()) {
                global_shortcut.unregister(shortcut.as_str())?;
            }
        } else if !global_shortcut.is_registered(shortcut.as_str()) {
            apply_shortcut_bindings(&app, &settings)
                .map_err(|error| FlickError::Message(format!("恢复快捷键失败: {error}")))?;
            break;
        }
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
    update_shortcut(app, state, shortcut, ShortcutKind::Capture)
}

#[tauri::command]
pub fn update_translate_shortcut(
    app: AppHandle,
    state: State<'_, AppState>,
    shortcut: String,
) -> Result<AppSettings, FlickError> {
    update_shortcut(app, state, shortcut, ShortcutKind::Translate)
}

#[tauri::command]
pub fn update_max_screenshots(
    state: State<'_, AppState>,
    max_screenshots: u32,
) -> Result<AppSettings, FlickError> {
    let normalized = max_screenshots.clamp(1, 1000);
    let updated = {
        let mut settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        settings.max_screenshots = normalized;
        settings.clone()
    };

    state.settings_store.save_settings(&updated)?;
    let screenshot_dir = current_screenshot_dir(&state)?;
    let _ = prune_capture_history(&screenshot_dir, normalized)?;
    if let Ok(mut history) = state.history.lock() {
        history.truncate(normalized as usize);
    }

    Ok(updated)
}

#[tauri::command]
pub fn update_interface_language(
    state: State<'_, AppState>,
    language: String,
) -> Result<AppSettings, FlickError> {
    let normalized = match language.trim().to_lowercase().as_str() {
        "zh" => "zh",
        "ja" => "ja",
        _ => "en",
    }
    .to_string();

    let updated = {
        let mut settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        settings.interface_language = normalized;
        settings.interface_language_set = true;
        settings.clone()
    };

    state.settings_store.save_settings(&updated)?;
    Ok(updated)
}

#[tauri::command]
pub fn update_screenshot_directory(
    state: State<'_, AppState>,
    path: String,
) -> Result<AppSettings, FlickError> {
    let normalized = path.trim();
    if normalized.is_empty() {
        return Err(FlickError::Message("screenshot directory cannot be empty".into()));
    }

    let next_dir = PathBuf::from(normalized);
    fs::create_dir_all(&next_dir)
        .map_err(|error| FlickError::Message(format!("failed to create screenshot directory: {error}")))?;

    {
        let mut screenshot_dir = state
            .screenshot_dir
            .lock()
            .map_err(|_| FlickError::Message("screenshot dir mutex poisoned".into()))?;
        *screenshot_dir = next_dir.clone();
    }

    let updated = {
        let mut settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        settings.screenshot_directory = next_dir.display().to_string();
        settings.clone()
    };

    state.settings_store.save_settings(&updated)?;
    Ok(updated)
}

#[derive(Clone, Copy)]
enum ShortcutKind {
    Capture,
    Translate,
}

fn update_shortcut(
    app: AppHandle,
    state: State<'_, AppState>,
    shortcut: String,
    kind: ShortcutKind,
) -> Result<AppSettings, FlickError> {
    let normalized = shortcut.trim().to_string();
    eprintln!(
        "[shortcut] update request received: kind={} raw='{}' normalized='{}'",
        shortcut_kind_name(&kind),
        shortcut,
        normalized
    );
    if normalized.is_empty() {
        eprintln!("[shortcut] rejected: empty shortcut");
        return Err(FlickError::Message("快捷键不能为空".into()));
    }

    let current_settings = {
        let settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        settings.clone()
    };
    let current = match kind {
        ShortcutKind::Capture => current_settings.capture_shortcut.clone(),
        ShortcutKind::Translate => current_settings.translate_shortcut.clone(),
    };
    eprintln!(
        "[shortcut] current {} shortcut: {}",
        shortcut_kind_name(&kind),
        current
    );

    if current == normalized {
        eprintln!("[shortcut] no-op update, shortcut unchanged");
        return get_app_settings(state);
    }

    let mut next_settings = current_settings.clone();
    match kind {
        ShortcutKind::Capture => next_settings.capture_shortcut = normalized.clone(),
        ShortcutKind::Translate => next_settings.translate_shortcut = normalized.clone(),
    }

    if let Err(error) = apply_shortcut_bindings(&app, &next_settings) {
        let _ = apply_shortcut_bindings(&app, &current_settings);
        return Err(FlickError::Message(format!(
            "快捷键注册失败，可能已被其他应用占用: {error}"
        )));
    }
    eprintln!(
        "[shortcut] register success: capture={} translate={}",
        next_settings.capture_shortcut, next_settings.translate_shortcut
    );

    let updated = {
        let mut settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        *settings = next_settings.clone();
        settings.clone()
    };
    eprintln!(
        "[shortcut] in-memory settings updated: capture={} translate={}",
        updated.capture_shortcut, updated.translate_shortcut
    );

    state.settings_store.save_settings(&updated)?;
    eprintln!(
        "[shortcut] persisted settings to store: capture={} translate={}",
        updated.capture_shortcut, updated.translate_shortcut
    );
    Ok(updated)
}

fn shortcut_kind_name(kind: &ShortcutKind) -> &'static str {
    match kind {
        ShortcutKind::Capture => "capture",
        ShortcutKind::Translate => "translate",
    }
}

#[tauri::command]
pub fn mock_ocr(
    state: State<'_, AppState>,
    request: OcrRequest,
) -> Result<OcrResponse, FlickError> {
    state.ocr_service.run(request).map_err(Into::into)
}

#[tauri::command]
pub fn mock_translate(
    state: State<'_, AppState>,
    request: TranslateRequest,
) -> Result<TranslateResponse, FlickError> {
    state
        .translation_service
        .translate(request)
        .map_err(Into::into)
}

pub fn open_capture_overlay(app: &AppHandle) -> Result<(), FlickError> {
    let window = ensure_capture_window(app)?;
    window.show()?;
    window.set_focus()?;
    thread::sleep(std::time::Duration::from_millis(16));
    emit_capture_status(app, "capture-started", "started");
    Ok(())
}

pub fn begin_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    begin_capture_session_with_intent(app, state, CaptureIntent::Capture)
}

pub fn begin_capture_session_with_intent(
    app: &AppHandle,
    state: &State<'_, AppState>,
    intent: CaptureIntent,
) -> Result<(), FlickError> {
    {
        let mut guard = state
            .capture_intent
            .lock()
            .map_err(|_| FlickError::Message("capture intent mutex poisoned".into()))?;
        *guard = intent;
    }

    #[cfg(target_os = "macos")]
    remember_previous_frontmost_app(state);

    if let Some(window) = app.get_webview_window("main") {
        let is_visible = window.is_visible().unwrap_or(false);
        let is_minimized = window.is_minimized().unwrap_or(false);
        #[cfg(target_os = "macos")]
        {
            let is_focused = window.is_focused().unwrap_or(false);
            if is_visible && !is_minimized && !is_focused {
                suppress_main_window_for_capture(app, state);
            }
        }
        let should_hide_main_window = !is_visible || is_minimized;
        if should_hide_main_window {
            let _ = window.hide();
        }
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

    let min_x = logical_monitors
        .iter()
        .map(|(x, _, _, _)| *x)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let min_y = logical_monitors
        .iter()
        .map(|(_, y, _, _)| *y)
        .reduce(f64::min)
        .unwrap_or(0.0);
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
    window.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
        min_x, min_y,
    )))?;
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

fn current_screenshot_dir(state: &State<'_, AppState>) -> Result<PathBuf, FlickError> {
    state
        .screenshot_dir
        .lock()
        .map_err(|_| FlickError::Message("screenshot dir mutex poisoned".into()))
        .map(|path| path.clone())
}

#[cfg(target_os = "macos")]
fn remember_previous_frontmost_app(state: &State<'_, AppState>) {
    use objc2_app_kit::{NSRunningApplication, NSWorkspace};

    let workspace = NSWorkspace::sharedWorkspace();
    let current_pid = NSRunningApplication::currentApplication().processIdentifier();
    let previous_pid = workspace
        .frontmostApplication()
        .map(|app| app.processIdentifier())
        .filter(|pid| *pid != current_pid);

    if let Ok(mut guard) = state.capture_previous_frontmost_pid.lock() {
        *guard = previous_pid;
    }
}

#[cfg(target_os = "macos")]
fn restore_previous_frontmost_app(state: &State<'_, AppState>) {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};

    let previous_pid = match state.capture_previous_frontmost_pid.lock() {
        Ok(mut guard) => guard.take(),
        Err(_) => None,
    };

    if let Some(app) =
        previous_pid.and_then(NSRunningApplication::runningApplicationWithProcessIdentifier)
    {
        let _ = app.activateWithOptions(NSApplicationActivationOptions::empty());
    }
}

#[cfg(target_os = "macos")]
fn suppress_main_window_for_capture(app: &AppHandle, state: &State<'_, AppState>) {
    let should_suppress = match state.capture_main_window_suppressed.lock() {
        Ok(mut guard) => {
            if *guard {
                false
            } else {
                *guard = true;
                true
            }
        }
        Err(_) => false,
    };

    if should_suppress {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_focusable(false);
            let _ = window.set_always_on_bottom(true);
        }
    }
}

#[cfg(target_os = "macos")]
fn restore_main_window_after_capture(app: &AppHandle, state: &State<'_, AppState>) {
    let should_restore = match state.capture_main_window_suppressed.lock() {
        Ok(mut guard) => {
            let value = *guard;
            *guard = false;
            value
        }
        Err(_) => false,
    };

    if should_restore {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_always_on_bottom(false);
            let _ = window.set_focusable(true);
        }
    }
}
