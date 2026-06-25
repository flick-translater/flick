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
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::{
    app::{AppState, windows},
    error::FlickError,
    models::{CaptureRecord, SelectionRect},
    services::{ScreenCaptureService, screen_capture::LiveFrameStream},
};

use super::{history, platform};

const DEFAULT_RECORDING_FPS: u32 = 6;
const RECORDING_QUEUE_CAPACITY: usize = 8;
const RECORDING_540P_MAX_GIF_WIDTH: u32 = 960;
const RECORDING_540P_MAX_GIF_HEIGHT: u32 = 540;
const RECORDING_720P_MAX_GIF_WIDTH: u32 = 1280;
const RECORDING_720P_MAX_GIF_HEIGHT: u32 = 720;

enum RecordingMessage {
    Frame(ImageBuffer<Rgba<u8>, Vec<u8>>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordingFormat {
    Gif,
}

impl RecordingFormat {
    fn parse(value: &str) -> Result<Self, FlickError> {
        match value.trim().to_lowercase().as_str() {
            "gif" => Ok(Self::Gif),
            _ => Err(FlickError::Message("unsupported recording format".into())),
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Gif => "gif",
        }
    }
}

struct RecordingOutputPaths {
    final_path: PathBuf,
    writing_path: PathBuf,
}

struct RecordingEncoderOptions {
    max_width: u32,
    max_height: u32,
    fps: u32,
}

struct RecordingEncodeResult {
    frame_count: u32,
}

struct RecordingSession {
    stream: Option<LiveFrameStream>,
    sender: SyncSender<RecordingMessage>,
    worker: Option<thread::JoinHandle<anyhow::Result<RecordingEncodeResult>>>,
    paused: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    output_paths: RecordingOutputPaths,
    width: u32,
    height: u32,
}

static RECORDING_SESSIONS: OnceLock<Mutex<HashMap<String, RecordingSession>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<String, RecordingSession>> {
    RECORDING_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn start_gif_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    start_recording(app, state, session_id, "gif".into())
}

pub fn start_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    format: String,
) -> Result<(), FlickError> {
    let format = RecordingFormat::parse(&format)?;
    let selection = pending_selection(&state, &session_id)?;
    validate_recording_size(&selection)?;
    finalize_pending_overlay_for_recording(&app, &state, &session_id)?;

    let screenshot_dir = history::current_screenshot_dir(&state)?;
    fs::create_dir_all(&screenshot_dir).map_err(|error| {
        FlickError::Message(format!("failed to create screenshot dir: {error}"))
    })?;

    let output_paths = recording_output_paths(&screenshot_dir, format);
    let (sender, receiver) = mpsc::sync_channel(RECORDING_QUEUE_CAPACITY);
    let encoder_options = recording_encoder_options(&state, format)?;
    let frame_interval = recording_frame_interval(encoder_options.fps);
    let worker = start_recording_encoder(
        format,
        output_paths.writing_path.clone(),
        encoder_options,
        receiver,
    );

    let paused = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let stream_sender = sender.clone();
    let stream_paused = Arc::clone(&paused);
    let stream_stopped = Arc::clone(&stopped);
    let last_frame_at = Arc::new(Mutex::new(Instant::now() - frame_interval));
    let stream_last_frame_at = Arc::clone(&last_frame_at);
    let on_frame = Box::new(move |frame: ImageBuffer<Rgba<u8>, Vec<u8>>| {
        if stream_stopped.load(Ordering::SeqCst) || stream_paused.load(Ordering::SeqCst) {
            return;
        }
        let Ok(mut last_frame_at) = stream_last_frame_at.lock() else {
            return;
        };
        if last_frame_at.elapsed() < frame_interval {
            return;
        }
        *last_frame_at = Instant::now();
        let _ = stream_sender.try_send(RecordingMessage::Frame(frame));
    });

    prepare_recording_capture_visibility(&app, &session_id);
    #[cfg(target_os = "windows")]
    {
        if let Some(frame) = capture_initial_recording_frame(&selection) {
            let _ = sender.try_send(RecordingMessage::Frame(frame));
        }
    }

    let stream = match ScreenCaptureService::default().open_live_frame_stream(&selection, on_frame)
    {
        Ok(stream) => stream,
        Err(error) => {
            stopped.store(true, Ordering::SeqCst);
            drop(sender);
            let _ = worker.join();
            let _ = fs::remove_file(&output_paths.writing_path);
            cleanup_recording_capture_visibility(&app, &session_id);
            return Err(FlickError::Message(format!(
                "failed to start screen recording: {error}"
            )));
        }
    };

    let mut sessions = sessions()
        .lock()
        .map_err(|_| FlickError::Message("recording session mutex poisoned".into()))?;
    if sessions.contains_key(&session_id) {
        stopped.store(true, Ordering::SeqCst);
        drop(stream);
        drop(sender);
        let _ = worker.join();
        let _ = fs::remove_file(&output_paths.writing_path);
        cleanup_recording_capture_visibility(&app, &session_id);
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
            output_paths,
            width: selection.width,
            height: selection.height,
        },
    );

