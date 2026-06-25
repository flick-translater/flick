use std::{
    collections::HashMap,
    fs,
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
use image::{ImageBuffer, Rgba};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::{
    app::{AppState, windows},
    error::FlickError,
    models::{CaptureRecord, SelectionRect},
    services::{ScreenCaptureService, screen_capture::LiveFrameStream},
};

use super::{history, platform, recording_gif, recording_video};

const DEFAULT_RECORDING_FPS: u32 = 6;
const RECORDING_QUEUE_CAPACITY: usize = 8;
const RECORDING_540P_MAX_GIF_WIDTH: u32 = 960;
const RECORDING_540P_MAX_GIF_HEIGHT: u32 = 540;
const RECORDING_720P_MAX_GIF_WIDTH: u32 = 1280;
const RECORDING_720P_MAX_GIF_HEIGHT: u32 = 720;
const RECORDING_1080P_MAX_VIDEO_WIDTH: u32 = 1920;
const RECORDING_1080P_MAX_VIDEO_HEIGHT: u32 = 1080;

pub(super) enum RecordingMessage {
    Frame {
        image: ImageBuffer<Rgba<u8>, Vec<u8>>,
        elapsed: Duration,
    },
    End {
        elapsed: Duration,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordingFormat {
    Gif,
    Video,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RecordingAudioMode {
    Unsupported,
    Disabled,
    System,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RecordingProfile {
    format: RecordingFormat,
    audio: RecordingAudioMode,
}

impl RecordingFormat {
    fn parse(value: &str) -> Result<Self, FlickError> {
        match value.trim().to_lowercase().as_str() {
            "gif" => Ok(Self::Gif),
            "video" | "mp4" => Ok(Self::Video),
            _ => Err(FlickError::Message("unsupported recording format".into())),
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Gif => "gif",
            Self::Video => "mp4",
        }
    }
}

impl RecordingProfile {
    fn from_settings(
        format: RecordingFormat,
        state: &State<'_, AppState>,
    ) -> Result<Self, FlickError> {
        let audio = match format {
            RecordingFormat::Gif => RecordingAudioMode::Unsupported,
            RecordingFormat::Video => {
                let source = state
                    .settings
                    .lock()
                    .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
                    .video_recording_audio_source
                    .clone();
                match source.trim().to_lowercase().as_str() {
                    "none" => RecordingAudioMode::Disabled,
                    _ => RecordingAudioMode::System,
                }
            }
        };
        Ok(Self { format, audio })
    }
}

struct RecordingOutputPaths {
    final_path: PathBuf,
    writing_path: PathBuf,
}

pub(super) struct RecordingEncoderOptions {
    pub(super) max_width: u32,
    pub(super) max_height: u32,
    pub(super) fps: u32,
    pub(super) ffmpeg_path: Option<String>,
    pub(super) audio: RecordingAudioMode,
}

pub(super) struct RecordingEncodeResult {
    pub(super) frame_count: u32,
}

struct RecordingSession {
    stream: Option<LiveFrameStream>,
    sender: SyncSender<RecordingMessage>,
    worker: Option<thread::JoinHandle<anyhow::Result<RecordingEncodeResult>>>,
    paused: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    output_paths: RecordingOutputPaths,
    format: RecordingFormat,
    started_at: Instant,
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
    let profile = RecordingProfile::from_settings(format, &state)?;
    let selection = pending_selection(&state, &session_id)?;
    validate_recording_size(&selection)?;
    finalize_pending_overlay_for_recording(&app, &state, &session_id)?;

    let output_dir = match format {
        RecordingFormat::Gif => history::current_screenshot_dir(&state)?,
        RecordingFormat::Video => history::current_video_dir(&state)?,
    };
    fs::create_dir_all(&output_dir)
        .map_err(|error| FlickError::Message(format!("failed to create recording dir: {error}")))?;

    let output_paths = recording_output_paths(&output_dir, format);
    let (sender, receiver) = mpsc::sync_channel(RECORDING_QUEUE_CAPACITY);
    let encoder_options = recording_encoder_options(&state, profile)?;
    let frame_interval = recording_frame_interval(encoder_options.fps);
    let worker = start_recording_encoder(
        format,
        output_paths.writing_path.clone(),
        encoder_options,
        receiver,
    );

    let paused = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let started_at = Instant::now();
    let stream_sender = sender.clone();
    let stream_paused = Arc::clone(&paused);
    let stream_stopped = Arc::clone(&stopped);
    let stream_started_at = started_at;
    let next_frame_due = Arc::new(Mutex::new(Instant::now()));
    let stream_next_frame_due = Arc::clone(&next_frame_due);
    let on_frame = Box::new(move |frame: ImageBuffer<Rgba<u8>, Vec<u8>>| {
        if stream_stopped.load(Ordering::SeqCst) || stream_paused.load(Ordering::SeqCst) {
            return;
        }
        let now = Instant::now();
        let Ok(mut next_frame_due) = stream_next_frame_due.lock() else {
            return;
        };
        if now < *next_frame_due {
            return;
        }
        while *next_frame_due <= now {
            *next_frame_due += frame_interval;
        }
        let elapsed = now.duration_since(stream_started_at);
        let _ = stream_sender.try_send(RecordingMessage::Frame {
            image: frame,
            elapsed,
        });
    });

    prepare_recording_capture_visibility(&app, &session_id);
    #[cfg(target_os = "windows")]
    {
        if let Some(frame) = capture_initial_recording_frame(&selection) {
            let _ = sender.try_send(RecordingMessage::Frame {
                image: frame,
                elapsed: Duration::ZERO,
            });
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
            format,
            started_at,
            width: selection.width,
            height: selection.height,
        },
    );

    let _ = app.emit("recording-status", "recording");
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
    let _ = session.sender.send(RecordingMessage::End {
        elapsed: session.started_at.elapsed(),
    });
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
    let event = match session.format {
        RecordingFormat::Gif => "capture-finished",
        RecordingFormat::Video => "video-finished",
    };
    let _ = app.emit(event, record.clone());

    if matches!(session.format, RecordingFormat::Gif) {
        let max_screenshots = state
            .settings
            .lock()
            .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
            .max_screenshots;
        let screenshot_dir = history::current_screenshot_dir(&state)?;
        let _ = history::prune_capture_history(&screenshot_dir, max_screenshots);
    }

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
        RecordingFormat::Gif => recording_gif::encode_gif(writing_path, options, receiver),
        RecordingFormat::Video => recording_video::encode_video(writing_path, options, receiver),
    })
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

fn recording_frame_interval(fps: u32) -> Duration {
    Duration::from_nanos(1_000_000_000 / u64::from(fps.max(1)))
}

fn recording_encoder_options(
    state: &State<'_, AppState>,
    profile: RecordingProfile,
) -> Result<RecordingEncoderOptions, FlickError> {
    match profile.format {
        RecordingFormat::Gif => gif_encoder_options(state),
        RecordingFormat::Video => video_encoder_options(state, profile.audio),
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
        ffmpeg_path: None,
        audio: RecordingAudioMode::Unsupported,
    })
}

fn video_encoder_options(
    state: &State<'_, AppState>,
    audio: RecordingAudioMode,
) -> Result<RecordingEncoderOptions, FlickError> {
    let settings = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .clone();
    let status = state
        .ffmpeg_status
        .lock()
        .map_err(|_| FlickError::Message("ffmpeg status mutex poisoned".into()))?
        .clone();
    if !status.available {
        return Err(FlickError::Message("ffmpeg is not available".into()));
    }

    let (max_width, max_height) = match settings.video_recording_size.trim().to_lowercase().as_str()
    {
        "540p" => (RECORDING_540P_MAX_GIF_WIDTH, RECORDING_540P_MAX_GIF_HEIGHT),
        "1080p" => (
            RECORDING_1080P_MAX_VIDEO_WIDTH,
            RECORDING_1080P_MAX_VIDEO_HEIGHT,
        ),
        _ => (RECORDING_720P_MAX_GIF_WIDTH, RECORDING_720P_MAX_GIF_HEIGHT),
    };
    Ok(RecordingEncoderOptions {
        max_width,
        max_height,
        fps: match settings.video_recording_fps {
            24 | 30 => settings.video_recording_fps,
            _ => 24,
        },
        ffmpeg_path: Some(status.path),
        audio,
    })
}
