//! Wayland-specific capture-session behavior backed by xdg-desktop-portal.

use std::{path::PathBuf, thread};

use ashpd::{
    Error as AshpdError,
    desktop::{ResponseError, screenshot::Screenshot},
};
use async_std::task;
use tauri::{AppHandle, Manager, State};

use crate::{
    app::{AppState, windows::emit_capture_status},
    error::FlickError,
    models::SelectionRect,
    services::CachedScreenCapture,
};

pub fn begin_interactive_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    clear_cached_snapshot(state)?;

    let app_handle = app.clone();
    thread::spawn(move || {
        let run = || -> Result<(), FlickError> {
            let screenshot_path = task::block_on(capture_via_portal())?;
            let image = image::open(&screenshot_path)
                .map_err(|error| {
                    FlickError::Message(format!("failed to read portal screenshot: {error}"))
                })?
                .into_rgba8();
            let selection = SelectionRect {
                x: 0,
                y: 0,
                width: image.width(),
                height: image.height(),
            };

            {
                let state = app_handle.state::<AppState>();
                let mut snapshots = state
                    .capture_snapshots
                    .lock()
                    .map_err(|_| FlickError::Message("capture snapshot mutex poisoned".into()))?;
                snapshots.clear();
                snapshots.push(CachedScreenCapture::new(selection.clone(), image));
            }

            let state = app_handle.state::<AppState>();
            crate::features::capture::complete_capture(&app_handle, &state, selection)
        };

        match run() {
            Ok(()) => {}
            Err(FlickError::Message(message)) if message == "cancelled" => {
                let _ = crate::features::capture::cancel_capture(&app_handle);
            }
            Err(error) => {
                emit_capture_status(&app_handle, "capture-error", error.to_string());
                let _ = crate::features::capture::cancel_capture(&app_handle);
            }
        }
    });

    Ok(())
}

pub fn cancel_interactive_capture_session(_app: &AppHandle, state: &State<'_, AppState>) {
    let _ = clear_cached_snapshot(state);
}

pub fn prepare_for_capture_session(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none()
        && !matches!(
            std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
            Some("wayland")
        )
    {
        return Err(FlickError::Message(
            "Wayland capture requested without an active Wayland session".into(),
        ));
    }
    Ok(())
}

pub fn complete_ui_before_capture_processing(
    _app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<Vec<CachedScreenCapture>, FlickError> {
    let mut guard = state
        .capture_snapshots
        .lock()
        .map_err(|_| FlickError::Message("capture snapshot mutex poisoned".into()))?;
    Ok(std::mem::take(&mut *guard))
}

pub fn finalize_capture_session(
    _app: &AppHandle,
    state: &State<'_, AppState>,
    _restore_previous_frontmost: bool,
) {
    let _ = clear_cached_snapshot(state);
}

pub fn restore_after_failed_capture(
    _app: &AppHandle,
    state: &State<'_, AppState>,
    _restore_previous_frontmost: bool,
) {
    let _ = clear_cached_snapshot(state);
}

pub fn cleanup_after_cancel(_app: &AppHandle, state: &State<'_, AppState>) {
    let _ = clear_cached_snapshot(state);
}

async fn capture_via_portal() -> Result<PathBuf, FlickError> {
    let response = Screenshot::request()
        .interactive(true)
        .modal(true)
        .send()
        .await
        .map_err(|error| {
            FlickError::Message(format!("failed to request portal screenshot: {error}"))
        })?
        .response()
        .map_err(|error| match error {
            AshpdError::Response(ResponseError::Cancelled) => {
                FlickError::Message("cancelled".into())
            }
            AshpdError::Response(ResponseError::Other) => {
                FlickError::Message("portal screenshot failed".into())
            }
            other => FlickError::Message(format!("failed to resolve portal screenshot: {other}")),
        })?;

    response
        .uri()
        .to_file_path()
        .map_err(|_| FlickError::Message("portal screenshot did not return a local file".into()))
}

fn clear_cached_snapshot(state: &State<'_, AppState>) -> Result<(), FlickError> {
    state
        .capture_snapshots
        .lock()
        .map_err(|_| FlickError::Message("capture snapshot mutex poisoned".into()))?
        .clear();
    Ok(())
}
