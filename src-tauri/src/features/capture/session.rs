use std::{
    fs,
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

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
            close_screenshot_editor_window, emit_capture_status, preload_screenshot_editor_window,
            selection_spans_multiple_monitors, show_pinned_image_window,
            show_screenshot_editor_window,
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
    let editor_supported = platform::supports_screenshot_editor_toolbar();
    let (ai_settings, ocr_auto_translate, ocr_target_language, editor_enabled) = {
        let settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::LockError("settings".into()))?;
        (
            settings.ai.clone(),
            settings.ocr_auto_translate,
            settings.ocr_target_language.clone(),
            intent == CaptureIntent::Capture
                && settings.screenshot_editor_toolbar_enabled
                && editor_supported,
        )
    };
    let defer_overlay_hide_for_editor = editor_enabled;
    let cached_screens = platform::complete_ui_before_capture_processing(
        app,
        state,
        !defer_overlay_hide_for_editor,
    )?;

    let app_handle = app.clone();
    let should_restore_previous_frontmost = intent == CaptureIntent::Capture;
    thread::spawn(move || {
        let run = || -> Result<(), FlickError> {
            let capture_service = ScreenCaptureService::default();
            let image = platform::capture_image(&capture_service, &selection, &cached_screens)?;

            let state = app_handle.state::<AppState>();
            if !defer_overlay_hide_for_editor {
                platform::finalize_capture_session(
                    &app_handle,
                    &state,
                    should_restore_previous_frontmost,
                );
            }

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
                if editor_enabled {
                    match create_pending_capture_edit(
                        &app_handle,
                        &state,
                        &image,
                        record.clone(),
                        selection.clone(),
                    ) {
                        Ok(()) => {
                            return Ok(());
                        }
                        Err(error) => {
                            eprintln!(
                                "failed to open screenshot editor; saving original image: {error}"
                            );
                        }
                    }
                }

                if defer_overlay_hide_for_editor {
                    platform::finalize_capture_session(
                        &app_handle,
                        &state,
                        should_restore_previous_frontmost,
                    );
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
        let session = pending
            .get(&session_id)
            .ok_or_else(|| FlickError::Message("pending capture edit not found".into()))?;
        if session.cancelled {
            return Err(FlickError::Message("pending capture edit cancelled".into()));
        }
        session.original_path.clone()
    };

    wait_for_file(Path::new(&original_path), Duration::from_secs(3))?;
    let bytes = fs::read(original_path)
        .map_err(|error| FlickError::Message(format!("failed to read pending image: {error}")))?;
    let data_url = format!(
        "data:image/png;base64,{}",
        general_purpose::STANDARD.encode(bytes)
    );
    Ok(data_url)
}

pub fn confirm_regular_capture_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    png_base64: String,
    pin_to_desktop: bool,
) -> Result<CaptureRecord, FlickError> {
    platform::finalize_capture_session(&app, &state, true);
    let pending = remove_pending_capture_edit(&state, &session_id)?;
    let record = CaptureRecord {
        id: pending.id.clone(),
        created_at: pending.created_at,
        width: pending.selection.width,
        height: pending.selection.height,
        path: pending.final_path.clone(),
    };
    close_screenshot_editor_window(&app, &session_id);
    cleanup_pending_original(&pending);

    let app_for_save = app.clone();
    let record_for_return = record.clone();
    thread::spawn(move || {
        let run = || -> Result<(), FlickError> {
            let image_bytes = decode_png_base64(&png_base64)?;
            let image = image::load_from_memory(&image_bytes)
                .map_err(|error| {
                    FlickError::Message(format!("failed to decode edited image: {error}"))
                })?
                .to_rgba8();

            let state = app_for_save.state::<AppState>();
            let screenshot_dir = history::current_screenshot_dir(&state)?;
            let max_screenshots = state
                .settings
                .lock()
                .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
                .max_screenshots;
            let record = CaptureRecord {
                id: record_for_return.id.clone(),
                created_at: record_for_return.created_at,
                width: image.width(),
                height: image.height(),
                path: record_for_return.path.clone(),
            };

            finalize_regular_capture_image(
                &app_for_save,
                &state,
                &screenshot_dir,
                &image,
                record.clone(),
                max_screenshots,
            )?;
            if pin_to_desktop {
                if let Err(error) = show_pinned_image_window(
                    &app_for_save,
                    &record.path,
                    image.width(),
                    image.height(),
                ) {
                    eprintln!("failed to show pinned image window: {error}");
                }
            }
            Ok(())
        };

        if let Err(error) = run() {
            eprintln!("capture edit background save failed: {error}");
        }
    });

    Ok(record)
}

pub fn save_regular_capture_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    png_base64: String,
) -> Result<CaptureRecord, FlickError> {
    platform::finalize_capture_session(&app, &state, true);
    let pending = remove_pending_capture_edit(&state, &session_id)?;
    close_screenshot_editor_window(&app, &session_id);
    cleanup_pending_original(&pending);

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
        id: pending.id,
        created_at: pending.created_at,
        width: image.width(),
        height: image.height(),
        path: pending.final_path,
    };

    ScreenCaptureService::default().save_png(&image, Path::new(&record.path))?;
    history::prune_capture_history(&screenshot_dir, max_screenshots)?;

    let mut history_guard = state
        .history
        .lock()
        .map_err(|_| FlickError::Message("history mutex poisoned".into()))?;
    history_guard.push_front(record.clone());
    history_guard.truncate(max_screenshots as usize);
    drop(history_guard);

    emit_capture_status(&app, "capture-finished", &record);
    Ok(record)
}

