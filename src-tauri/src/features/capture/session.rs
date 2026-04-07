use std::{sync::Arc, thread};

use chrono::Utc;
use tauri::{AppHandle, Manager, State};
use tokio::runtime::Runtime;
use uuid::Uuid;

use crate::{
    app::{AppState, CaptureIntent, windows::emit_capture_status},
    error::FlickError,
    features::translation,
    models::{CaptureRecord, SelectionRect, TranslateRequest},
    services::{OcrService, ScreenCaptureService},
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

    let ocr_service: Arc<dyn OcrService> = state
        .ocr_service
        .lock()
        .map_err(|_| FlickError::Message("ocr service mutex poisoned".into()))?
        .clone();
    let (ai_settings, ocr_auto_translate, ocr_target_language) = {
        let settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::LockError("settings".into()))?;
        (
            settings.ai.clone(),
            settings.ocr_auto_translate,
            settings.ocr_target_language.clone(),
        )
    };
    let cached_screens = platform::complete_ui_before_capture_processing(app, state)?;

    let app_handle = app.clone();
    let should_restore_previous_frontmost = intent == CaptureIntent::Capture;
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

            let record = CaptureRecord {
                id: id.clone(),
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

            if intent == CaptureIntent::Translate {
                if let Err(e) = translation::show_window_immediately(&app_handle, &record.path) {
                    eprintln!("Failed to show window: {}", e);
                }

                let ocr_result = {
                    let mut image_bytes = Vec::new();
                    image
                        .write_to(
                            &mut std::io::Cursor::new(&mut image_bytes),
                            image::ImageFormat::Png,
                        )
                        .map_err(|e| FlickError::Message(format!("failed to encode PNG: {}", e)))?;

                    ocr_service.run_with_data(&image_bytes)
                };

                let save_path = path.clone();
                let save_app = app_handle.clone();
                let save_screenshot_dir = screenshot_dir.clone();
                let save_max_screenshots = max_screenshots;
                let save_record = record.clone();
                thread::spawn(move || {
                    let capture_service = ScreenCaptureService::default();
                    if let Err(e) = capture_service.save_png(&image, &save_path) {
                        eprintln!("Failed to save image: {}", e);
                    }

                    if let Err(error) = capture_service.copy_to_clipboard(&image) {
                        eprintln!("failed to write screenshot to clipboard: {error}");
                    }

                    if let Err(e) =
                        history::prune_capture_history(&save_screenshot_dir, save_max_screenshots)
                    {
                        eprintln!("Failed to prune history: {}", e);
                    }

                    let state = save_app.state::<AppState>();
                    if let Ok(mut history_guard) = state.history.lock() {
                        history_guard.push_front(save_record.clone());
                        history_guard.truncate(save_max_screenshots as usize);
                    }

                    emit_capture_status(&save_app, "capture-finished", &save_record);
                });

                match ocr_result {
                    Ok(ocr) => {
                        let detected_source_language = translation::detect_text_language(&ocr.text);
                        let has_ocr_text = !ocr.text.trim().is_empty();
                        let should_auto_translate = ocr_auto_translate
                            && has_ocr_text
                            && translation::has_active_ai_provider(&ai_settings);
                        translation::emit_ocr_ready(
                            &app_handle,
                            &record.path,
                            &ocr.text,
                            detected_source_language.as_deref(),
                            should_auto_translate,
                            &ocr_target_language,
                        )?;

                        if !has_ocr_text {
                            eprintln!(
                                "OCR completed but returned empty text; skipping translation"
                            );
                            return Ok(());
                        }

                        if !should_auto_translate {
                            return Ok(());
                        }

                        let rt = Runtime::new().map_err(|e| {
                            FlickError::Message(format!("failed to create tokio runtime: {}", e))
                        })?;
                        let pipeline = translation::TranslationPipeline::new(TranslateRequest {
                            text: ocr.text.clone(),
                            source_language: detected_source_language.clone(),
                            target_language: ocr_target_language.clone(),
                        })
                        .with_image_path(record.path.clone())
                        .prepare();
                        let translation_result = rt.block_on(
                            translation::run_pipeline_with_ai_settings(&ai_settings, &pipeline),
                        );

                        match translation_result {
                            Ok(translation) => {
                                translation::save_pipeline_history(
                                    &app_handle.state::<AppState>(),
                                    &pipeline,
                                    &translation,
                                )?;
                                translation::emit_translation_ready(
                                    &app_handle,
                                    &record.path,
                                    &ocr.text,
                                    &ocr_target_language,
                                    translation,
                                )?;
                            }
                            Err(e) => {
                                eprintln!("translation failed: {}", e);
                                return Err(e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("OCR failed: {}", e);
                        return Err(e.into());
                    }
                }
            } else {
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
            }
            Ok(())
        };

        if let Err(error) = run() {
            eprintln!("capture process failed: {}", error);
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
