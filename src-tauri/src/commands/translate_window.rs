//! Thin Tauri command adapters for the floating translation window.

#[cfg(target_os = "macos")]
use std::sync::mpsc;

use tauri::AppHandle;

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
pub fn get_translate_window_pinned(app: AppHandle) -> Result<bool, FlickError> {
    Ok(windows::ensure_translate_window(&app)?.is_always_on_top()?)
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
pub fn set_translate_window_pinned(app: AppHandle, pinned: bool) -> Result<(), FlickError> {
    windows::ensure_translate_window(&app)?.set_always_on_top(pinned)?;
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

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        let _ = window;
        Err(FlickError::Message(
            "translation window native drag is only implemented on macOS".into(),
        ))
    }
}
