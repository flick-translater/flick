//! Thin Tauri command adapters for the floating translation window.

#[cfg(target_os = "macos")]
use std::sync::mpsc;

use tauri::{AppHandle, Manager};

use crate::{
    app::{AppState, windows},
    error::FlickError,
    features::translation,
    models::TranslateWindowState,
    services::{TtsSnapshot, TtsTarget},
};

#[tauri::command]
pub fn show_translate_window(app: AppHandle) -> Result<(), FlickError> {
    windows::show_translate_window(&app)?;
    Ok(())
}

#[tauri::command]
pub fn get_translate_window_pinned(
    _app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<bool, FlickError> {
    state
        .translate_window_pinned
        .lock()
        .map(|value| *value)
        .map_err(|_| FlickError::LockError("translate_window_pinned".into()))
}

#[tauri::command]
pub fn is_translate_window_pinning_supported() -> bool {
    crate::app::platform::translate_window_pinning_supported()
}

#[tauri::command]
pub fn get_translate_window_state(
    state: tauri::State<'_, AppState>,
) -> Result<TranslateWindowState, FlickError> {
    state
        .translate_window_state
        .lock()
        .map(|value| value.clone())
        .map_err(|_| FlickError::LockError("translate_window_state".into()))
}

#[tauri::command]
pub fn swap_translate_window_content(
    state: tauri::State<'_, AppState>,
) -> Result<TranslateWindowState, FlickError> {
    let mut snapshot = state
        .translate_window_state
        .lock()
        .map_err(|_| FlickError::LockError("translate_window_state".into()))?;

    let resolved_source_language = snapshot
        .detected_source_language
        .as_deref()
        .filter(|value| !value.eq_ignore_ascii_case("auto"))
        .or(snapshot.ocr_detected_source_language.as_deref())
        .unwrap_or(snapshot.target_language.as_str())
        .to_string();
    let previous_target_language = snapshot.target_language.clone();

    let previous_source_text = std::mem::take(&mut snapshot.source_text);
    snapshot.source_text = std::mem::take(&mut snapshot.translated_text);
    snapshot.translated_text = previous_source_text;
    snapshot.detected_source_language = Some(previous_target_language.clone());
    snapshot.ocr_detected_source_language = Some(previous_target_language);
    snapshot.target_language = resolved_source_language;

    Ok(snapshot.clone())
}

#[tauri::command]
pub fn set_translate_window_pinned(app: AppHandle, pinned: bool) -> Result<(), FlickError> {
    if !crate::app::platform::translate_window_pinning_supported() {
        return Err(FlickError::Message(
            "translate window pinning is not supported on this platform/session".into(),
        ));
    }

    let window = windows::ensure_translate_window(&app)?;
    window.set_always_on_top(pinned)?;
    if let Some(state) = app.try_state::<AppState>() {
        let mut pinned_state = state
            .translate_window_pinned
            .lock()
            .map_err(|_| FlickError::LockError("translate_window_pinned".into()))?;
        *pinned_state = pinned;
    }
    if !pinned {
        windows::refresh_previous_frontmost_app(&app);
    }
    Ok(())
}

#[tauri::command]
pub fn minimize_translate_window(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), FlickError> {
    state.tts_service.stop()?;
    windows::ensure_translate_window(&app)?.minimize()?;
    Ok(())
}

#[tauri::command]
pub fn close_translate_window(app: AppHandle) -> Result<(), FlickError> {
    windows::hide_translate_window(&app)?;
    Ok(())
}

#[tauri::command]
pub fn speak_window_text(
    state: tauri::State<'_, AppState>,
    text: String,
    language: Option<String>,
    target: TtsTarget,
) -> Result<(), FlickError> {
    state
        .tts_service
        .speak(&text, language.as_deref(), target)?;
    Ok(())
}

#[tauri::command]
pub fn stop_window_tts(state: tauri::State<'_, AppState>) -> Result<(), FlickError> {
    state.tts_service.stop()?;
    Ok(())
}

#[tauri::command]
pub fn get_window_tts_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<TtsSnapshot, FlickError> {
    Ok(state.tts_service.snapshot())
}

#[tauri::command]
pub fn translate_selected_text(app: AppHandle) -> Result<(), FlickError> {
    translation::translate_selected_text_to_window(&app)
}

#[tauri::command]
pub fn begin_translate_window_drag(app: AppHandle) -> Result<(), FlickError> {
    let window = windows::ensure_translate_window(&app)?;

    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSApplication, NSWindow};

        let ns_window = window.ns_window()? as usize;
        let (sender, receiver) = mpsc::channel();

        app.run_on_main_thread(move || {
            let result = (|| -> Result<(), FlickError> {
                let mtm = MainThreadMarker::new()
                    .ok_or_else(|| FlickError::Message("main thread marker unavailable".into()))?;
                let app = NSApplication::sharedApplication(mtm);
                let window: &NSWindow =
                    unsafe { &*(ns_window as *mut std::ffi::c_void).cast::<NSWindow>() };

                app.activate();
                window.makeKeyAndOrderFront(None);

                let event = app
                    .currentEvent()
                    .ok_or_else(|| FlickError::Message("no current mouse event for drag".into()))?;
                window.performWindowDragWithEvent(&event);
                Ok(())
            })();

            let _ = sender.send(result);
        })?;

        return receiver
            .recv()
            .map_err(|error| FlickError::Message(error.to_string()))?;
    }

    #[cfg(target_os = "linux")]
    {
        let _ = app;
        window.set_focus()?;
        window.start_dragging()?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        let _ = app;
        window.set_focus()?;
        window.start_dragging()?;
        Ok(())
    }
}
