use std::{fs, path::Path, sync::Arc, thread};

use base64::{Engine as _, engine::general_purpose};
use chrono::Utc;
use image::{ImageBuffer, Rgba};
use tauri::{AppHandle, Manager, State};
use tokio::runtime::Runtime;
use uuid::Uuid;

use crate::{
    app::{
        AppState, CaptureIntent,
        windows::{
            close_screenshot_editor_window, emit_capture_status, show_screenshot_editor_window,
        },
    },
    error::FlickError,
    features::translation,
    models::{CaptureRecord, PendingCaptureEdit, SelectionRect, TranslateRequest},
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
                                translation::mark_window_translation_failed(&app_handle, &e);
                                return Err(e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("OCR failed: {}", e);
                        translation::mark_window_translation_failed(&app_handle, &e);
                        return Err(e.into());
                    }
                }
            } else {
                let editor_enabled = state
                    .settings
                    .lock()
                    .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
                    .screenshot_editor_toolbar_enabled;

                if editor_enabled {
                    match create_pending_capture_edit(
                        &app_handle,
                        &state,
                        &image,
                        record.clone(),
                        selection.clone(),
                    ) {
                        Ok(()) => return Ok(()),
                        Err(error) => {
                            eprintln!(
                                "failed to open screenshot editor; saving original image: {error}"
                            );
                        }
                    }
                }

                finalize_regular_capture_image(
                    &app_handle,
                    &state,
                    &screenshot_dir,
                    &image,
                    record,
                    max_screenshots,
                )?;
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

pub fn get_pending_capture_image(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, FlickError> {
    let original_path = {
        let pending = state
            .pending_capture_edits
            .lock()
            .map_err(|_| FlickError::Message("pending capture edits mutex poisoned".into()))?;
        pending
            .get(&session_id)
            .map(|session| session.original_path.clone())
            .ok_or_else(|| FlickError::Message("pending capture edit not found".into()))?
    };

    let bytes = fs::read(original_path)
        .map_err(|error| FlickError::Message(format!("failed to read pending image: {error}")))?;
    Ok(format!(
        "data:image/png;base64,{}",
        general_purpose::STANDARD.encode(bytes)
    ))
}

pub fn confirm_regular_capture_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    png_base64: String,
) -> Result<CaptureRecord, FlickError> {
    let pending = remove_pending_capture_edit(&state, &session_id)?;
    let image_bytes = decode_png_base64(&png_base64)?;
    let image = image::load_from_memory(&image_bytes)
        .map_err(|error| FlickError::Message(format!("failed to decode edited image: {error}")))?
        .to_rgba8();

    let screenshot_dir = history::current_screenshot_dir(&state)?;
    let max_screenshots = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .max_screenshots;
    let record = CaptureRecord {
        id: pending.id.clone(),
        created_at: pending.created_at,
        width: image.width(),
        height: image.height(),
        path: pending.final_path.clone(),
    };

    let result = finalize_regular_capture_image(
        &app,
        &state,
        &screenshot_dir,
        &image,
        record,
        max_screenshots,
    );

    cleanup_pending_original(&pending);
    if result.is_ok() {
        close_screenshot_editor_window(&app, &session_id);
    }
    result
}

pub fn cancel_capture_edit(
    app: &AppHandle,
    state: &AppState,
    session_id: &str,
) -> Result<(), FlickError> {
    match remove_pending_capture_edit_by_state(state, session_id) {
        Ok(pending) => cleanup_pending_original(&pending),
        Err(FlickError::Message(message)) if message == "pending capture edit not found" => {}
        Err(error) => return Err(error),
    }
    close_screenshot_editor_window(app, session_id);
    Ok(())
}

pub fn cancel_capture_edit_command(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    cancel_capture_edit(&app, &state, &session_id)
}

fn create_pending_capture_edit(
    app: &AppHandle,
    state: &AppState,
    image: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    record: CaptureRecord,
    selection: SelectionRect,
) -> Result<(), FlickError> {
    let pending_dir = state.data_dir.join("pending-capture-edits");
    fs::create_dir_all(&pending_dir).map_err(|error| {
        FlickError::Message(format!(
            "failed to create pending capture edit directory: {error}"
        ))
    })?;
    let original_path = pending_dir.join(format!("{}.png", record.id));
    ScreenCaptureService::default().save_png(image, &original_path)?;

    let pending = PendingCaptureEdit {
        id: record.id.clone(),
        created_at: record.created_at,
        original_path: original_path.display().to_string(),
        final_path: record.path.clone(),
        selection: selection.clone(),
    };

    {
        let mut sessions = state
            .pending_capture_edits
            .lock()
            .map_err(|_| FlickError::Message("pending capture edits mutex poisoned".into()))?;
        sessions.insert(record.id.clone(), pending);
    }

    show_screenshot_editor_window(app, &record.id, &selection, image.width(), image.height())
        .map_err(|error| {
            let _ = remove_pending_capture_edit_by_state(state, &record.id).map(|pending| {
                cleanup_pending_original(&pending);
            });
            FlickError::Message(format!("failed to open screenshot editor: {error}"))
        })?;
    start_capture_edit_escape_watcher(app.clone(), record.id.clone());

    Ok(())
}

fn start_capture_edit_escape_watcher(app: AppHandle, session_id: String) {
    thread::spawn(move || {
        let mut was_pressed = false;
        loop {
            let still_pending = app
                .try_state::<AppState>()
                .and_then(|state| {
                    state
                        .pending_capture_edits
                        .lock()
                        .ok()
                        .map(|pending| pending.contains_key(&session_id))
                })
                .unwrap_or(false);
            if !still_pending {
                break;
            }

            let pressed = platform::editor_escape_key_pressed();
            if pressed && !was_pressed {
                if let Some(state) = app.try_state::<AppState>() {
                    let _ = cancel_capture_edit(&app, &state, &session_id);
                }
                break;
            }
            was_pressed = pressed;
            thread::sleep(std::time::Duration::from_millis(16));
        }
    });
}

fn finalize_regular_capture_image(
    app: &AppHandle,
    state: &AppState,
    screenshot_dir: &Path,
    image: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    record: CaptureRecord,
    max_screenshots: u32,
) -> Result<CaptureRecord, FlickError> {
    let capture_service = ScreenCaptureService::default();
    capture_service.save_png(image, Path::new(&record.path))?;

    if let Err(error) = capture_service.copy_to_clipboard(image) {
        eprintln!("failed to write screenshot to clipboard: {error}");
    }
    history::prune_capture_history(screenshot_dir, max_screenshots)?;

    let mut history_guard = state
        .history
        .lock()
        .map_err(|_| FlickError::Message("history mutex poisoned".into()))?;
    history_guard.push_front(record.clone());
    history_guard.truncate(max_screenshots as usize);
    drop(history_guard);

    emit_capture_status(app, "capture-finished", &record);
    Ok(record)
}

fn decode_png_base64(value: &str) -> Result<Vec<u8>, FlickError> {
    let payload = value
        .split_once(',')
        .map(|(_, payload)| payload)
        .unwrap_or(value);
    general_purpose::STANDARD
        .decode(payload)
        .map_err(|error| FlickError::Message(format!("failed to decode edited PNG: {error}")))
}

fn remove_pending_capture_edit(
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<PendingCaptureEdit, FlickError> {
    remove_pending_capture_edit_by_state(&state, session_id)
}

fn remove_pending_capture_edit_by_state(
    state: &AppState,
    session_id: &str,
) -> Result<PendingCaptureEdit, FlickError> {
    state
        .pending_capture_edits
        .lock()
        .map_err(|_| FlickError::Message("pending capture edits mutex poisoned".into()))?
        .remove(session_id)
        .ok_or_else(|| FlickError::Message("pending capture edit not found".into()))
}

fn cleanup_pending_original(pending: &PendingCaptureEdit) {
    if let Err(error) = fs::remove_file(&pending.original_path) {
        eprintln!("failed to remove pending capture edit image: {error}");
    }
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
