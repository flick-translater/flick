//! Thin Tauri command adapters for settings and app integration points.

use std::{fs, path::PathBuf};

use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt as _;
#[cfg(not(target_os = "macos"))]
use tauri_plugin_global_shortcut::GlobalShortcutExt as _;

use crate::{
    app::{AppState, apply_shortcut_bindings},
    error::FlickError,
    features::capture,
    models::{AISettings, AppSettings, AutostartStatus, OcrEngineInfo},
    services::{available_ocr_engines, create_ocr_service},
};

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
    crate::app::set_autostart_enabled(&app, enabled)?;
    Ok(())
}

#[tauri::command]
pub fn set_shortcut_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    recording: bool,
) -> Result<(), FlickError> {
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        let _ = state;
        crate::app::macos_hotkeys::set_recording_paused(recording)
            .map_err(|error| FlickError::Message(format!("切换快捷键录制状态失败: {error}")))?;
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
            .clone();
        let global_shortcut = app.global_shortcut();

        for shortcut in [
            &settings.capture_shortcut,
            &settings.translate_shortcut,
            &settings.selected_translate_shortcut,
        ] {
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
}

#[tauri::command]
pub fn get_app_settings(state: State<'_, AppState>) -> Result<AppSettings, FlickError> {
    let settings = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .clone();

    Ok(settings)
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
pub fn update_selected_translate_shortcut(
    app: AppHandle,
    state: State<'_, AppState>,
    shortcut: String,
) -> Result<AppSettings, FlickError> {
    update_shortcut(app, state, shortcut, ShortcutKind::SelectedTranslate)
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
    let screenshot_dir = capture::current_screenshot_dir(&state)?;
    let _ = capture::prune_capture_history(&screenshot_dir, normalized)?;
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
        return Err(FlickError::Message(
            "screenshot directory cannot be empty".into(),
        ));
    }

    let next_dir = PathBuf::from(normalized);
    fs::create_dir_all(&next_dir).map_err(|error| {
        FlickError::Message(format!("failed to create screenshot directory: {error}"))
    })?;

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

#[tauri::command]
pub fn update_ocr_provider(
    state: State<'_, AppState>,
    provider: String,
) -> Result<AppSettings, FlickError> {
    let normalized = provider.trim().to_lowercase();
    let available = available_ocr_engines();
    if !available.iter().any(|engine| engine.id == normalized) {
        return Err(FlickError::Message(
            "invalid OCR provider for current platform".into(),
        ));
    }

    let new_service = create_ocr_service(&normalized);

    let updated = {
        let mut settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        settings.ocr_provider = normalized;

        let mut ocr_service = state
            .ocr_service
            .lock()
            .map_err(|_| FlickError::Message("ocr service mutex poisoned".into()))?;
        *ocr_service = new_service;

        settings.clone()
    };

    state.settings_store.save_settings(&updated)?;
    Ok(updated)
}

#[tauri::command]
pub fn get_available_ocr_engines() -> Result<Vec<OcrEngineInfo>, FlickError> {
    Ok(available_ocr_engines())
}

#[tauri::command]
pub fn update_ocr_shortcut_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, FlickError> {
    let current_settings = {
        let settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        settings.clone()
    };

    if current_settings.ocr_shortcut_enabled == enabled {
        return get_app_settings(state);
    }

    let mut next_settings = current_settings.clone();
    next_settings.ocr_shortcut_enabled = enabled;

    if let Err(error) = apply_shortcut_bindings(&app, &next_settings) {
        let _ = apply_shortcut_bindings(&app, &current_settings);
        return Err(FlickError::Message(format!(
            "OCR 快捷键状态更新失败: {error}"
        )));
    }

    let updated = {
        let mut settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        *settings = next_settings.clone();
        settings.clone()
    };

    state.settings_store.save_settings(&updated)?;
    Ok(updated)
}

#[tauri::command]
pub fn update_ocr_auto_translate(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, FlickError> {
    let updated = {
        let mut settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        settings.ocr_auto_translate = enabled;
        settings.clone()
    };

    state.settings_store.save_settings(&updated)?;
    Ok(updated)
}

#[tauri::command]
pub fn update_ocr_target_language(
    state: State<'_, AppState>,
    language: String,
) -> Result<AppSettings, FlickError> {
    let normalized = language.trim().to_lowercase();
    if normalized.is_empty() {
        return Err(FlickError::Message("OCR 目标语言不能为空".into()));
    }

    let updated = {
        let mut settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        settings.ocr_target_language = normalized;
        settings.clone()
    };

    state.settings_store.save_settings(&updated)?;
    Ok(updated)
}

#[derive(Clone, Copy)]
enum ShortcutKind {
    Capture,
    Translate,
    SelectedTranslate,
}

fn update_shortcut(
    app: AppHandle,
    state: State<'_, AppState>,
    shortcut: String,
    kind: ShortcutKind,
) -> Result<AppSettings, FlickError> {
    let normalized = shortcut.trim().to_string();
    if normalized.is_empty() {
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
        ShortcutKind::SelectedTranslate => current_settings.selected_translate_shortcut.clone(),
    };
    if current == normalized {
        return get_app_settings(state);
    }

    let mut next_settings = current_settings.clone();
    match kind {
        ShortcutKind::Capture => next_settings.capture_shortcut = normalized.clone(),
        ShortcutKind::Translate => next_settings.translate_shortcut = normalized.clone(),
        ShortcutKind::SelectedTranslate => {
            next_settings.selected_translate_shortcut = normalized.clone()
        }
    }

    if let Err(error) = apply_shortcut_bindings(&app, &next_settings) {
        let _ = apply_shortcut_bindings(&app, &current_settings);
        return Err(FlickError::Message(format!(
            "快捷键注册失败，可能已被其他应用占用: {error}"
        )));
    }

    let updated = {
        let mut settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        *settings = next_settings.clone();
        settings.clone()
    };

    state.settings_store.save_settings(&updated)?;
    Ok(updated)
}

#[tauri::command]
pub fn update_ai_settings(
    state: State<'_, AppState>,
    ai_settings: AISettings,
) -> Result<AppSettings, FlickError> {
    let updated = {
        let mut settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?;
        settings.ai = ai_settings;
        settings.clone()
    };

    state.settings_store.save_settings(&updated)?;
    Ok(updated)
}