pub fn cancel_capture_edit(
    app: &AppHandle,
    state: &AppState,
    session_id: &str,
) -> Result<(), FlickError> {
    let tauri_state = app.state::<AppState>();
    platform::finalize_capture_session(app, &tauri_state, true);
    {
        let mut pending = state
            .pending_capture_edits
            .lock()
            .map_err(|_| FlickError::Message("pending capture edits mutex poisoned".into()))?;
        if let Some(session) = pending.get_mut(session_id) {
            session.cancelled = true;
            session.overlay_finalized = true;
            cleanup_pending_original(session);
        }
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

pub fn capture_editor_ready(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    let should_finalize = {
        let mut pending = state
            .pending_capture_edits
            .lock()
            .map_err(|_| FlickError::Message("pending capture edits mutex poisoned".into()))?;
        if let Some(session) = pending.get_mut(&session_id) {
            let should_finalize = !session.cancelled
                && !session.overlay_finalized
                && !session.keep_overlay_until_finish;
            if should_finalize {
                session.overlay_finalized = true;
            }
            should_finalize
        } else {
            false
        }
    };
    if should_finalize {
        platform::finalize_capture_session(&app, &state, true);
    }
    Ok(())
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
    let writing_path = pending_dir.join(format!("{}.writing.png", record.id));
    let image_for_save = image.clone();
    let original_path_for_save = original_path.clone();
    let writing_path_for_save = writing_path.clone();
    let save_handle = thread::spawn(move || {
        let result = ScreenCaptureService::default()
            .save_png(&image_for_save, &writing_path_for_save)
            .and_then(|_| {
                fs::rename(&writing_path_for_save, &original_path_for_save)
                    .map_err(anyhow::Error::from)
            });
        result
    });

    let keep_overlay_until_finish = platform::keep_native_overlay_until_editor_finish();
    if selection_spans_multiple_monitors(app, &selection) {}
    let pending = PendingCaptureEdit {
        id: record.id.clone(),
        created_at: record.created_at,
        original_path: original_path.display().to_string(),
        final_path: record.path.clone(),
        selection: selection.clone(),
        cancelled: false,
        overlay_finalized: false,
        keep_overlay_until_finish,
    };

    {
        let mut sessions = state
            .pending_capture_edits
            .lock()
            .map_err(|_| FlickError::Message("pending capture edits mutex poisoned".into()))?;
        sessions.insert(record.id.clone(), pending);
    }
    start_capture_edit_escape_watcher(app.clone(), record.id.clone());
    start_capture_edit_finalize_fallback(app.clone(), record.id.clone());

    let editor_color = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .screenshot_editor_color
        .clone();

    let show_result = show_screenshot_editor_window(
        app,
        &record.id,
        &selection,
        image.width(),
        image.height(),
        &editor_color,
    );
    let save_result = save_handle
        .join()
        .map_err(|_| FlickError::Message("temp png save thread panicked".into()))?;

    if let Err(error) = show_result {
        let _ = remove_pending_capture_edit_by_state(state, &record.id).map(|pending| {
            cleanup_pending_original(&pending);
        });
        return Err(FlickError::Message(format!(
            "failed to open screenshot editor: {error}"
        )));
    }
    if let Err(error) = save_result {
        let _ = fs::remove_file(&writing_path);
        let _ = remove_pending_capture_edit_by_state(state, &record.id).map(|pending| {
            cleanup_pending_original(&pending);
        });
        close_screenshot_editor_window(app, &record.id);
        return Err(FlickError::Message(format!(
            "failed to save pending screenshot: {error}"
        )));
    }
    let cancelled_session = {
        let mut sessions = state
            .pending_capture_edits
            .lock()
            .map_err(|_| FlickError::Message("pending capture edits mutex poisoned".into()))?;
        if sessions
            .get(&record.id)
            .map(|pending| pending.cancelled)
            .unwrap_or(false)
        {
            sessions.remove(&record.id)
        } else {
            None
        }
    };
    if let Some(cancelled) = cancelled_session {
        cleanup_pending_original(&cancelled);
        return Ok(());
    }
    Ok(())
}

fn start_capture_edit_finalize_fallback(app: AppHandle, session_id: String) {
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(8));
        let state = app.state::<AppState>();
        let should_finalize = state
            .pending_capture_edits
            .lock()
            .map(|mut sessions| {
                if let Some(session) = sessions.get_mut(&session_id) {
                    let should_finalize = !session.cancelled
                        && !session.overlay_finalized
                        && !session.keep_overlay_until_finish;
                    if should_finalize {
                        session.overlay_finalized = true;
                    }
                    should_finalize
                } else {
                    false
                }
            })
            .unwrap_or(false);
        if should_finalize {
            platform::finalize_capture_session(&app, &state, true);
        }
    });
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

