use std::thread;

use chrono::Utc;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::{
    app::{AppState, CaptureIntent, windows::emit_capture_status},
    error::FlickError,
    features::{ocr, translation},
    models::{CaptureRecord, OcrRequest, SelectionRect, TranslateRequest},
    services::ScreenCaptureService,
};

use super::{history, platform};

pub fn cancel_capture(app: &AppHandle) -> Result<(), FlickError> {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut guard) = state.capture_snapshots.lock() {
            guard.clear();
        }
        platform::cancel_interactive_capture(app, &state);
        platform::cleanup_after_cancel(app, &state);
    }

    emit_capture_status(app, "capture-cancelled", "cancelled");
    Ok(())
}

pub fn complete_capture(
    app: &AppHandle,
    state: &State<'_, AppState>,
    selection: SelectionRect,
) -> Result<(), FlickError> {
    let screenshot_dir = history::current_screenshot_dir(state)?;
    let intent = *state
        .capture_intent
        .lock()
        .map_err(|_| FlickError::Message("capture intent mutex poisoned".into()))?;
    let ocr_service = state.ocr_service.clone();
    let translation_service = state.translation_service.clone();
    let cached_screens = platform::complete_ui_before_capture_processing(app, state)?;

    let app_handle = app.clone();
    let should_restore_previous_frontmost = intent == CaptureIntent::Capture;
    // The expensive capture, disk IO, and OCR/translation chain runs off the UI thread.
    thread::spawn(move || {
        let run = || -> Result<(), FlickError> {
            let capture_service = ScreenCaptureService::default();
            let image = platform::capture_image(&capture_service, &selection, &cached_screens)?;
            let state = app_handle.state::<AppState>();
            platform::finalize_capture_session(
                &app_handle,
                &state,
                should_restore_previous_frontmost,
            );

            let id = Uuid::new_v4().to_string();
            let created_at = Utc::now();
            let path = screenshot_dir.join(format!(
                "{}-{}.png",
                created_at.format("%Y%m%d-%H%M%S"),
                &id[..8]
            ));

            // Persist enough metadata for history browsing without keeping image bytes in memory.
            let record = CaptureRecord {
                id,
                created_at,
                width: image.width(),
                height: image.height(),
                path: path.display().to_string(),
            };

            let max_screenshots = state
                .settings
                .lock()
                .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
                .max_screenshots;
            capture_service.save_png(&image, &path)?;
            if let Err(error) = capture_service.copy_to_clipboard(&image) {
                eprintln!("failed to write screenshot to clipboard: {error}");
            }
            history::prune_capture_history(&screenshot_dir, max_screenshots)?;

            let mut history_guard = state
                .history
                .lock()
                .map_err(|_| FlickError::Message("history mutex poisoned".into()))?;
            history_guard.push_front(record.clone());
            history_guard.truncate(max_screenshots as usize);
            drop(history_guard);

            emit_capture_status(&app_handle, "capture-finished", &record);
            // Translate mode extends the plain capture flow instead of branching before capture.
            if intent == CaptureIntent::Translate {
                let ocr = ocr::run_with_service(
                    ocr_service.as_ref(),
                    OcrRequest {
                        image_path: record.path.clone(),
                        language_hint: None,
                    },
                )?;
                let translation = translation::run_with_service(
                    translation_service.as_ref(),
                    TranslateRequest {
                        text: ocr.text.clone(),
                        source_language: None,
                        target_language: "zh".into(),
                    },
                )?;
                translation::emit_translation_ready(
                    &app_handle,
                    &record.path,
                    &ocr.text,
                    translation,
                )?;
            }
            Ok(())
        };

        if let Err(error) = run() {
            let state = app_handle.state::<AppState>();
            platform::restore_after_failed_capture(
                &app_handle,
                &state,
                should_restore_previous_frontmost,
            );
            emit_capture_status(&app_handle, "capture-error", error.to_string());
        }
    });

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

    platform::prepare_for_capture_session(app, state)?;
    platform::start_interactive_capture(app, state)?;
    Ok(())
}
