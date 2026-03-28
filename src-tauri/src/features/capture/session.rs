use std::{sync::MutexGuard, thread};

use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::{
    app::{
        AppState, CaptureIntent,
        windows::{
            capture_window_label, emit_capture_status, ensure_capture_window,
            is_capture_window_label,
        },
    },
    error::FlickError,
    features::{ocr, translation},
    models::{
        CaptureContext, CaptureRecord, CursorPosition, OcrRequest, SelectionRect, TranslateRequest,
    },
    services::ScreenCaptureService,
};

use super::{history, platform};

pub fn focus_capture_window(app: &AppHandle, label: &str) -> Result<(), FlickError> {
    let mut target_window = None;
    for (_, window) in app
        .webview_windows()
        .into_iter()
        .filter(|(window_label, _)| is_capture_window_label(window_label))
    {
        let is_target = window.label() == label;
        let _ = window.set_focusable(is_target);
        if is_target {
            target_window = Some(window);
        }
    }

    let window = target_window
        .ok_or_else(|| FlickError::Message(format!("capture window not found: {label}")))?;
    window.set_focus()?;
    Ok(())
}

pub fn get_global_cursor_position(app: &AppHandle) -> Result<CursorPosition, FlickError> {
    platform::current_global_cursor_position(app)
}

pub fn cancel_capture(app: &AppHandle) -> Result<(), FlickError> {
    emit_capture_event_to_windows(app, "capture-ended", "cancelled");
    for (_, window) in app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| is_capture_window_label(label))
    {
        window.hide()?;
    }

    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut guard) = state.capture_snapshots.lock() {
            guard.clear();
        }
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
    thread::spawn(move || {
        let run = || -> Result<(), FlickError> {
            let capture_service = ScreenCaptureService::default();
            let image = platform::capture_image(&capture_service, &selection, &cached_screens)?;
            capture_service.copy_to_clipboard(&image)?;

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
            history::prune_capture_history(&screenshot_dir, max_screenshots)?;

            let mut history_guard = state
                .history
                .lock()
                .map_err(|_| FlickError::Message("history mutex poisoned".into()))?;
            history_guard.push_front(record.clone());
            history_guard.truncate(max_screenshots as usize);
            drop(history_guard);

            emit_capture_status(&app_handle, "capture-finished", &record);
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

pub fn refresh_capture_context(
    app: &AppHandle,
    state: &State<'_, AppState>,
    label: &str,
) -> Result<CaptureContext, FlickError> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| FlickError::Message(format!("capture window not found: {label}")))?;
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
        .capture_contexts
        .lock()
        .map_err(|_| FlickError::Message("capture contexts mutex poisoned".into()))?;
    guard.insert(label.to_string(), context.clone());

    Ok(context)
}

pub fn get_capture_context(
    state: &State<'_, AppState>,
    label: &str,
) -> Result<CaptureContext, FlickError> {
    state
        .capture_contexts
        .lock()
        .map_err(|_| FlickError::Message("capture contexts mutex poisoned".into()))?
        .get(label)
        .cloned()
        .ok_or_else(|| FlickError::Message(format!("capture context not found for {label}")))
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
    prepare_capture_context(app, state)?;
    open_capture_overlay(app)?;
    Ok(())
}

fn prepare_capture_context(app: &AppHandle, state: &State<'_, AppState>) -> Result<(), FlickError> {
    let monitors = app.available_monitors()?;
    let mut contexts = Vec::with_capacity(monitors.len());

    for (index, monitor) in monitors.iter().enumerate() {
        let scale = monitor.scale_factor();
        let x = monitor.position().x as f64 / scale;
        let y = monitor.position().y as f64 / scale;
        let width = monitor.size().width as f64 / scale;
        let height = monitor.size().height as f64 / scale;
        let label = capture_window_label(index);
        let context = CaptureContext {
            x,
            y,
            width,
            height,
        };

        let window = ensure_capture_window(app, &label)?;
        window.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(x, y)))?;
        window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(width, height)))?;
        window.set_always_on_top(true)?;
        contexts.push((label, context));
    }

    {
        let mut guard = lock_capture_contexts(state)?;
        guard.clear();
        for (label, context) in &contexts {
            guard.insert(label.clone(), context.clone());
        }
    }

    let active_labels = contexts
        .into_iter()
        .map(|(label, _)| label)
        .collect::<Vec<_>>();

    for (label, window) in app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| is_capture_window_label(label))
    {
        if !active_labels.iter().any(|active| active == &label) {
            let _ = window.close();
        }
    }

    Ok(())
}

fn open_capture_overlay(app: &AppHandle) -> Result<(), FlickError> {
    let mut capture_windows = app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| is_capture_window_label(label))
        .map(|(_, window)| window)
        .collect::<Vec<_>>();

    capture_windows.sort_by(|a, b| a.label().cmp(&b.label()));
    let mut focus_label = capture_windows
        .first()
        .map(|window| window.label().to_string());
    if let Ok(cursor) = get_global_cursor_position(app) {
        if let Some((label, _)) = app
            .state::<AppState>()
            .capture_contexts
            .lock()
            .map_err(|_| FlickError::Message("capture contexts mutex poisoned".into()))?
            .iter()
            .find(|(_, context)| {
                cursor.x >= context.x
                    && cursor.x <= context.x + context.width
                    && cursor.y >= context.y
                    && cursor.y <= context.y + context.height
            })
        {
            focus_label = Some(label.clone());
        }
    }

    if let Some(label) = focus_label.clone() {
        capture_windows.sort_by_key(|window| if window.label() == label { 0 } else { 1 });
    }

    if let Some(target_label) = focus_label.clone() {
        for window in &capture_windows {
            let should_focus = window.label() == target_label;
            let _ = window.set_focusable(should_focus);
        }
    }

    if let Some(target_window) = capture_windows.first() {
        target_window.show()?;
        target_window.set_focus()?;
    }

    for window in capture_windows.iter().skip(1) {
        window.show()?;
    }

    if let Some(label) = focus_label {
        if let Some(window) = capture_windows
            .iter()
            .find(|window| window.label() == label)
        {
            window.set_focus()?;
        }
    }
    thread::sleep(std::time::Duration::from_millis(16));
    emit_capture_event_to_windows(app, "capture-started", "started");
    let app_handle = app.clone();
    thread::spawn(move || {
        thread::sleep(std::time::Duration::from_millis(120));
        emit_capture_event_to_windows(&app_handle, "capture-started", "started");
    });
    Ok(())
}

pub(crate) fn emit_capture_event_to_windows(
    app: &AppHandle,
    event: &str,
    payload: impl serde::Serialize + Clone,
) {
    for (_, window) in app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| is_capture_window_label(label))
    {
        let _ = window.emit(event, payload.clone());
    }
}

fn lock_capture_contexts<'a>(
    state: &'a State<'_, AppState>,
) -> Result<MutexGuard<'a, crate::models::CaptureContexts>, FlickError> {
    state
        .capture_contexts
        .lock()
        .map_err(|_| FlickError::Message("capture contexts mutex poisoned".into()))
}
