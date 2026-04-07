//! Windows-specific capture-session behavior.

use std::{
    process::Command,
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use arboard::Clipboard;
use image::{ImageBuffer, Rgba};
use tauri::{AppHandle, Manager, State};
use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;

use crate::{
    app::{AppState, windows::emit_capture_status},
    error::FlickError,
    models::SelectionRect,
    services::CachedScreenCapture,
};

const CLIPBOARD_POLL_INTERVAL: Duration = Duration::from_millis(120);
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
struct WindowsCaptureRuntime {
    active_session_id: Option<u64>,
    next_session_id: u64,
}

fn capture_runtime() -> &'static Mutex<WindowsCaptureRuntime> {
    static RUNTIME: OnceLock<Mutex<WindowsCaptureRuntime>> = OnceLock::new();
    RUNTIME.get_or_init(|| Mutex::new(WindowsCaptureRuntime::default()))
}

pub fn begin_interactive_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    let session_id = {
        let mut runtime = capture_runtime()
            .lock()
            .map_err(|_| FlickError::Message("windows capture runtime mutex poisoned".into()))?;
        if runtime.active_session_id.is_some() {
            return Err(FlickError::Message("capture session already active".into()));
        }
        runtime.next_session_id += 1;
        runtime.active_session_id = Some(runtime.next_session_id);
        runtime.next_session_id
    };

    {
        let mut snapshots = state
            .capture_snapshots
            .lock()
            .map_err(|_| FlickError::Message("capture snapshot mutex poisoned".into()))?;
        snapshots.clear();
    }

    let initial_sequence = clipboard_sequence_number();
    launch_system_snipping_ui()?;

    let app_handle = app.clone();
    thread::spawn(move || watch_clipboard_for_capture(app_handle, session_id, initial_sequence));

    Ok(())
}

pub fn cancel_interactive_capture_session(_app: &AppHandle, _state: &State<'_, AppState>) {
    clear_active_session();
}

pub fn prepare_for_capture_session(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    Ok(())
}

pub fn complete_ui_before_capture_processing(
    _app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<Vec<CachedScreenCapture>, FlickError> {
    clear_active_session();
    let mut snapshots = state
        .capture_snapshots
        .lock()
        .map_err(|_| FlickError::Message("capture snapshot mutex poisoned".into()))?;
    Ok(std::mem::take(&mut *snapshots))
}

pub fn finalize_capture_session(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
    _restore_previous_frontmost: bool,
) {
}

pub fn restore_after_failed_capture(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
    _restore_previous_frontmost: bool,
) {
    clear_active_session();
}

pub fn cleanup_after_cancel(_app: &AppHandle, state: &State<'_, AppState>) {
    clear_active_session();
    if let Ok(mut snapshots) = state.capture_snapshots.lock() {
        snapshots.clear();
    }
}

fn watch_clipboard_for_capture(app: AppHandle, session_id: u64, initial_sequence: u32) {
    let deadline = Instant::now() + CAPTURE_TIMEOUT;

    while session_is_active(session_id) && Instant::now() < deadline {
        let current_sequence = clipboard_sequence_number();
        if current_sequence != initial_sequence {
            if let Some(image) = clipboard_image() {
                if let Err(error) = cache_clipboard_capture(&app, &image) {
                    clear_active_session();
                    emit_capture_status(&app, "capture-error", error.to_string());
                    return;
                }

                let selection = SelectionRect {
                    x: 0,
                    y: 0,
                    width: image.width(),
                    height: image.height(),
                };
                let state = app.state::<AppState>();
                if let Err(error) =
                    crate::features::capture::complete_capture(&app, &state, selection)
                {
                    clear_active_session();
                    emit_capture_status(&app, "capture-error", error.to_string());
                }
                return;
            }
        }

        thread::sleep(CLIPBOARD_POLL_INTERVAL);
    }

    if session_is_active(session_id) {
        let _ = crate::features::capture::cancel_capture(&app);
    }
}

fn launch_system_snipping_ui() -> Result<(), FlickError> {
    Command::new("explorer.exe")
        .arg("ms-screenclip:")
        .spawn()
        .map(|_| ())
        .map_err(|error| {
            FlickError::Message(format!("failed to launch Windows snipping UI: {error}"))
        })
}

fn clipboard_sequence_number() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

fn clipboard_image() -> Option<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let mut clipboard = Clipboard::new().ok()?;
    let image = clipboard.get_image().ok()?;
    let width = u32::try_from(image.width).ok()?;
    let height = u32::try_from(image.height).ok()?;
    let bytes = image.bytes.into_owned();
    ImageBuffer::from_vec(width, height, bytes)
}

fn cache_clipboard_capture(
    app: &AppHandle,
    image: &ImageBuffer<Rgba<u8>, Vec<u8>>,
) -> Result<(), FlickError> {
    let snapshot = CachedScreenCapture::new(
        SelectionRect {
            x: 0,
            y: 0,
            width: image.width(),
            height: image.height(),
        },
        image.clone(),
    );
    let state = app.state::<AppState>();
    let mut snapshots = state
        .capture_snapshots
        .lock()
        .map_err(|_| FlickError::Message("capture snapshot mutex poisoned".into()))?;
    snapshots.clear();
    snapshots.push(snapshot);
    Ok(())
}

fn session_is_active(session_id: u64) -> bool {
    capture_runtime()
        .lock()
        .map(|runtime| runtime.active_session_id == Some(session_id))
        .unwrap_or(false)
}

fn clear_active_session() {
    if let Ok(mut runtime) = capture_runtime().lock() {
        runtime.active_session_id = None;
    }
}
