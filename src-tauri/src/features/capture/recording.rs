use std::{
    collections::HashMap,
    fs,
    io::BufWriter,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use chrono::Utc;
use image::{ImageBuffer, Rgba, imageops::FilterType};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::{
    app::{AppState, windows},
    error::FlickError,
    models::{CaptureRecord, SelectionRect},
    services::{ScreenCaptureService, screen_capture::LiveFrameStream},
};

use super::{history, platform};

const RECORDING_FPS: u64 = 6;
const RECORDING_FRAME_INTERVAL: Duration = Duration::from_millis(1000 / RECORDING_FPS);
const RECORDING_GIF_DELAY_CS: u16 = (100 / RECORDING_FPS) as u16;
const RECORDING_QUEUE_CAPACITY: usize = 8;
const RECORDING_540P_MAX_GIF_WIDTH: u32 = 960;
const RECORDING_540P_MAX_GIF_HEIGHT: u32 = 540;
const RECORDING_720P_MAX_GIF_WIDTH: u32 = 1280;
const RECORDING_720P_MAX_GIF_HEIGHT: u32 = 720;

enum RecordingMessage {
    Frame(ImageBuffer<Rgba<u8>, Vec<u8>>),
}

struct RecordingSession {
    stream: Option<LiveFrameStream>,
    sender: SyncSender<RecordingMessage>,
    worker: Option<thread::JoinHandle<anyhow::Result<u32>>>,
    paused: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    final_path: PathBuf,
    writing_path: PathBuf,
    width: u32,
    height: u32,
}

static RECORDING_SESSIONS: OnceLock<Mutex<HashMap<String, RecordingSession>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<String, RecordingSession>> {
    RECORDING_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn recording_log(message: impl AsRef<str>) {
    eprintln!("[gif-recording] {}", message.as_ref());
}

pub fn start_gif_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    recording_log(format!("start_gif_recording: enter session={session_id}"));
    let selection = pending_selection(&state, &session_id)?;
    recording_log(format!(
        "start_gif_recording: selection x={} y={} width={} height={}",
        selection.x, selection.y, selection.width, selection.height
    ));
    validate_recording_size(&selection)?;
    finalize_pending_overlay_for_recording(&app, &state, &session_id)?;

    let screenshot_dir = history::current_screenshot_dir(&state)?;
    recording_log(format!(
        "start_gif_recording: screenshot_dir={}",
        screenshot_dir.display()
    ));
    fs::create_dir_all(&screenshot_dir).map_err(|error| {
        FlickError::Message(format!("failed to create screenshot dir: {error}"))
    })?;

    let id = Uuid::new_v4().to_string();
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    let final_path = screenshot_dir.join(format!("recording-{timestamp}-{id}.gif"));
    let writing_path = screenshot_dir.join(format!("recording-{timestamp}-{id}.writing.gif"));
    let (sender, receiver) = mpsc::sync_channel(RECORDING_QUEUE_CAPACITY);
    let worker_path = writing_path.clone();
    let width = selection.width;
    let height = selection.height;
    let (max_gif_width, max_gif_height) = recording_size_limits(&state)?;
    recording_log(format!(
        "start_gif_recording: final_path={} writing_path={} max_gif={}x{}",
        final_path.display(),
        writing_path.display(),
        max_gif_width,
        max_gif_height
    ));
    let worker = thread::spawn(move || {
        encode_gif(
            worker_path,
            width,
            height,
            max_gif_width,
            max_gif_height,
            receiver,
        )
    });

    let paused = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let stream_sender = sender.clone();
    let stream_paused = Arc::clone(&paused);
    let stream_stopped = Arc::clone(&stopped);
    let last_frame_at = Arc::new(Mutex::new(Instant::now() - RECORDING_FRAME_INTERVAL));
    let stream_last_frame_at = Arc::clone(&last_frame_at);
    let delivered_frames = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let queued_frames = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let dropped_frames = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let stream_delivered_frames = Arc::clone(&delivered_frames);
    let stream_queued_frames = Arc::clone(&queued_frames);
    let stream_dropped_frames = Arc::clone(&dropped_frames);
    let on_frame = Box::new(move |frame: ImageBuffer<Rgba<u8>, Vec<u8>>| {
        let delivered = stream_delivered_frames.fetch_add(1, Ordering::SeqCst) + 1;
        if stream_stopped.load(Ordering::SeqCst) || stream_paused.load(Ordering::SeqCst) {
            if delivered <= 3 || delivered % 60 == 0 {
                recording_log(format!(
                    "stream callback: drop delivered={delivered} reason=stopped_or_paused"
                ));
            }
            return;
        }
        let Ok(mut last_frame_at) = stream_last_frame_at.lock() else {
            recording_log("stream callback: drop reason=last_frame_at mutex poisoned");
            return;
        };
        if last_frame_at.elapsed() < RECORDING_FRAME_INTERVAL {
            return;
        }
        *last_frame_at = Instant::now();
        match stream_sender.try_send(RecordingMessage::Frame(frame)) {
            Ok(()) => {
                let queued = stream_queued_frames.fetch_add(1, Ordering::SeqCst) + 1;
                if queued <= 3 || queued % 30 == 0 {
                    recording_log(format!(
                        "stream callback: queued frame delivered={delivered} queued={queued}"
                    ));
                }
            }
            Err(_) => {
                let dropped = stream_dropped_frames.fetch_add(1, Ordering::SeqCst) + 1;
                if dropped <= 3 || dropped % 30 == 0 {
                    recording_log(format!(
                        "stream callback: drop delivered={delivered} dropped={dropped} reason=queue_full_or_closed"
                    ));
                }
            }
        }
    });

    recording_log("start_gif_recording: opening live frame stream");
    let stream = ScreenCaptureService::default()
        .open_live_frame_stream(&selection, on_frame)
        .map_err(|error| {
            FlickError::Message(format!("failed to start screen recording: {error}"))
        })?;
    recording_log("start_gif_recording: live frame stream opened");

    let mut sessions = sessions()
        .lock()
        .map_err(|_| FlickError::Message("recording session mutex poisoned".into()))?;
    if sessions.contains_key(&session_id) {
        recording_log(format!(
            "start_gif_recording: duplicate active session session={session_id}"
        ));
        stopped.store(true, Ordering::SeqCst);
        drop(stream);
        drop(sender);
        return Err(FlickError::Message(
            "recording session already active".into(),
        ));
    }

    sessions.insert(
        session_id.clone(),
        RecordingSession {
            stream: Some(stream),
            sender,
            worker: Some(worker),
            paused,
            stopped,
            final_path,
            writing_path,
            width,
            height,
        },
    );

    recording_log(format!(
        "start_gif_recording: session stored session={session_id}"
    ));
    let _ = app.emit("gif-recording-status", "recording");
    recording_log("start_gif_recording: emitted gif-recording-status=recording");
    Ok(())
}

pub fn pause_gif_recording(session_id: String) -> Result<(), FlickError> {
    recording_log(format!("pause_gif_recording: enter session={session_id}"));
    with_session(&session_id, |session| {
        session.paused.store(true, Ordering::SeqCst);
        recording_log("pause_gif_recording: paused=true");
        Ok(())
    })
}

pub fn resume_gif_recording(session_id: String) -> Result<(), FlickError> {
    recording_log(format!("resume_gif_recording: enter session={session_id}"));
    with_session(&session_id, |session| {
        session.paused.store(false, Ordering::SeqCst);
        recording_log("resume_gif_recording: paused=false");
        Ok(())
    })
}

pub fn finish_gif_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<CaptureRecord, FlickError> {
    recording_log(format!("finish_gif_recording: enter session={session_id}"));
    let mut session = remove_session(&session_id)?;
    recording_log("finish_gif_recording: session removed from registry");
    session.stopped.store(true, Ordering::SeqCst);
    drop(session.stream.take());
    recording_log("finish_gif_recording: live stream dropped");
    drop(session.sender);
    recording_log("finish_gif_recording: sender dropped; encoder will drain queued frames");

    let frame_count = session
        .worker
        .take()
        .ok_or_else(|| FlickError::Message("recording encoder worker is missing".into()))?
        .join()
        .map_err(|_| FlickError::Message("recording encoder worker panicked".into()))?
        .map_err(|error| FlickError::Message(format!("failed to encode gif: {error}")))?;
    recording_log(format!(
        "finish_gif_recording: encoder joined frame_count={frame_count}"
    ));

    if frame_count == 0 {
        recording_log("finish_gif_recording: no frames recorded; removing writing file");
        let _ = fs::remove_file(&session.writing_path);
        return Err(FlickError::Message("no frames were recorded".into()));
    }

    recording_log(format!(
        "finish_gif_recording: rename {} -> {}",
        session.writing_path.display(),
        session.final_path.display()
    ));
    fs::rename(&session.writing_path, &session.final_path)
        .map_err(|error| FlickError::Message(format!("failed to save gif recording: {error}")))?;

    let record = CaptureRecord {
        id: session
            .final_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        created_at: Utc::now(),
        width: session.width,
        height: session.height,
        path: session.final_path.display().to_string(),
    };

    let _ = copy_path_to_clipboard(&record.path);
    recording_log(format!(
        "finish_gif_recording: copied gif file/path to clipboard path={}",
        record.path
    ));
    let _ = app.emit("capture-finished", record.clone());
    recording_log("finish_gif_recording: emitted capture-finished");

    let max_screenshots = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .max_screenshots;
    let screenshot_dir = history::current_screenshot_dir(&state)?;
    let _ = history::prune_capture_history(&screenshot_dir, max_screenshots);
    recording_log("finish_gif_recording: prune history attempted");

    Ok(record)
}

pub fn cancel_gif_recording(session_id: String) -> Result<(), FlickError> {
    recording_log(format!("cancel_gif_recording: enter session={session_id}"));
    let Ok(mut session) = remove_session(&session_id) else {
        recording_log("cancel_gif_recording: no active session");
        return Ok(());
    };
    recording_log("cancel_gif_recording: session removed from registry");
    session.stopped.store(true, Ordering::SeqCst);
    drop(session.stream.take());
    recording_log("cancel_gif_recording: live stream dropped");
    drop(session.sender);
    recording_log("cancel_gif_recording: sender dropped");
    if let Some(worker) = session.worker.take() {
        let _ = worker.join();
        recording_log("cancel_gif_recording: encoder worker joined");
    }
    let _ = fs::remove_file(&session.writing_path);
    recording_log(format!(
        "cancel_gif_recording: removed writing file {}",
        session.writing_path.display()
    ));
    Ok(())
}

pub fn set_gif_recording_window_shape(
    app: AppHandle,
    session_id: String,
    recording: bool,
) -> Result<(), FlickError> {
    recording_log(format!(
        "set_gif_recording_window_shape: enter session={session_id} recording={recording}"
    ));
    #[cfg(target_os = "windows")]
    {
        let Some(window) = screenshot_editor_window(&app, &session_id) else {
            recording_log("set_gif_recording_window_shape/windows: editor window not found");
            return Ok(());
        };
        recording_log(format!(
            "set_gif_recording_window_shape/windows: window label={}",
            window.label()
        ));
        let url = window
            .url()
            .map_err(|error| FlickError::Message(format!("failed to read editor url: {error}")))?;
        recording_log(format!("set_gif_recording_window_shape/windows: url={url}"));
        let regions = if recording {
            gif_recording_regions(&url)
        } else {
            regular_editor_regions(&url)
        };
        recording_log(format!(
            "set_gif_recording_window_shape/windows: apply regions={}",
            format_regions(&regions)
        ));
        crate::app::platform::configure_screenshot_editor_window_shape(&window, &regions);
        recording_log("set_gif_recording_window_shape/windows: shape applied");
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(window) = screenshot_editor_window_cross_platform(&app, &session_id) {
            recording_log(format!(
                "set_gif_recording_window_shape/non-windows: set_ignore_cursor_events={recording} label={}",
                window.label()
            ));
            let _ = window.set_ignore_cursor_events(recording);
        } else {
            recording_log("set_gif_recording_window_shape/non-windows: editor window not found");
        }
        recording_log(
            "set_gif_recording_window_shape: platform has no window shape implementation; using cursor passthrough",
        );
    }

    Ok(())
}

pub fn open_gif_recording_toolbar_window(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    recording_log(format!(
        "open_gif_recording_toolbar_window: enter session={session_id}"
    ));
    finalize_pending_overlay_for_recording(&app, &state, &session_id)?;
    windows::show_gif_recording_toolbar_window(&app, &session_id)?;
    recording_log("open_gif_recording_toolbar_window: complete");
    Ok(())
}

pub fn close_gif_recording_toolbar_window(app: AppHandle, session_id: String) {
    recording_log(format!(
        "close_gif_recording_toolbar_window: enter session={session_id}"
    ));
    windows::close_gif_recording_toolbar_window(&app, &session_id);
}

fn pending_selection(
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<SelectionRect, FlickError> {
    let pending = state
        .pending_capture_edits
        .lock()
        .map_err(|_| FlickError::Message("pending capture edits mutex poisoned".into()))?;
    pending
        .get(session_id)
        .map(|edit| edit.selection.clone())
        .ok_or_else(|| FlickError::Message("recording session selection is missing".into()))
}

fn finalize_pending_overlay_for_recording(
    app: &AppHandle,
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<(), FlickError> {
    let should_finalize = {
        let mut pending = state
            .pending_capture_edits
            .lock()
            .map_err(|_| FlickError::Message("pending capture edits mutex poisoned".into()))?;
        if let Some(session) = pending.get_mut(session_id) {
            if session.overlay_finalized {
                false
            } else {
                session.overlay_finalized = true;
                true
            }
        } else {
            false
        }
    };
    recording_log(format!(
        "finalize_pending_overlay_for_recording: should_finalize={should_finalize}"
    ));
    if should_finalize {
        platform::finalize_capture_session(app, state, true);
        recording_log("finalize_pending_overlay_for_recording: platform finalize complete");
    }
    Ok(())
}

fn validate_recording_size(selection: &SelectionRect) -> Result<(), FlickError> {
    if selection.width < 2 || selection.height < 2 {
        return Err(FlickError::Message(
            "recording selection is too small".into(),
        ));
    }
    if selection.width > u16::MAX as u32 || selection.height > u16::MAX as u32 {
        return Err(FlickError::Message(
            "recording selection is too large for GIF output".into(),
        ));
    }
    Ok(())
}

fn with_session(
    session_id: &str,
    update: impl FnOnce(&mut RecordingSession) -> Result<(), FlickError>,
) -> Result<(), FlickError> {
    let mut sessions = sessions()
        .lock()
        .map_err(|_| FlickError::Message("recording session mutex poisoned".into()))?;
    let session = sessions
        .get_mut(session_id)
        .ok_or_else(|| FlickError::Message("recording session is not active".into()))?;
    update(session)
}

fn remove_session(session_id: &str) -> Result<RecordingSession, FlickError> {
    sessions()
        .lock()
        .map_err(|_| FlickError::Message("recording session mutex poisoned".into()))?
        .remove(session_id)
        .ok_or_else(|| FlickError::Message("recording session is not active".into()))
}

fn encode_gif(
    path: PathBuf,
    width: u32,
    height: u32,
    max_gif_width: u32,
    max_gif_height: u32,
    receiver: mpsc::Receiver<RecordingMessage>,
) -> anyhow::Result<u32> {
    recording_log(format!(
        "encode_gif: start path={} nominal_width={width} nominal_height={height}",
        path.display()
    ));
    let file = fs::File::create(&path)
        .with_context(|| format!("failed to create gif recording file at {}", path.display()))?;
    let mut writer = Some(BufWriter::new(file));
    let mut encoder: Option<gif::Encoder<BufWriter<fs::File>>> = None;
    let mut output_width = 0;
    let mut output_height = 0;

    let mut frame_count = 0;
    while let Ok(message) = receiver.recv() {
        match message {
            RecordingMessage::Frame(frame) => {
                if encoder.is_none() {
                    let (scaled_width, scaled_height) = scaled_gif_size(
                        frame.width(),
                        frame.height(),
                        max_gif_width,
                        max_gif_height,
                    );
                    output_width = scaled_width;
                    output_height = scaled_height;
                    let width_u16 = u16::try_from(output_width).context("invalid GIF width")?;
                    let height_u16 = u16::try_from(output_height).context("invalid GIF height")?;
                    recording_log(format!(
                        "encode_gif: first frame actual_width={} actual_height={} output_width={} output_height={} fps={RECORDING_FPS}",
                        frame.width(),
                        frame.height(),
                        output_width,
                        output_height
                    ));
                    let mut next_encoder = gif::Encoder::new(
                        writer
                            .take()
                            .ok_or_else(|| anyhow::anyhow!("GIF writer is missing"))?,
                        width_u16,
                        height_u16,
                        &[],
                    )
                    .context("failed to create GIF encoder")?;
                    next_encoder
                        .set_repeat(gif::Repeat::Infinite)
                        .context("failed to configure GIF loop")?;
                    encoder = Some(next_encoder);
                }
                let width_u16 = u16::try_from(output_width).context("invalid GIF width")?;
                let height_u16 = u16::try_from(output_height).context("invalid GIF height")?;
                let mut pixels = if frame.width() == output_width && frame.height() == output_height
                {
                    frame.into_raw()
                } else {
                    image::imageops::resize(
                        &frame,
                        output_width,
                        output_height,
                        FilterType::Triangle,
                    )
                    .into_raw()
                };
                let mut gif_frame =
                    gif::Frame::from_rgba_speed(width_u16, height_u16, &mut pixels, 10);
                gif_frame.delay = RECORDING_GIF_DELAY_CS.max(1);
                encoder
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("GIF encoder is missing"))?
                    .write_frame(&gif_frame)
                    .context("failed to write GIF frame")?;
                frame_count += 1;
                if frame_count <= 3 || frame_count % 30 == 0 {
                    recording_log(format!("encode_gif: wrote frame_count={frame_count}"));
                }
            }
        }
    }

    recording_log("encode_gif: channel closed");
    recording_log(format!("encode_gif: complete frame_count={frame_count}"));
    Ok(frame_count)
}

fn copy_path_to_clipboard(path: &str) -> anyhow::Result<()> {
    recording_log(format!("copy_path_to_clipboard: start path={path}"));
    let mut clipboard = arboard::Clipboard::new().context("failed to access clipboard")?;
    if let Err(file_error) = clipboard.set().file_list(&[Path::new(path)]) {
        recording_log(format!(
            "copy_path_to_clipboard: file_list failed; fallback to text error={file_error}"
        ));
        clipboard
            .set_text(path.to_string())
            .with_context(|| format!("failed to copy gif file to clipboard: {file_error}"))?;
        recording_log("copy_path_to_clipboard: text fallback complete");
    } else {
        recording_log("copy_path_to_clipboard: file_list complete");
    }
    Ok(())
}

fn scaled_gif_size(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    if max_width == 0 || max_height == 0 || (width <= max_width && height <= max_height) {
        return (width.max(1), height.max(1));
    }
    let scale = (max_width as f64 / width.max(1) as f64)
        .min(max_height as f64 / height.max(1) as f64)
        .min(1.0);
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
}

fn recording_size_limits(state: &State<'_, AppState>) -> Result<(u32, u32), FlickError> {
    let size = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .gif_recording_size
        .clone();
    Ok(match size.trim().to_lowercase().as_str() {
        "540p" => (RECORDING_540P_MAX_GIF_WIDTH, RECORDING_540P_MAX_GIF_HEIGHT),
        _ => (RECORDING_720P_MAX_GIF_WIDTH, RECORDING_720P_MAX_GIF_HEIGHT),
    })
}

#[cfg(target_os = "windows")]
fn format_regions(regions: &[SelectionRect]) -> String {
    regions
        .iter()
        .map(|region| {
            format!(
                "({},{} {}x{})",
                region.x, region.y, region.width, region.height
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn screenshot_editor_window_cross_platform(
    app: &AppHandle,
    session_id: &str,
) -> Option<tauri::WebviewWindow> {
    let session_label = format!("screenshot-editor-{session_id}");
    app.get_webview_window(&session_label)
        .or_else(|| app.get_webview_window("screenshot-editor-preload"))
}

#[cfg(target_os = "windows")]
fn screenshot_editor_window(app: &AppHandle, session_id: &str) -> Option<tauri::WebviewWindow> {
    screenshot_editor_window_cross_platform(app, session_id)
}

#[cfg(target_os = "windows")]
fn gif_recording_regions(url: &tauri::Url) -> Vec<SelectionRect> {
    let selection_left = query_f64(url, "selection_left").unwrap_or(0.0);
    let selection_top = query_f64(url, "selection_top").unwrap_or(0.0);
    let width = query_f64(url, "display_width").unwrap_or(1.0).max(1.0);
    let height = query_f64(url, "display_height").unwrap_or(1.0).max(1.0);
    let toolbar_left = query_f64(url, "toolbar_left").unwrap_or(8.0);
    let toolbar_top = query_f64(url, "toolbar_top").unwrap_or(8.0);
    let border = 4.0_f64.min(width).min(height).max(1.0);

    vec![
        rect(selection_left, selection_top, width, border),
        rect(
            selection_left,
            selection_top + height - border,
            width,
            border,
        ),
        rect(selection_left, selection_top, border, height),
        rect(
            selection_left + width - border,
            selection_top,
            border,
            height,
        ),
        rect(toolbar_left, toolbar_top, 240.0, 56.0),
    ]
}

#[cfg(target_os = "windows")]
fn regular_editor_regions(url: &tauri::Url) -> Vec<SelectionRect> {
    let selection_left = query_f64(url, "selection_left").unwrap_or(0.0);
    let selection_top = query_f64(url, "selection_top").unwrap_or(0.0);
    let width = query_f64(url, "display_width").unwrap_or(1.0).max(1.0);
    let height = query_f64(url, "display_height").unwrap_or(1.0).max(1.0);
    let toolbar_left = query_f64(url, "toolbar_left").unwrap_or(8.0);
    let toolbar_top = query_f64(url, "toolbar_top").unwrap_or(8.0);
    let thumbnail_left = query_f64(url, "thumbnail_left").unwrap_or(8.0);
    let thumbnail_region_top = query_f64(url, "thumbnail_region_top").unwrap_or(8.0);
    let thumbnail_width = query_f64(url, "thumbnail_width").unwrap_or(300.0);
    let thumbnail_height = query_f64(url, "thumbnail_height").unwrap_or(560.0);

    vec![
        rect(selection_left, selection_top, width, height),
        rect(toolbar_left, toolbar_top - 300.0, 680.0, 340.0),
        rect(
            thumbnail_left,
            thumbnail_region_top,
            thumbnail_width,
            thumbnail_height,
        ),
    ]
}

#[cfg(target_os = "windows")]
fn rect(x: f64, y: f64, width: f64, height: f64) -> SelectionRect {
    SelectionRect {
        x: x.floor() as i32,
        y: y.floor().max(0.0) as i32,
        width: width.ceil().max(1.0) as u32,
        height: height.ceil().max(1.0) as u32,
    }
}

#[cfg(target_os = "windows")]
fn query_f64(url: &tauri::Url, key: &str) -> Option<f64> {
    url.query_pairs()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| value.parse::<f64>().ok())
}