    let _ = app.emit("gif-recording-status", "recording");
    Ok(())
}

pub fn pause_gif_recording(session_id: String) -> Result<(), FlickError> {
    pause_recording(session_id)
}

pub fn pause_recording(session_id: String) -> Result<(), FlickError> {
    with_session(&session_id, |session| {
        session.paused.store(true, Ordering::SeqCst);
        Ok(())
    })
}

pub fn resume_gif_recording(session_id: String) -> Result<(), FlickError> {
    resume_recording(session_id)
}

pub fn resume_recording(session_id: String) -> Result<(), FlickError> {
    with_session(&session_id, |session| {
        session.paused.store(false, Ordering::SeqCst);
        Ok(())
    })
}

pub fn finish_gif_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<CaptureRecord, FlickError> {
    finish_recording(app, state, session_id)
}

pub fn finish_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<CaptureRecord, FlickError> {
    let mut session = remove_session(&session_id)?;
    session.stopped.store(true, Ordering::SeqCst);
    drop(session.stream.take());
    drop(session.sender);
    cleanup_recording_capture_visibility(&app, &session_id);

    let result = session
        .worker
        .take()
        .ok_or_else(|| FlickError::Message("recording encoder worker is missing".into()))?
        .join()
        .map_err(|_| FlickError::Message("recording encoder worker panicked".into()))?
        .map_err(|error| FlickError::Message(format!("failed to encode recording: {error}")))?;

    if result.frame_count == 0 {
        let _ = fs::remove_file(&session.output_paths.writing_path);
        return Err(FlickError::Message("no frames were recorded".into()));
    }

    fs::rename(
        &session.output_paths.writing_path,
        &session.output_paths.final_path,
    )
    .map_err(|error| FlickError::Message(format!("failed to save recording: {error}")))?;

    let record = CaptureRecord {
        id: session
            .output_paths
            .final_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        created_at: Utc::now(),
        width: session.width,
        height: session.height,
        path: session.output_paths.final_path.display().to_string(),
    };

    let _ = copy_path_to_clipboard(&record.path);
    let _ = app.emit("capture-finished", record.clone());

    let max_screenshots = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .max_screenshots;
    let screenshot_dir = history::current_screenshot_dir(&state)?;
    let _ = history::prune_capture_history(&screenshot_dir, max_screenshots);

    Ok(record)
}

pub fn cancel_gif_recording(app: AppHandle, session_id: String) -> Result<(), FlickError> {
    cancel_recording(app, session_id)
}

pub fn cancel_recording(app: AppHandle, session_id: String) -> Result<(), FlickError> {
    let Ok(mut session) = remove_session(&session_id) else {
        cleanup_recording_capture_visibility(&app, &session_id);
        return Ok(());
    };
    session.stopped.store(true, Ordering::SeqCst);
    drop(session.stream.take());
    drop(session.sender);
    if let Some(worker) = session.worker.take() {
        let _ = worker.join();
    }
    let _ = fs::remove_file(&session.output_paths.writing_path);
    cleanup_recording_capture_visibility(&app, &session_id);
    Ok(())
}

pub fn set_gif_recording_window_shape(
    app: AppHandle,
    session_id: String,
    recording: bool,
) -> Result<(), FlickError> {
    set_recording_window_mode(app, session_id, recording)
}

pub fn set_recording_window_mode(
    app: AppHandle,
    session_id: String,
    recording: bool,
) -> Result<(), FlickError> {
    platform::set_recording_window_mode(&app, &session_id, recording)
}

pub fn open_gif_recording_toolbar_window(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    open_recording_controls_window(app, state, session_id)
}