fn wait_for_file(path: &Path, timeout: Duration) -> Result<(), FlickError> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.is_file() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(8));
    }
    Err(FlickError::Message(format!(
        "pending capture image was not ready: {}",
        path.display()
    )))
}

pub(crate) fn remove_pending_capture_edit(
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

pub(crate) fn cleanup_pending_original(pending: &PendingCaptureEdit) {
    match fs::remove_file(&pending.original_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => eprintln!("failed to remove pending capture edit image: {error}"),
    }
    let writing_path = pending
        .original_path
        .strip_suffix(".png")
        .map(|path| format!("{path}.writing.png"))
        .unwrap_or_else(|| format!("{}.writing", pending.original_path));
    let _ = fs::remove_file(writing_path);
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

    let should_preload_editor = {
        let settings = state
            .settings
            .lock()
            .map_err(|_| FlickError::LockError("settings".into()))?;
        intent == CaptureIntent::Capture
            && settings.screenshot_editor_toolbar_enabled
            && platform::supports_screenshot_editor_toolbar()
    };
    if should_preload_editor {
        if let Err(error) = preload_screenshot_editor_window(app) {
            eprintln!("failed to preload screenshot editor window: {error}");
        }
    }

    platform::prepare_for_capture_session(app, state)?;
    platform::start_interactive_capture(app, state)?;
    Ok(())
}