pub fn open_recording_controls_window(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    finalize_pending_overlay_for_recording(&app, &state, &session_id)?;
    windows::show_gif_recording_toolbar_window(&app, &session_id)?;
    Ok(())
}

pub fn close_gif_recording_toolbar_window(app: AppHandle, session_id: String) {
    close_recording_controls_window(app, session_id);
}

pub fn close_recording_controls_window(app: AppHandle, session_id: String) {
    windows::close_gif_recording_toolbar_window(&app, &session_id);
}

#[cfg(target_os = "windows")]
fn capture_initial_recording_frame(
    selection: &SelectionRect,
) -> Option<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    ScreenCaptureService::default()
        .capture_selection(selection, &[])
        .ok()
}

fn prepare_recording_capture_visibility(app: &AppHandle, session_id: &str) {
    platform::prepare_recording_capture_visibility(app, session_id);
}

fn cleanup_recording_capture_visibility(app: &AppHandle, session_id: &str) {
    platform::cleanup_recording_capture_visibility(app, session_id);
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
    if should_finalize {
        platform::finalize_capture_session(app, state, true);
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

fn recording_output_paths(screenshot_dir: &Path, format: RecordingFormat) -> RecordingOutputPaths {
    let id = Uuid::new_v4().to_string();
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    let extension = format.extension();
    RecordingOutputPaths {
        final_path: screenshot_dir.join(format!("recording-{timestamp}-{id}.{extension}")),
        writing_path: screenshot_dir
            .join(format!("recording-{timestamp}-{id}.writing.{extension}")),
    }
}

fn start_recording_encoder(
    format: RecordingFormat,
    writing_path: PathBuf,
    options: RecordingEncoderOptions,
    receiver: mpsc::Receiver<RecordingMessage>,
) -> thread::JoinHandle<anyhow::Result<RecordingEncodeResult>> {
    thread::spawn(move || match format {
        RecordingFormat::Gif => encode_gif(writing_path, options, receiver),
    })
}

fn encode_gif(
    path: PathBuf,
    options: RecordingEncoderOptions,
    receiver: mpsc::Receiver<RecordingMessage>,
) -> anyhow::Result<RecordingEncodeResult> {
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
                        options.max_width,
                        options.max_height,
                    );
                    output_width = scaled_width;
                    output_height = scaled_height;
                    let width_u16 = u16::try_from(output_width).context("invalid GIF width")?;
                    let height_u16 = u16::try_from(output_height).context("invalid GIF height")?;
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
                gif_frame.delay = gif_delay_cs(options.fps);
                encoder
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("GIF encoder is missing"))?
                    .write_frame(&gif_frame)
                    .context("failed to write GIF frame")?;
                frame_count += 1;
            }
        }
    }

    Ok(RecordingEncodeResult { frame_count })
}

fn copy_path_to_clipboard(path: &str) -> anyhow::Result<()> {
    let mut clipboard = arboard::Clipboard::new().context("failed to access clipboard")?;
    if let Err(file_error) = clipboard.set().file_list(&[Path::new(path)]) {
        clipboard
            .set_text(path.to_string())
            .with_context(|| format!("failed to copy gif file to clipboard: {file_error}"))?;
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

fn recording_frame_interval(fps: u32) -> Duration {
    Duration::from_millis(1000 / u64::from(fps.max(1)))
}

fn gif_delay_cs(fps: u32) -> u16 {
    (100 / fps.max(1)) as u16
}

fn recording_encoder_options(
    state: &State<'_, AppState>,
    format: RecordingFormat,
) -> Result<RecordingEncoderOptions, FlickError> {
    match format {
        RecordingFormat::Gif => gif_encoder_options(state),
    }
}

fn gif_encoder_options(state: &State<'_, AppState>) -> Result<RecordingEncoderOptions, FlickError> {
    let size = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .gif_recording_size
        .clone();
    let fps = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .gif_recording_fps;
    let (max_width, max_height) = match size.trim().to_lowercase().as_str() {
        "540p" => (RECORDING_540P_MAX_GIF_WIDTH, RECORDING_540P_MAX_GIF_HEIGHT),
        _ => (RECORDING_720P_MAX_GIF_WIDTH, RECORDING_720P_MAX_GIF_HEIGHT),
    };
    Ok(RecordingEncoderOptions {
        max_width,
        max_height,
        fps: match fps {
            6 | 8 | 10 => fps,
            _ => DEFAULT_RECORDING_FPS,
        },
    })
}
