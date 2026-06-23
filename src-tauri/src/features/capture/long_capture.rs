use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose};
use chrono::{SecondsFormat, Utc};
#[cfg(target_os = "macos")]
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes, kCFRunLoopDefaultMode};
#[cfg(target_os = "macos")]
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult, EventField,
};
use image::{
    ImageBuffer, ImageEncoder, Rgba,
    codecs::png::{CompressionType, FilterType as PngFilterType},
    imageops::FilterType as ResizeFilterType,
};
use tauri::{
    AppHandle, Emitter, LogicalPosition, Manager, State, WebviewUrl, WebviewWindowBuilder,
};

use crate::{
    app::AppState,
    error::FlickError,
    models::{CaptureRecord, LongCaptureUpdate, SelectionRect},
    services::ScreenCaptureService,
};

use super::{history, platform, session};

/// Hide the editor window before the initial live capture so the OS composites the frame
/// without the editor on top of it. (Only used for the one-shot initial frame; the streaming
/// sampling loop relies on session-wide capture exclusion instead.)
const WINDOW_HIDE_DELAY: Duration = Duration::from_millis(70);
const FINALIZE_CAPTURE_WAIT_TIMEOUT: Duration = Duration::from_millis(1500);
const FINALIZE_CAPTURE_WAIT_POLL: Duration = Duration::from_millis(25);
const LONG_PREVIEW_WIDTH: u32 = 240;
/// Target interval between frame samples while the user is scrolling. Small enough that even
/// fast scrolling keeps consecutive frames overlapping, so the stitcher can recover the shift.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(60);
/// How long scrolling must be quiet (covering trackpad inertia) before the sampling loop stops.
const SCROLL_IDLE_STOP: Duration = Duration::from_millis(280);
/// Number of columns sampled per row when building its content hash.
const SHIFT_SAMPLE_COLS: u32 = 64;
/// Rows whose sampled pixels span less than this (weighted) luminance range are treated as
/// blank/trivial and excluded from alignment scoring.
const TRIVIAL_ROW_LUMA_RANGE: u32 = 48;
/// Shifts within this many pixels of the best shift are treated as the same alignment (scrolling
/// is continuous, so neighbours share most of the same run) and excluded from the runner-up.
const SHIFT_NEIGHBOR_GUARD: u32 = 4;
/// Minimum number of matched non-trivial rows required as absolute evidence for an alignment.
const MIN_QUALITY_ROWS: u32 = 6;
/// Minimum match fraction (parts per thousand) of matched-vs-comparable rows for a trusted
/// alignment. High enough to reject misaligned content (which produces conflicts), low enough to
/// tolerate the few rows that legitimately differ at the edges between two frames.
const MIN_MATCH_PERMILLE: u32 = 750;
const BOUNDARY_COMPARE_ROWS_PERCENT: f64 = 100.0;
/// Per-channel tolerance when checking whether a frame equals an already-stitched region. Keeps
/// boundary re-detection robust against anti-aliasing jitter between two captures of the same content.
const PIXEL_MATCH_TOLERANCE: u8 = 6;

fn long_log(message: impl AsRef<str>) {
    eprintln!(
        "[long-capture] {} {}",
        Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        message.as_ref()
    );
}

fn monotonic_millis() -> i64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as i64
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CaptureRange {
    top: i64,
    bottom: i64,
}

impl CaptureRange {
    fn from_top_height(top: i64, height: u32) -> Self {
        Self {
            top,
            bottom: top + i64::from(height),
        }
    }

    fn height(self) -> u32 {
        (self.bottom - self.top).max(1) as u32
    }

    fn contains(self, other: Self) -> bool {
        other.top >= self.top && other.bottom <= self.bottom
    }
}

struct LongCaptureSession {
    selection: SelectionRect,
    /// The full stitched image accumulated so far.
    stitched: ImageBuffer<Rgba<u8>, Vec<u8>>,
    /// Covered coordinate range of `stitched` in the long-capture coordinate space.
    stitched_range: CaptureRange,
    /// Top coordinate of `last_frame` in the long-capture coordinate space.
    current_y: i64,
    /// The most recently captured single frame.
    last_frame: ImageBuffer<Rgba<u8>, Vec<u8>>,
    /// Magnitude (pixels) of the last confidently-detected scroll shift. Used to seed the next
    /// frame's shift search, since scroll speed is continuous frame-to-frame. This makes the hint
    /// track the real scroll velocity instead of a fixed guess, which matters for disambiguating
    /// periodic content (tables/lists/code).
    last_shift: u32,
    /// Set while a real-wheel capture worker is waiting/capturing after a scroll.
    capture_pending: Arc<AtomicBool>,
    /// Set when the session ends; the scroll watcher thread observes this and exits.
    stop: Arc<AtomicBool>,
}

enum PreviewUpdate {
    Replace,
    Append {
        rows: u32,
        image: ImageBuffer<Rgba<u8>, Vec<u8>>,
    },
    /// The stitched image did not grow, but `current_y` (the viewport position within the
    /// stitched image) moved — e.g. scrolling back up into already-captured content. The
    /// front-end still needs this so its preview viewport tracks the scroll.
    OffsetOnly,
    /// Nothing changed at all; no update should be emitted.
    None,
}

fn sessions() -> &'static Mutex<HashMap<String, LongCaptureSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, LongCaptureSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn start_long_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<LongCaptureUpdate, FlickError> {
    long_log(format!("start: session={session_id}"));
    let selection = pending_selection(&state, &session_id)?;
    long_log(format!(
        "start: pending selection x={} y={} width={} height={}",
        selection.x, selection.y, selection.width, selection.height
    ));
    let frame = capture_live_frame(&app, &state, &session_id, &selection)?;
    long_log(format!(
        "start: initial frame captured {}x{}",
        frame.width(),
        frame.height()
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let capture_pending = Arc::new(AtomicBool::new(false));
    let cursor_passthrough = Arc::new(AtomicBool::new(false));
    let last_scroll_millis = Arc::new(AtomicI64::new(0));
    let last_scroll_delta = Arc::new(AtomicI64::new(0));
    let target_pid = long_capture_target_pid(&state);
    let session = LongCaptureSession {
        selection: selection.clone(),
        stitched: frame.clone(),
        stitched_range: CaptureRange::from_top_height(0, frame.height()),
        current_y: 0,
        last_frame: frame,
        last_shift: 0,
        capture_pending: capture_pending.clone(),
        stop: stop.clone(),
    };
    let update = build_update(&session, PreviewUpdate::Replace)?;
    long_log(format!(
        "start: initial update total_height={} frame_height={} preview_len={} current_len={}",
        update.total_height,
        update.frame_height,
        update.preview_data_url.len(),
        update.current_frame_data_url.len()
    ));
    sessions()
        .lock()
        .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?
        .insert(session_id.clone(), session);
    long_log(format!("start: session stored session={session_id}"));
    start_real_scroll_watcher(
        app,
        session_id,
        selection,
        stop,
        capture_pending,
        cursor_passthrough,
        last_scroll_millis,
        last_scroll_delta,
        target_pid,
    );
    Ok(update)
}

pub fn get_long_capture_image(session_id: String) -> Result<String, FlickError> {
    let guard = sessions()
        .lock()
        .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?;
    let session = guard
        .get(&session_id)
        .ok_or_else(|| FlickError::Message("long capture session not found".into()))?;
    image_to_data_url(&session.stitched)
}

#[cfg(target_os = "macos")]
fn start_real_scroll_watcher(
    app: AppHandle,
    session_id: String,
    selection: SelectionRect,
    stop: Arc<AtomicBool>,
    capture_pending: Arc<AtomicBool>,
    cursor_passthrough: Arc<AtomicBool>,
    last_scroll_millis: Arc<AtomicI64>,
    last_scroll_delta: Arc<AtomicI64>,
    target_pid: Option<i32>,
) {
    long_log(format!(
        "real_scroll_watcher: start session={session_id} selection=({},{} {}x{})",
        selection.x, selection.y, selection.width, selection.height
    ));
    thread::spawn(move || {
        run_real_scroll_watcher(
            app,
            session_id,
            selection,
            stop,
            capture_pending,
            cursor_passthrough,
            last_scroll_millis,
            last_scroll_delta,
            target_pid,
        );
    });
}

#[cfg(not(target_os = "macos"))]
fn start_real_scroll_watcher(
    _app: AppHandle,
    _session_id: String,
    _selection: SelectionRect,
    _stop: Arc<AtomicBool>,
    _capture_pending: Arc<AtomicBool>,
    _cursor_passthrough: Arc<AtomicBool>,
    _last_scroll_millis: Arc<AtomicI64>,
    _last_scroll_delta: Arc<AtomicI64>,
    _target_pid: Option<i32>,
) {
}

#[cfg(target_os = "macos")]
fn run_real_scroll_watcher(
    app: AppHandle,
    session_id: String,
    selection: SelectionRect,
    stop: Arc<AtomicBool>,
    capture_pending: Arc<AtomicBool>,
    cursor_passthrough: Arc<AtomicBool>,
    last_scroll_millis: Arc<AtomicI64>,
    last_scroll_delta: Arc<AtomicI64>,
    target_pid: Option<i32>,
) {
    platform::set_overlay_mouse_passthrough(&app, true);
    // Exclude the overlay and editor window from screen capture for the whole session, instead
    // of toggling it per frame. The sampling loop captures continuously while the user scrolls,
    // so per-frame hide/show would add latency and flicker on every single frame.
    platform::set_overlay_capture_sharing(&app, false);
    if let Some((_, window)) = screenshot_editor_window(&app, &session_id) {
        platform::set_window_capture_sharing(&window, false);
    }
    let event_types = vec![CGEventType::MouseMoved, CGEventType::ScrollWheel];
    let app_for_tap = app.clone();
    let session_for_tap = session_id.clone();
    let selection_for_tap = selection.clone();
    let stop_for_tap = stop.clone();
    let capture_pending_for_tap = capture_pending.clone();
    let cursor_passthrough_for_tap = cursor_passthrough.clone();
    let last_scroll_millis_for_tap = last_scroll_millis.clone();
    let last_scroll_delta_for_tap = last_scroll_delta.clone();
    // `target_pid` is unused now that we no longer synthesize scroll events into a target app;
    // the user scrolls it directly. Kept in the signature for cross-platform symmetry.
    let _ = target_pid;

    let tap = match CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        event_types,
        move |_proxy, event_type, event| {
            if stop_for_tap.load(Ordering::SeqCst) {
                return CallbackResult::Keep;
            }

            let location = event.location();
            let inside_selection = point_in_selection(location.x, location.y, &selection_for_tap);
            update_editor_cursor_passthrough(
                &app_for_tap,
                &session_for_tap,
                inside_selection,
                &cursor_passthrough_for_tap,
            );

            if matches!(event_type, CGEventType::ScrollWheel) && inside_selection {
                // Free-scroll model: let the wheel event pass through so the user scrolls the
                // real target window. We only record that scrolling is happening (and its
                // direction) so the sampling loop knows when to grab frames.
                let delta_y = scroll_delta_y(event);
                if delta_y != 0.0 {
                    last_scroll_millis_for_tap.store(monotonic_millis(), Ordering::SeqCst);
                    last_scroll_delta_for_tap
                        .store(delta_y.signum().round() as i64, Ordering::SeqCst);
                    ensure_sampling_running(
                        app_for_tap.clone(),
                        session_for_tap.clone(),
                        capture_pending_for_tap.clone(),
                        stop_for_tap.clone(),
                        last_scroll_millis_for_tap.clone(),
                        last_scroll_delta_for_tap.clone(),
                    );
                }
                return CallbackResult::Keep;
            }

            CallbackResult::Keep
        },
    ) {
        Ok(tap) => tap,
        Err(()) => {
            long_log("real_scroll_watcher: failed to create event tap");
            return;
        }
    };

    let source = match tap.mach_port().create_runloop_source(0) {
        Ok(source) => source,
        Err(()) => {
            long_log("real_scroll_watcher: failed to create runloop source");
            return;
        }
    };

    let run_loop = CFRunLoop::get_current();
    run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
    tap.enable();
    long_log("real_scroll_watcher: event tap enabled");

    while !stop.load(Ordering::SeqCst) {
        CFRunLoop::run_in_mode(
            unsafe { kCFRunLoopDefaultMode },
            Duration::from_millis(50),
            true,
        );
    }

    set_editor_cursor_passthrough(&app, &session_id, false);
    platform::set_overlay_mouse_passthrough(&app, false);
    // Restore capture sharing that was disabled for the whole session above.
    platform::set_overlay_capture_sharing(&app, true);
    if let Some((_, window)) = screenshot_editor_window(&app, &session_id) {
        platform::set_window_capture_sharing(&window, true);
    }
    long_log("real_scroll_watcher: stopped");
}

#[cfg(target_os = "macos")]
fn point_in_selection(x: f64, y: f64, selection: &SelectionRect) -> bool {
    x >= selection.x as f64
        && x <= (selection.x + selection.width as i32) as f64
        && y >= selection.y as f64
        && y <= (selection.y + selection.height as i32) as f64
}

#[cfg(target_os = "macos")]
fn scroll_delta_y(event: &core_graphics::event::CGEvent) -> f64 {
    let point_delta =
        event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1);
    if point_delta != 0 {
        return point_delta as f64;
    }
    let fixed_delta =
        event.get_double_value_field(EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1);
    if fixed_delta != 0.0 {
        return fixed_delta;
    }
    event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1) as f64
}

/// Ensure exactly one sampling loop is running for this session.
///
/// Called from the event tap on every wheel event. The `capture_pending` flag doubles as a
/// "sampling loop active" guard so concurrent wheel events don't spawn duplicate loops.
#[cfg(target_os = "macos")]
fn ensure_sampling_running(
    app: AppHandle,
    session_id: String,
    capture_pending: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    last_scroll_millis: Arc<AtomicI64>,
    last_scroll_delta: Arc<AtomicI64>,
) {
    if capture_pending
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        // A sampling loop is already running; it will keep grabbing frames while scrolling
        // continues, so there's nothing to do.
        return;
    }

    thread::spawn(move || {
        long_log("sampling: loop start");
        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }

            let idle_ms = monotonic_millis() - last_scroll_millis.load(Ordering::SeqCst);
            // Stop sampling once scrolling (including trackpad inertia) has been quiet long
            // enough. The loop restarts on the next wheel event.
            if idle_ms > SCROLL_IDLE_STOP.as_millis() as i64 {
                long_log(format!("sampling: idle {idle_ms}ms, stopping loop"));
                break;
            }

            let direction = last_scroll_delta.load(Ordering::SeqCst);
            let tick_started = Instant::now();
            if let Err(error) = sample_and_stitch_frame(&app, &session_id, direction) {
                long_log(format!("sampling: frame failed {error}"));
            }

            // Pace the loop to the target sampling interval, accounting for the time already
            // spent capturing and stitching this frame.
            let spent = tick_started.elapsed();
            if let Some(remaining) = SAMPLE_INTERVAL.checked_sub(spent) {
                thread::sleep(remaining);
            }
        }
        capture_pending.store(false, Ordering::SeqCst);
        long_log("sampling: loop stopped");
    });
}

/// Capture one live frame of the selection and stitch it onto the running image.
///
/// Under the free-scroll model the window/overlay capture exclusion is set once for the whole
/// session (see [`run_real_scroll_watcher`]), so this hot path does no per-frame window
/// hiding, no synthesized scrolling, and no settle delays — just capture, measure, stitch.
#[cfg(target_os = "macos")]
fn sample_and_stitch_frame(
    app: &AppHandle,
    session_id: &str,
    direction: i64,
) -> Result<(), FlickError> {
    let total_started = Instant::now();
    let selection = {
        let guard = sessions()
            .lock()
            .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?;
        guard
            .get(session_id)
            .map(|session| session.selection.clone())
            .ok_or_else(|| FlickError::Message("long capture session not found".into()))?
    };

    let capture_started = Instant::now();
    let frame = capture_live_frame_with_editor_hidden(&selection)?;
    let capture_ms = capture_started.elapsed().as_millis();

    let update = {
        let mut guard = sessions()
            .lock()
            .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?;
        let session = guard
            .get_mut(session_id)
            .ok_or_else(|| FlickError::Message("long capture session not found".into()))?;
        let shift_hint = free_scroll_shift_hint(session, &frame);
        let preview_update = append_frame_from_free_scroll(session, frame, direction, shift_hint);
        if matches!(preview_update, PreviewUpdate::None) {
            // Truly nothing changed (no movement or low confidence): skip emitting to keep the
            // front-end render loop light. `OffsetOnly` still emits so the preview viewport
            // tracks scrolling back through already-captured content.
            long_log(format!("sampling: no change capture_ms={capture_ms}"));
            return Ok(());
        }
        build_update(session, preview_update)?
    };
    emit_long_capture_update(app, session_id, update)?;
    long_log(format!(
        "sampling: stitched capture_ms={capture_ms} total_ms={}",
        total_started.elapsed().as_millis()
    ));
    Ok(())
}

fn emit_long_capture_update(
    app: &AppHandle,
    session_id: &str,
    update: LongCaptureUpdate,
) -> Result<(), FlickError> {
    if let Some((label, window)) = screenshot_editor_window(app, session_id) {
        long_log(format!(
            "emit_update: window label={label} total_height={} frame_height={}",
            update.total_height, update.frame_height
        ));
        window
            .emit("long-capture-update", update)
            .map_err(|error| {
                FlickError::Message(format!(
                    "failed to emit long capture update to window: {error}"
                ))
            })?;
        return Ok(());
    } else {
        long_log(format!(
            "emit_update: editor window not found; app emit total_height={} frame_height={}",
            update.total_height, update.frame_height
        ));
    }

    app.emit("long-capture-update", update).map_err(|error| {
        FlickError::Message(format!("failed to emit long capture update: {error}"))
    })?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn update_editor_cursor_passthrough(
    app: &AppHandle,
    session_id: &str,
    passthrough: bool,
    current: &Arc<AtomicBool>,
) {
    if current.swap(passthrough, Ordering::SeqCst) == passthrough {
        return;
    }
    set_editor_cursor_passthrough(app, session_id, passthrough);
}

#[cfg(target_os = "macos")]
fn set_editor_cursor_passthrough(app: &AppHandle, session_id: &str, passthrough: bool) {
    if let Some((label, window)) = screenshot_editor_window(app, session_id) {
        long_log(format!(
            "real_scroll_watcher: set_ignore_cursor_events label={label} passthrough={passthrough}"
        ));
        let _ = window.set_ignore_cursor_events(passthrough);
    }
}

pub fn save_long_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<CaptureRecord, FlickError> {
    finalize_long_capture(app, state, session_id, false)
}

pub fn confirm_long_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<CaptureRecord, FlickError> {
    finalize_long_capture(app, state, session_id, true)
}

pub fn cancel_long_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    long_log(format!("cancel: enter session={session_id}"));
    if let Some(session) = sessions()
        .lock()
        .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?
        .remove(&session_id)
    {
        session.stop.store(true, Ordering::SeqCst);
    }
    cleanup_long_capture_ui(&app, &state, &session_id);
    long_log("cancel: complete");
    Ok(())
}

pub fn prepare_long_capture_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    long_log(format!("prepare_edit: enter session={session_id}"));
    if let Some(session) = sessions()
        .lock()
        .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?
        .remove(&session_id)
    {
        session.stop.store(true, Ordering::SeqCst);
    }
    if let Some((label, window)) = screenshot_editor_window(&app, &session_id) {
        long_log(format!(
            "prepare_edit: editor label={label} restore cursor"
        ));
        let _ = window.set_ignore_cursor_events(false);
    }
    platform::set_overlay_mouse_passthrough(&app, false);
    platform::set_overlay_capture_sharing(&app, true);
    long_log("prepare_edit: finalize capture session/overlay start");
    platform::finalize_capture_session(&app, &state, true);
    long_log("prepare_edit: complete");
    Ok(())
}

pub fn open_long_capture_edit_window(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), FlickError> {
    long_log(format!("open_edit_window: enter session={session_id}"));
    let has_session = {
        let guard = sessions()
            .lock()
            .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?;
        if let Some(session) = guard.get(&session_id) {
            session.stop.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    };
    if !has_session {
        return Err(FlickError::Message("long capture session not found".into()));
    }

    let label = format!("screenshot-editor-long-{session_id}");
    if let Some(window) = app.get_webview_window(&label) {
        long_log(format!(
            "open_edit_window: existing window found label={label}; cleanup old capture window"
        ));
        cleanup_long_capture_capture_window(&app, &state, &session_id);
        long_log(format!("open_edit_window: show existing window label={label}"));
        let _ = window.show();
        let _ = window.set_focus();
        long_log("open_edit_window: existing window focused");
        return Ok(());
    }

    let (monitor_x, monitor_y, monitor_width, monitor_height) = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| {
            let scale = monitor.scale_factor();
            (
                monitor.position().x as f64 / scale,
                monitor.position().y as f64 / scale,
                monitor.size().width as f64 / scale,
                monitor.size().height as f64 / scale,
            )
        })
        .unwrap_or((0.0, 0.0, 1280.0, 800.0));
    let width = (monitor_width - 80.0).min(720.0).max(560.0);
    let height = (monitor_height - 80.0).min(700.0).max(360.0);
    let x = monitor_x + (monitor_width - width) / 2.0;
    let y = monitor_y + (monitor_height - height) / 2.0;
    let url = format!("screenshot-editor.html?session_id={session_id}&long_edit=1");
    long_log(format!(
        "open_edit_window: build start label={label} url={url} pos={x:.1},{y:.1} size={width:.1}x{height:.1}"
    ));

    let window = WebviewWindowBuilder::new(&app, label.clone(), WebviewUrl::App(url.into()))
        .title("Flick Screenshot Editor")
        .devtools(false)
        .inner_size(width, height)
        .position(x, y)
        .resizable(true)
        .visible(false)
        .focused(false)
        .always_on_top(false)
        .accept_first_mouse(true)
        .decorations(true)
        .transparent(false)
        .shadow(true)
        .build()?;
    long_log(format!("open_edit_window: build complete label={label}"));
    let _ = window.set_position(LogicalPosition::new(x, y));
    long_log(format!("open_edit_window: cleanup old capture window before frontend show label={label}"));
    cleanup_long_capture_capture_window(&app, &state, &session_id);
    long_log(format!(
        "open_edit_window: opened hidden label=screenshot-editor-long-{session_id} size={}x{}",
        width.round(),
        height.round()
    ));
    Ok(())
}

fn finalize_long_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    copy_to_clipboard: bool,
) -> Result<CaptureRecord, FlickError> {
    long_log(format!(
        "finalize: enter session={session_id} copy_to_clipboard={copy_to_clipboard}"
    ));
    wait_for_pending_capture(&session_id)?;
    let long_session = sessions()
        .lock()
        .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?
        .remove(&session_id)
        .ok_or_else(|| FlickError::Message("long capture session not found".into()))?;
    long_session.stop.store(true, Ordering::SeqCst);
    let image = long_session.stitched;
    long_log(format!(
        "finalize: stitched image {}x{}",
        image.width(),
        image.height()
    ));
    cleanup_long_capture_ui(&app, &state, &session_id);
    let pending = session::remove_pending_capture_edit(&state, &session_id)?;
    long_log(format!(
        "finalize: pending removed final_path={}",
        pending.final_path
    ));
    session::cleanup_pending_original(&pending);

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

    let capture_service = ScreenCaptureService::default();
    long_log(format!("finalize: save png start path={}", record.path));
    capture_service.save_png(&image, Path::new(&record.path))?;
    long_log("finalize: save png complete");
    if copy_to_clipboard {
        long_log("finalize: copy clipboard start");
        capture_service.copy_to_clipboard(&image).map_err(|error| {
            FlickError::Message(format!("failed to copy long screenshot: {error}"))
        })?;
        long_log("finalize: copy clipboard complete");
    }
    long_log("finalize: prune history start");
    history::prune_capture_history(&screenshot_dir, max_screenshots)?;
    long_log("finalize: prune history complete");
    let mut history_guard = state
        .history
        .lock()
        .map_err(|_| FlickError::Message("history mutex poisoned".into()))?;
    history_guard.push_front(record.clone());
    history_guard.truncate(max_screenshots as usize);
    drop(history_guard);
    crate::app::windows::emit_capture_status(&app, "capture-finished", &record);
    long_log("finalize: complete");
    Ok(record)
}

fn cleanup_long_capture_ui(app: &AppHandle, state: &State<'_, AppState>, session_id: &str) {
    long_log("cleanup_ui: restore overlay/editor state start");
    cleanup_long_capture_capture_window(app, state, session_id);
    long_log("cleanup_ui: complete");
}

fn cleanup_long_capture_capture_window(
    app: &AppHandle,
    state: &State<'_, AppState>,
    session_id: &str,
) {
    if let Some((label, window)) = screenshot_editor_window(app, session_id) {
        long_log(format!(
            "cleanup_ui: editor label={label} restore cursor/hide"
        ));
        let _ = window.set_ignore_cursor_events(false);
        let _ = window.hide();
    } else {
        long_log(format!(
            "cleanup_ui: no capture editor window found session={session_id}"
        ));
    }
    platform::set_overlay_mouse_passthrough(app, false);
    platform::set_overlay_capture_sharing(app, true);
    long_log("cleanup_ui: finalize capture session/overlay start");
    platform::finalize_capture_session(app, state, true);
}

fn wait_for_pending_capture(session_id: &str) -> Result<(), FlickError> {
    let capture_pending = {
        let guard = sessions()
            .lock()
            .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?;
        guard
            .get(session_id)
            .map(|session| session.capture_pending.clone())
            .ok_or_else(|| FlickError::Message("long capture session not found".into()))?
    };

    let mut waited = Duration::ZERO;
    while capture_pending.load(Ordering::SeqCst) && waited < FINALIZE_CAPTURE_WAIT_TIMEOUT {
        thread::sleep(FINALIZE_CAPTURE_WAIT_POLL);
        waited += FINALIZE_CAPTURE_WAIT_POLL;
    }

    if waited > Duration::ZERO {
        long_log(format!(
            "finalize: waited_for_pending_capture_ms={} still_pending={}",
            waited.as_millis(),
            capture_pending.load(Ordering::SeqCst)
        ));
    }
    Ok(())
}

fn pending_selection(
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<SelectionRect, FlickError> {
    long_log(format!("pending_selection: lookup session={session_id}"));
    let pending = state
        .pending_capture_edits
        .lock()
        .map_err(|_| FlickError::Message("pending capture edits mutex poisoned".into()))?;
    pending
        .get(session_id)
        .map(|session| session.selection.clone())
        .ok_or_else(|| FlickError::Message("pending capture edit not found".into()))
}

/// Capture the live contents of the selection region from the *current* screen.
///
/// The frozen overlay is kept visible while the user is in long-capture mode. We hide it
/// briefly only for live capture, then restore it immediately so the mask stays full-screen.
fn capture_live_frame(
    app: &AppHandle,
    state: &State<'_, AppState>,
    session_id: &str,
    selection: &SelectionRect,
) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, FlickError> {
    long_log(format!(
        "capture_live_frame: start session={session_id} selection=({},{} {}x{})",
        selection.x, selection.y, selection.width, selection.height
    ));
    let window = screenshot_editor_window(app, session_id);
    long_log(format!(
        "capture_live_frame: editor window found={} label={}",
        window.is_some(),
        window
            .as_ref()
            .map(|(label, _)| label.as_str())
            .unwrap_or("<none>")
    ));
    if let Some((_, window)) = window.as_ref() {
        long_log("capture_live_frame: hide editor start");
        let _ = window.hide();
        long_log("capture_live_frame: hide editor complete");
    }
    long_log("capture_live_frame: hide overlay start");
    platform::hide_overlay_for_live_capture(app, state);
    long_log("capture_live_frame: hide overlay complete");
    thread::sleep(WINDOW_HIDE_DELAY);
    long_log("capture_live_frame: capture live desktop start");
    let result = capture_live_frame_with_editor_hidden(selection);
    match &result {
        Ok(image) => long_log(format!(
            "capture_live_frame: capture live desktop complete {}x{}",
            image.width(),
            image.height()
        )),
        Err(error) => long_log(format!(
            "capture_live_frame: capture live desktop failed {error}"
        )),
    }
    long_log("capture_live_frame: restore overlay start");
    platform::restore_overlay_after_live_capture(app, state, selection);
    long_log("capture_live_frame: restore overlay complete");
    if let Some((_, window)) = window.as_ref() {
        long_log("capture_live_frame: show editor start");
        let _ = window.show();
        let _ = window.set_focus();
        long_log("capture_live_frame: show editor complete");
    }
    result
}

fn screenshot_editor_window(
    app: &AppHandle,
    session_id: &str,
) -> Option<(String, tauri::WebviewWindow)> {
    let session_label = format!("screenshot-editor-{session_id}");
    if let Some(window) = app.get_webview_window(&session_label) {
        return Some((session_label, window));
    }

    let preload_label = "screenshot-editor-preload".to_string();
    app.get_webview_window(&preload_label)
        .map(|window| (preload_label, window))
}

fn capture_live_frame_with_editor_hidden(
    selection: &SelectionRect,
) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, FlickError> {
    long_log(format!(
        "capture_live_frame_with_editor_hidden: service capture start selection=({},{} {}x{})",
        selection.x, selection.y, selection.width, selection.height
    ));
    ScreenCaptureService::default()
        .capture_selection(selection, &[])
        .map_err(FlickError::from)
}

fn long_capture_target_pid(state: &State<'_, AppState>) -> Option<i32> {
    #[cfg(target_os = "macos")]
    {
        return state
            .previous_frontmost_app_pid
            .lock()
            .ok()
            .and_then(|pid| *pid);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = state;
        None
    }
}

fn append_frame_with_delta(
    session: &mut LongCaptureSession,
    frame: ImageBuffer<Rgba<u8>, Vec<u8>>,
    signed_delta: i64,
) -> PreviewUpdate {
    long_log(format!(
        "append_delta: start signed_delta={} stitched={}x{} last={}x{} frame={}x{}",
        signed_delta,
        session.stitched.width(),
        session.stitched.height(),
        session.last_frame.width(),
        session.last_frame.height(),
        frame.width(),
        frame.height()
    ));
    if frame.width() != session.last_frame.width() {
        long_log("append_delta: width changed, replacing stitched image");
        return reset_stitched_to_frame(session, frame);
    }

    if frame.dimensions() == session.last_frame.dimensions()
        && frame.as_raw() == session.last_frame.as_raw()
    {
        session.last_frame = frame;
        long_log("append_delta: frame unchanged, assuming scroll boundary");
        return PreviewUpdate::None;
    }

    if signed_delta == 0 {
        session.last_frame = frame;
        long_log("append_delta: zero delta, not appending");
        return PreviewUpdate::None;
    }

    let new_y = session.current_y + signed_delta;
    if let Some(boundary_y) = unchanged_scroll_boundary_y(session, &frame, signed_delta) {
        let moved = boundary_y != session.current_y;
        session.current_y = boundary_y;
        session.last_frame = frame;
        long_log(format!(
            "append_delta: frame matches stitched boundary y={boundary_y}, not appending moved={moved}"
        ));
        return if moved {
            PreviewUpdate::OffsetOnly
        } else {
            PreviewUpdate::None
        };
    }

    long_log(format!(
        "append_delta: merge signed_delta={signed_delta} new_y={new_y} current_y={}",
        session.current_y
    ));
    merge_frame_by_range(session, frame, new_y)
}

/// Stitch a freely-scrolled frame.
///
/// Under free scrolling there is no reliable wheel-delta prior, so both the direction and the
/// magnitude of the shift are recovered from the image. `direction_hint` (sign of the most
/// recent wheel delta, may be 0) only biases which direction is tried first and seeds the
/// search window; the result comes from the projection match. A low-confidence match (no
/// overlap, repetitive texture) is dropped rather than stitched, to avoid corrupting the image.
fn append_frame_from_free_scroll(
    session: &mut LongCaptureSession,
    frame: ImageBuffer<Rgba<u8>, Vec<u8>>,
    direction_hint: i64,
    shift_hint: u32,
) -> PreviewUpdate {
    long_log(format!(
        "append_free: start direction_hint={direction_hint} shift_hint={shift_hint} stitched={}x{} last={}x{} frame={}x{}",
        session.stitched.width(),
        session.stitched.height(),
        session.last_frame.width(),
        session.last_frame.height(),
        frame.width(),
        frame.height()
    ));

    if frame.dimensions() != session.last_frame.dimensions() {
        long_log("append_free: dimensions changed, replacing stitched image");
        return reset_stitched_to_frame(session, frame);
    }

    // Try the hinted direction first; if it isn't confident, try the other one. On macOS a
    // negative wheel delta moves content down on screen.
    let hint_down = direction_hint < 0;
    let primary = detect_scroll_shift(&session.last_frame, &frame, hint_down, shift_hint);
    let (content_down, measurement) = if primary.confident || direction_hint != 0 {
        if primary.confident {
            (hint_down, primary)
        } else {
            let alt = detect_scroll_shift(&session.last_frame, &frame, !hint_down, shift_hint);
            if alt.confident {
                (!hint_down, alt)
            } else {
                (hint_down, primary)
            }
        }
    } else {
        // No directional hint at all: probe both and keep the more confident one.
        let down = detect_scroll_shift(&session.last_frame, &frame, true, shift_hint);
        let up = detect_scroll_shift(&session.last_frame, &frame, false, shift_hint);
        match (down.confident, up.confident) {
            (true, false) => (true, down),
            (false, true) => (false, up),
            _ => (true, down),
        }
    };

    if !measurement.confident {
        // No trustworthy overlap (scrolled too fast or ambiguous content). Keep the frame as
        // the new reference so the next frame can re-anchor, but do not grow the stitch.
        session.last_frame = frame;
        long_log(format!(
            "append_free: low-confidence shift={} dropped (no stitch)",
            measurement.shift
        ));
        return PreviewUpdate::None;
    }

    // Remember the magnitude so the next frame's search is seeded with the actual scroll speed.
    if measurement.shift > 0 {
        session.last_shift = measurement.shift;
    }
    let signed_delta = if content_down {
        i64::from(measurement.shift)
    } else {
        -i64::from(measurement.shift)
    };
    long_log(format!(
        "append_free: content_down={content_down} shift={} signed_delta={signed_delta}",
        measurement.shift
    ));
    append_frame_with_delta(session, frame, signed_delta)
}

fn unchanged_scroll_boundary_y(
    session: &LongCaptureSession,
    frame: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    signed_delta: i64,
) -> Option<i64> {
    if frame.dimensions() != session.last_frame.dimensions()
        || frame.width() != session.stitched.width()
        || session.stitched.height() < frame.height()
    {
        return None;
    }

    if signed_delta < 0 && session.current_y == session.stitched_range.top {
        let matches = frame_matches_stitched_region(
            frame,
            &session.stitched,
            (session.stitched_range.top - session.stitched_range.top) as u32,
            BOUNDARY_COMPARE_ROWS_PERCENT,
        );
        long_log(format!("boundary_check: top exact_match={matches}"));
        if matches {
            return Some(session.stitched_range.top);
        }
    }

    if signed_delta > 0
        && session.current_y + i64::from(frame.height()) == session.stitched_range.bottom
    {
        let bottom_y = session.stitched_range.bottom - i64::from(frame.height());
        let matches = frame_matches_stitched_region(
            frame,
            &session.stitched,
            (bottom_y - session.stitched_range.top) as u32,
            BOUNDARY_COMPARE_ROWS_PERCENT,
        );
        long_log(format!("boundary_check: bottom exact_match={matches}"));
        if matches {
            return Some(bottom_y);
        }
    }

    None
}

/// Result of measuring the vertical shift between two consecutive frames.
#[derive(Clone, Copy, Debug)]
struct ShiftMeasurement {
    /// Detected vertical shift in pixels (0 = no movement).
    shift: u32,
    /// Whether the measurement is trustworthy enough to stitch with.
    ///
    /// Free scrolling has no reliable wheel-delta prior, so the shift is recovered purely
    /// from the image. A measurement is rejected when the best candidate is not clearly
    /// better than the runner-up (repetitive textures) or when no overlap remains (the user
    /// scrolled faster than a frame height).
    confident: bool,
}

/// Per-row signature of a frame used for vertical-shift detection.
///
/// Each row gets a content hash plus a "trivial" flag. Hashing (rather than summing luminance)
/// means two visually different rows that happen to share a brightness total are still
/// distinguished. The trivial flag marks blank/near-uniform rows (whitespace, solid fills):
/// those rows match each other for free at *any* shift and carry no alignment information, so
/// the matcher must not count them — otherwise a large shift that lines up a band of blank rows
/// scores perfectly and wins, which is exactly the failure we are fixing.
struct RowSignatures {
    hashes: Vec<u64>,
    trivial: Vec<bool>,
}

impl RowSignatures {
    fn from_frame(frame: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> Self {
        let width = frame.width();
        let height = frame.height();
        let col_step = (width / SHIFT_SAMPLE_COLS).max(1);
        let raw = frame.as_raw();
        let row_stride = (width * 4) as usize;
        let mut hashes = Vec::with_capacity(height as usize);
        let mut trivial = Vec::with_capacity(height as usize);
        for row in 0..height {
            let base = row as usize * row_stride;
            // FNV-1a over quantized RGB of the sampled columns gives a content-sensitive hash.
            let mut hash = 0xcbf2_9ce4_8422_2325_u64;
            let mut min_luma = u32::MAX;
            let mut max_luma = 0_u32;
            let mut col = 0;
            while col < width {
                let idx = base + (col as usize) * 4;
                // Quantize to 5 bits per channel so anti-aliasing jitter doesn't change the hash.
                let r = raw[idx] >> 3;
                let g = raw[idx + 1] >> 3;
                let b = raw[idx + 2] >> 3;
                for byte in [r, g, b] {
                    hash ^= u64::from(byte);
                    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                }
                let luma =
                    u32::from(raw[idx]) * 2 + u32::from(raw[idx + 1]) * 5 + u32::from(raw[idx + 2]);
                min_luma = min_luma.min(luma);
                max_luma = max_luma.max(luma);
                col += col_step;
            }
            // A row whose sampled pixels span only a tiny luminance range is "blank" — it has no
            // distinguishing structure to anchor an alignment on.
            trivial.push(max_luma.saturating_sub(min_luma) < TRIVIAL_ROW_LUMA_RANGE);
            hashes.push(hash);
        }
        Self { hashes, trivial }
    }

    fn len(&self) -> u32 {
        self.hashes.len() as u32
    }
}

/// How well two frames align at a candidate `shift`.
///
/// Direction convention (both frames share the selection's coordinate system, row 0 at the top):
/// - `content_down` (scrolled down, looking at later content): the *bottom* of the previous frame
///   overlaps the *top* of the current frame, i.e. `previous[r + shift] == current[r]`.
/// - `!content_down` (scrolled up, looking back): the *bottom* of the current frame overlaps the
///   *top* of the previous frame, i.e. `previous[r] == current[r + shift]`.
///
/// Scoring combines two signals so it works on both dense and sparse/periodic content:
/// - `matched`: overlapping rows where both are non-trivial and their hashes are equal.
/// - `conflicts`: overlapping rows that actively contradict the alignment — a non-trivial row
///   lined up against a blank one, or two non-trivial rows whose hashes differ. A rigid scroll
///   lines blank up with blank and content with content, so its conflicts are ~0; a period-off
///   alignment of repetitive content matches the same content rows but slams content bands into
///   whitespace gaps, producing many conflicts. Ranking by `matched - conflicts` therefore picks
///   the true translation even when a wrong shift matches just as many rows outright.
#[derive(Clone, Copy)]
struct AlignScore {
    matched: u32,
    conflicts: u32,
}

impl AlignScore {
    /// Match fraction in parts-per-thousand: matched / (matched + conflicts). This is the ranking
    /// key. Unlike raw counts it does not reward a small shift for having more overlapping rows —
    /// a true translation and a "barely moved" alignment both score ~1000, and the tie is then
    /// broken toward the hint (the real scroll velocity), which is what stops the detector from
    /// collapsing onto shift≈0 while the user is still scrolling.
    fn match_permille(self) -> u32 {
        let comparable = self.matched + self.conflicts;
        if comparable == 0 {
            0
        } else {
            self.matched * 1000 / comparable
        }
    }
}

fn align_score(
    previous: &RowSignatures,
    current: &RowSignatures,
    content_down: bool,
    shift: u32,
) -> AlignScore {
    let height = previous.len().min(current.len());
    let overlap = height.saturating_sub(shift);
    let min_overlap = (height / 4).max(1);
    if overlap < min_overlap {
        return AlignScore {
            matched: 0,
            conflicts: 0,
        };
    }
    let mut matched = 0_u32;
    let mut conflicts = 0_u32;
    let mut row = 0;
    while row < overlap {
        let (previous_row, current_row) = if content_down {
            (row + shift, row)
        } else {
            (row, row + shift)
        };
        let (pr, cr) = (previous_row as usize, current_row as usize);
        let (p_trivial, c_trivial) = (previous.trivial[pr], current.trivial[cr]);
        if p_trivial && c_trivial {
            // Blank against blank: no evidence either way, neutral.
        } else if p_trivial != c_trivial {
            // Content lined up against whitespace — the frames are not aligned here.
            conflicts += 1;
        } else if previous.hashes[pr] == current.hashes[cr] {
            matched += 1;
        } else {
            conflicts += 1;
        }
        row += 1;
    }
    AlignScore { matched, conflicts }
}

/// Detect the vertical shift between two same-sized frames purely from image content.
///
/// `hint` only seeds and narrows the search; the returned shift is the one that maximizes the
/// number of matched non-trivial rows. A match is accepted only when it explains most of the
/// comparable rows and clearly beats any competing (non-adjacent) alignment.
fn detect_scroll_shift(
    previous: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    current: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    content_down: bool,
    hint: u32,
) -> ShiftMeasurement {
    if previous.dimensions() != current.dimensions() {
        return ShiftMeasurement {
            shift: 0,
            confident: false,
        };
    }
    if previous.as_raw() == current.as_raw() {
        return ShiftMeasurement {
            shift: 0,
            confident: true,
        };
    }

    let previous_sig = RowSignatures::from_frame(previous);
    let current_sig = RowSignatures::from_frame(current);
    let height = previous_sig.len().min(current_sig.len());
    let max_shift = (height.saturating_mul(3) / 4).min(height.saturating_sub(1));
    if max_shift == 0 {
        return ShiftMeasurement {
            shift: 0,
            confident: true,
        };
    }

    // Scan the whole range — maximizing matched rows is cheap (hash compares) and a windowed
    // search around the hint risks missing the true shift when scrolling is fast.
    let best = scan_alignment_range(&previous_sig, &current_sig, content_down, 0, max_shift, hint);

    long_log(format!(
        "detect_shift: content_down={content_down} hint={} best={} quality={} matched={} conflicts={} runner_up_quality={} confident={}",
        hint.min(max_shift),
        best.shift,
        best.quality,
        best.matched,
        best.conflicts,
        best.runner_up_quality,
        best.confident
    ));

    ShiftMeasurement {
        shift: best.shift,
        confident: best.confident,
    }
}

struct AlignmentResult {
    shift: u32,
    quality: u32,
    matched: u32,
    conflicts: u32,
    runner_up_quality: u32,
    confident: bool,
}

fn scan_alignment_range(
    previous: &RowSignatures,
    current: &RowSignatures,
    content_down: bool,
    lo: u32,
    hi: u32,
    hint: u32,
) -> AlignmentResult {
    // Rank by match *fraction*, not raw matched count. Raw counts grow with overlap, so the
    // smallest shift always wins and the detector collapses onto shift≈0 while the user is still
    // scrolling. Among shifts that have enough absolute evidence (`matched >= MIN_QUALITY_ROWS`),
    // pick the highest match fraction; ties (a true scroll and a barely-moved alignment both score
    // ~1000) are broken toward the hint, i.e. the real scroll velocity.
    let mut best_shift = 0_u32;
    let mut best = AlignScore {
        matched: 0,
        conflicts: 0,
    };
    let mut have_best = false;
    for shift in lo..=hi {
        let score = align_score(previous, current, content_down, shift);
        if score.matched < MIN_QUALITY_ROWS {
            continue;
        }
        let better = !have_best
            || score.match_permille() > best.match_permille()
            || (score.match_permille() == best.match_permille()
                && shift.abs_diff(hint) < best_shift.abs_diff(hint));
        if better {
            best = score;
            best_shift = shift;
            have_best = true;
        }
    }

    // Runner-up: the highest match fraction among shifts that are neither adjacent to the winner
    // nor to the hint, with enough absolute evidence. The hint is the expected scroll, so a strong
    // alignment near it is not a competitor; only a strong unrelated alignment signals ambiguity.
    let mut runner_up_permille = 0_u32;
    for shift in lo..=hi {
        if shift.abs_diff(best_shift) <= SHIFT_NEIGHBOR_GUARD
            || shift.abs_diff(hint) <= SHIFT_NEIGHBOR_GUARD
        {
            continue;
        }
        let score = align_score(previous, current, content_down, shift);
        if score.matched < MIN_QUALITY_ROWS {
            continue;
        }
        if score.match_permille() > runner_up_permille {
            runner_up_permille = score.match_permille();
        }
    }

    // Confident when the winner has enough absolute evidence and a high match fraction. We do not
    // additionally require it to strictly beat the runner-up: with conflicts already penalizing
    // misalignment and ties broken toward the hint, an equal-fraction alignment elsewhere is the
    // unavoidable ambiguity of truly periodic content, which the hint has already resolved.
    let enough_absolute = have_best && best.matched >= MIN_QUALITY_ROWS;
    let high_fraction = best.match_permille() >= MIN_MATCH_PERMILLE;
    let confident = enough_absolute && high_fraction;

    AlignmentResult {
        shift: best_shift,
        quality: best.match_permille(),
        matched: best.matched,
        conflicts: best.conflicts,
        runner_up_quality: runner_up_permille,
        confident,
    }
}

fn frame_matches_stitched_region(
    frame: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    stitched: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    stitched_y: u32,
    rows_percent: f64,
) -> bool {
    if stitched_y.saturating_add(frame.height()) > stitched.height()
        || frame.width() != stitched.width()
    {
        return false;
    }

    let rows_percent = rows_percent.clamp(0.0, 100.0);
    let rows_to_compare = if rows_percent >= 100.0 {
        frame.height()
    } else {
        ((frame.height() as f64) * (rows_percent / 100.0))
            .ceil()
            .max(1.0) as u32
    }
    .min(frame.height());

    let mut row = 0;
    while row < rows_to_compare {
        let mut col = 0;
        while col < frame.width() {
            let a = frame.get_pixel(col, row).0;
            let b = stitched.get_pixel(col, stitched_y + row).0;
            // Tolerant comparison: under free scrolling the same content re-rendered in a later
            // frame can differ by a few levels per channel (HiDPI font anti-aliasing, subpixel
            // rounding), so a strict equality check would spuriously reject a true match.
            if a[0].abs_diff(b[0]) > PIXEL_MATCH_TOLERANCE
                || a[1].abs_diff(b[1]) > PIXEL_MATCH_TOLERANCE
                || a[2].abs_diff(b[2]) > PIXEL_MATCH_TOLERANCE
            {
                return false;
            }
            col += 1;
        }
        row += 1;
    }

    true
}

fn reset_stitched_to_frame(
    session: &mut LongCaptureSession,
    frame: ImageBuffer<Rgba<u8>, Vec<u8>>,
) -> PreviewUpdate {
    session.stitched = frame.clone();
    session.stitched_range = CaptureRange::from_top_height(0, frame.height());
    session.current_y = 0;
    session.last_frame = frame;
    PreviewUpdate::Replace
}

fn merge_frame_by_range(
    session: &mut LongCaptureSession,
    frame: ImageBuffer<Rgba<u8>, Vec<u8>>,
    frame_y: i64,
) -> PreviewUpdate {
    let width = session.stitched.width();
    let old_range = session.stitched_range;
    let frame_range = CaptureRange::from_top_height(frame_y, frame.height());

    long_log(format!(
        "merge_range: frame=[{}, {}) stitched=[{}, {}) current_y={}",
        frame_range.top, frame_range.bottom, old_range.top, old_range.bottom, session.current_y
    ));

    if old_range.contains(frame_range) {
        let moved = frame_y != session.current_y;
        session.current_y = frame_y;
        session.last_frame = frame;
        long_log(format!(
            "merge_range: frame already covered, no stitched growth moved={moved}"
        ));
        return if moved {
            PreviewUpdate::OffsetOnly
        } else {
            PreviewUpdate::None
        };
    }

    let grows_top = frame_range.top < old_range.top;
    let grows_bottom = frame_range.bottom > old_range.bottom;

    let new_range = CaptureRange {
        top: old_range.top.min(frame_range.top),
        bottom: old_range.bottom.max(frame_range.bottom),
    };
    let mut grown = ImageBuffer::from_pixel(width, new_range.height(), Rgba([255, 255, 255, 255]));
    image::imageops::overlay(
        &mut grown,
        &session.stitched,
        0,
        old_range.top - new_range.top,
    );

    if grows_top {
        let new_rows = (old_range.top - frame_range.top) as u32;
        let prepended = frame_rows(&frame, 0, new_rows);
        image::imageops::overlay(&mut grown, &prepended, 0, frame_range.top - new_range.top);
    }

    let bottom_append = if grows_bottom {
        let new_rows = (new_range.bottom - old_range.bottom) as u32;
        let src_top = (old_range.bottom - frame_range.top).max(0) as u32;
        let appended = frame_rows(&frame, src_top, new_rows);
        image::imageops::overlay(&mut grown, &appended, 0, old_range.bottom - new_range.top);
        Some((new_rows, appended))
    } else {
        None
    };

    let preview_update = if grows_top {
        long_log(format!(
            "merge_range: prepend top old_top={} new_top={}",
            old_range.top, new_range.top
        ));
        PreviewUpdate::Replace
    } else if let Some((new_rows, appended)) = bottom_append {
        long_log(format!(
            "merge_range: append bottom old_bottom={} new_bottom={} new_rows={}",
            old_range.bottom, new_range.bottom, new_rows
        ));
        PreviewUpdate::Append {
            rows: new_rows,
            image: appended,
        }
    } else {
        PreviewUpdate::None
    };

    session.stitched = grown;
    session.stitched_range = new_range;
    session.current_y = frame_y;
    session.last_frame = frame;
    long_log(format!(
        "merge_range: stitched updated range=[{}, {}) height={}",
        session.stitched_range.top,
        session.stitched_range.bottom,
        session.stitched.height(),
    ));
    preview_update
}

fn frame_rows(
    frame: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    src_top: u32,
    rows: u32,
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    ImageBuffer::from_fn(frame.width(), rows, |col, row| {
        *frame.get_pixel(col, src_top + row)
    })
}

/// Coarse seed for the shift search under free scrolling.
///
/// Scroll speed is continuous frame-to-frame, so the best predictor of this frame's shift is the
/// last confidently-detected one. This keeps the hint tracking the real scroll velocity, which is
/// what disambiguates periodic content (the alignment nearest the hint is the true scroll). Until
/// a shift has been measured, fall back to a modest fraction of the frame height.
#[cfg(target_os = "macos")]
fn free_scroll_shift_hint(
    session: &LongCaptureSession,
    frame: &ImageBuffer<Rgba<u8>, Vec<u8>>,
) -> u32 {
    if session.last_shift > 0 {
        session.last_shift
    } else {
        ((frame.height() as f64) * 0.15).round().max(1.0) as u32
    }
}

fn build_update(
    session: &LongCaptureSession,
    preview_update: PreviewUpdate,
) -> Result<LongCaptureUpdate, FlickError> {
    let total_started = Instant::now();
    long_log(format!(
        "build_update: stitched={}x{} last={}x{} selection_height={}",
        session.stitched.width(),
        session.stitched.height(),
        session.last_frame.width(),
        session.last_frame.height(),
        session.selection.height
    ));
    let current_started = Instant::now();
    let current_frame_data_url = image_to_data_url(&session.last_frame)?;
    let current_ms = current_started.elapsed().as_millis();
    let preview_started = Instant::now();
    let (preview_data_url, preview_append_data_url, preview_append_rows, preview_kind) =
        match preview_update {
            PreviewUpdate::Replace => {
                let preview = preview_image(&session.stitched);
                let preview_resize_ms = preview_started.elapsed().as_millis();
                let preview_encode_started = Instant::now();
                let preview_data_url = image_to_data_url(&preview)?;
                let preview_encode_ms = preview_encode_started.elapsed().as_millis();
                long_log(format!(
                    "build_update: preview replace resize_ms={} encode_ms={} len={}",
                    preview_resize_ms,
                    preview_encode_ms,
                    preview_data_url.len()
                ));
                (preview_data_url, String::new(), 0, "replace")
            }
            PreviewUpdate::Append { rows, image } => {
                let preview = preview_image(&image);
                let preview_resize_ms = preview_started.elapsed().as_millis();
                let preview_encode_started = Instant::now();
                let append_data_url = image_to_data_url(&preview)?;
                let preview_encode_ms = preview_encode_started.elapsed().as_millis();
                long_log(format!(
                    "build_update: preview append rows={} resize_ms={} encode_ms={} len={}",
                    rows,
                    preview_resize_ms,
                    preview_encode_ms,
                    append_data_url.len()
                ));
                (String::new(), append_data_url, rows, "append")
            }
            PreviewUpdate::OffsetOnly => (String::new(), String::new(), 0, "offset_only"),
            PreviewUpdate::None => (String::new(), String::new(), 0, "none"),
        };
    long_log(format!(
        "build_update: encode current_ms={} preview_kind={} total_ms={} current_len={} preview_len={} preview_append_len={} preview_append_rows={}",
        current_ms,
        preview_kind,
        total_started.elapsed().as_millis(),
        current_frame_data_url.len(),
        preview_data_url.len(),
        preview_append_data_url.len(),
        preview_append_rows
    ));
    Ok(LongCaptureUpdate {
        current_frame_data_url,
        preview_data_url,
        preview_append_data_url,
        preview_append_rows,
        width: session.stitched.width(),
        frame_height: session.last_frame.height(),
        total_height: session.stitched.height(),
        scroll_offset: (session.current_y - session.stitched_range.top) as i32,
        min_offset: 0,
        max_offset: (session.stitched_range.bottom
            - session.stitched_range.top
            - i64::from(session.last_frame.height()))
        .max(0) as i32,
    })
}

fn preview_image(image: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    if image.width() <= LONG_PREVIEW_WIDTH {
        return image.clone();
    }
    let height = ((image.height() as f64) * (LONG_PREVIEW_WIDTH as f64 / image.width() as f64))
        .round()
        .max(1.0) as u32;
    image::imageops::resize(image, LONG_PREVIEW_WIDTH, height, ResizeFilterType::Nearest)
}

fn image_to_data_url(image: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> Result<String, FlickError> {
    let mut png_bytes = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(
        &mut png_bytes,
        CompressionType::Fast,
        PngFilterType::Adaptive,
    )
    .write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ColorType::Rgba8.into(),
    )
    .map_err(|error| FlickError::Message(format!("failed to encode long screenshot: {error}")))?;
    Ok(format!(
        "data:image/png;base64,{}",
        general_purpose::STANDARD.encode(png_bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tall synthetic "page" where each row has a distinct color.
    fn synthetic_page(width: u32, height: u32) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        ImageBuffer::from_fn(width, height, |x, y| {
            let r = (y % 256) as u8;
            let g = ((y / 256) % 256) as u8;
            let b = ((x + y) % 256) as u8;
            Rgba([r, g, b, 255])
        })
    }

    /// Extract the rows [top, top + frame_height) from the page as a single frame.
    fn frame_at(
        page: &ImageBuffer<Rgba<u8>, Vec<u8>>,
        top: u32,
        frame_height: u32,
    ) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        ImageBuffer::from_fn(page.width(), frame_height, |x, y| {
            *page.get_pixel(x, top + y)
        })
    }

    fn session_with(frame: ImageBuffer<Rgba<u8>, Vec<u8>>, height: u32) -> LongCaptureSession {
        LongCaptureSession {
            selection: SelectionRect {
                x: 0,
                y: 0,
                width: frame.width(),
                height,
            },
            stitched: frame.clone(),
            stitched_range: CaptureRange::from_top_height(0, frame.height()),
            current_y: 0,
            last_frame: frame,
            last_shift: 0,
            capture_pending: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn delta_scroll_appends_bottom_without_duplication_or_gaps() {
        let frame_height = 300;
        let page = synthetic_page(120, 1000);
        let mut session = session_with(frame_at(&page, 0, frame_height), frame_height);

        // Scroll the page down in 150px steps and feed each frame in.
        for top in (150..=600).step_by(150) {
            let frame = frame_at(&page, top, frame_height);
            append_frame_with_delta(&mut session, frame, 150);
        }

        // After scrolling to top=600 with a 300px window, the stitched image should cover
        // rows [0, 900) of the original page, exactly and without duplication.
        assert_eq!(session.stitched.height(), 900);
        for y in 0..session.stitched.height() {
            for x in 0..session.stitched.width() {
                assert_eq!(
                    session.stitched.get_pixel(x, y),
                    page.get_pixel(x, y),
                    "mismatch at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn delta_scroll_inside_covered_range_does_not_grow() {
        let frame_height = 300;
        let page = synthetic_page(120, 1000);
        let mut session = session_with(frame_at(&page, 0, frame_height), frame_height);
        append_frame_with_delta(&mut session, frame_at(&page, 150, frame_height), 150);
        append_frame_with_delta(&mut session, frame_at(&page, 300, frame_height), 150);

        assert_eq!(
            session.stitched_range,
            CaptureRange {
                top: 0,
                bottom: 600
            }
        );
        assert_eq!(session.current_y, 300);

        append_frame_with_delta(&mut session, frame_at(&page, 150, frame_height), -150);

        assert_eq!(
            session.stitched_range,
            CaptureRange {
                top: 0,
                bottom: 600
            }
        );
        assert_eq!(session.current_y, 150);
        assert_eq!(session.stitched.height(), 600);
    }

    #[test]
    fn unchanged_boundary_frame_does_not_prepend_when_scrolling_past_origin() {
        let frame_height = 300;
        let page = synthetic_page(120, 1000);
        let mut session = session_with(frame_at(&page, 0, frame_height), frame_height);

        append_frame_with_delta(&mut session, frame_at(&page, 150, frame_height), 150);
        append_frame_with_delta(&mut session, frame_at(&page, 300, frame_height), 150);
        append_frame_with_delta(&mut session, frame_at(&page, 150, frame_height), -150);
        append_frame_with_delta(&mut session, frame_at(&page, 0, frame_height), -150);
        append_frame_with_delta(&mut session, frame_at(&page, 0, frame_height), -150);

        assert_eq!(
            session.stitched_range,
            CaptureRange {
                top: 0,
                bottom: 600
            }
        );
        assert_eq!(session.current_y, 0);
    }

    #[test]
    fn scrolling_back_into_covered_region_reports_offset_change() {
        let frame_height = 300;
        let page = synthetic_page(120, 1000);
        let mut session = session_with(frame_at(&page, 0, frame_height), frame_height);

        // Grow downward so [0, 600) is covered with current_y at 300.
        append_frame_with_delta(&mut session, frame_at(&page, 150, frame_height), 150);
        append_frame_with_delta(&mut session, frame_at(&page, 300, frame_height), 150);
        assert_eq!(session.current_y, 300);

        // Scroll back up into already-covered content: the image must not grow, but the
        // viewport offset moved, so the update must be OffsetOnly (not None) so the front-end
        // preview can follow.
        let update = append_frame_with_delta(&mut session, frame_at(&page, 150, frame_height), -150);
        assert!(
            matches!(update, PreviewUpdate::OffsetOnly),
            "expected OffsetOnly when scrolling back into covered region"
        );
        assert_eq!(session.current_y, 150);
        assert_eq!(
            session.stitched_range,
            CaptureRange { top: 0, bottom: 600 },
            "stitched image must not grow when revisiting covered rows"
        );

        // Scrolling back down through covered content also reports an offset change.
        let update = append_frame_with_delta(&mut session, frame_at(&page, 300, frame_height), 150);
        assert!(matches!(update, PreviewUpdate::OffsetOnly));
        assert_eq!(session.current_y, 300);
    }

    /// A page whose per-row luminance is strictly monotonic (and thus non-periodic), so the
    /// vertical-shift detector has a single unambiguous alignment. `synthetic_page` repeats every
    /// 256 rows in its row projection, which is unrealistic for real screenshots and confuses a
    /// 1-D projection match, so shift tests use this instead.
    fn monotonic_page(width: u32, height: u32) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        ImageBuffer::from_fn(width, height, |x, y| {
            let v = ((y * 251 + x * 7) % 65536) as u32;
            Rgba([(v >> 8) as u8, (v & 0xff) as u8, ((y * 3) & 0xff) as u8, 255])
        })
    }

    #[test]
    fn detect_scroll_shift_recovers_known_shift_both_directions() {
        let page = monotonic_page(120, 1000);
        let frame_height = 254;
        let previous = frame_at(&page, 200, frame_height);

        // Scrolled down: the new frame reveals later content (rows 240..494). With content_down,
        // previous.row[r + shift] matches current.row[r] → 200 + r + shift = 240 + r → shift 40.
        let current_down = frame_at(&page, 240, frame_height);
        let down = detect_scroll_shift(&previous, &current_down, true, 30);
        assert!(down.confident, "down shift should be confident");
        assert_eq!(down.shift, 40, "down shift magnitude");

        // Scrolled up: the new frame reveals earlier content (rows 160..414). With !content_down,
        // previous.row[r] matches current.row[r + shift] → 200 + r = 160 + r + shift → shift 40.
        let current_up = frame_at(&page, 160, frame_height);
        let up = detect_scroll_shift(&previous, &current_up, false, 30);
        assert!(up.confident, "up shift should be confident");
        assert_eq!(up.shift, 40, "up shift magnitude");

        // Querying the wrong direction must not produce a confident nonzero shift for the pair.
        let down_wrong = detect_scroll_shift(&previous, &current_up, true, 30);
        assert!(
            !down_wrong.confident || down_wrong.shift == 0,
            "querying the wrong direction must not yield a confident nonzero shift"
        );
    }

    /// A mostly-blank page: solid white except for a thin band of distinctive content every
    /// `content_period` rows. This is the case that broke the projection matcher — blank rows
    /// align for free at any shift, so a naive "least difference" match snaps to a wrong large
    /// shift that happens to line up whitespace.
    fn sparse_page(width: u32, height: u32, content_period: u32) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        ImageBuffer::from_fn(width, height, |x, y| {
            if y % content_period < 3 {
                let v = ((y * 131 + x * 17) % 65536) as u32;
                Rgba([(v >> 8) as u8, (v & 0xff) as u8, ((x * 5) & 0xff) as u8, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        })
    }

    #[test]
    fn detect_scroll_shift_ignores_blank_rows() {
        // The frame is mostly whitespace with content bands every 40 rows. A small real scroll
        // (shift 12) must be recovered from the content bands, not be hijacked by the many blank
        // rows that match each other at large shifts.
        let page = sparse_page(120, 1000, 40);
        let frame_height = 254;
        let previous = frame_at(&page, 200, frame_height);
        let current = frame_at(&page, 212, frame_height); // scrolled down 12px

        let measurement = detect_scroll_shift(&previous, &current, true, 10);
        assert!(
            measurement.confident,
            "content bands should give a confident match"
        );
        assert_eq!(
            measurement.shift, 12,
            "shift must come from content rows, not blank-row alignment"
        );
    }

    /// A list/table-like page: a content band every `period` rows, but each band's content is
    /// distinct (real lists have different text per row). The *layout* is periodic, the *content*
    /// is not — so a period-off alignment slams differing content together and racks up conflicts.
    fn periodic_page(width: u32, height: u32, period: u32) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        ImageBuffer::from_fn(width, height, |x, y| {
            let phase = y % period;
            if phase < 4 {
                let band = y / period; // distinct content per band
                let v = ((band * 2657 + phase * 97 + x * 13) % 65536) as u32;
                Rgba([(v >> 8) as u8, (v & 0xff) as u8, ((band * 53 + x * 11) & 0xff) as u8, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        })
    }

    #[test]
    fn detect_scroll_shift_resolves_periodic_layout_with_distinct_content() {
        // Bands every 24 rows but each band differs. A period-off shift would line content bands
        // up with the wrong (different) bands and against whitespace, so the quality metric — not
        // the hint — must still pick the true 18px shift.
        let page = periodic_page(120, 1000, 24);
        let frame_height = 254;
        let previous = frame_at(&page, 240, frame_height);
        let current = frame_at(&page, 258, frame_height); // real scroll: 18px down

        let measurement = detect_scroll_shift(&previous, &current, true, 18);
        assert!(
            measurement.confident,
            "distinct-content bands should resolve confidently from the alignment quality"
        );
        assert_eq!(measurement.shift, 18, "must pick the true translation");
    }

    #[test]
    fn detect_scroll_shift_does_not_collapse_to_tiny_shift() {
        // Reproduces the "scrolls but barely moves" bug. A tiny shift always overlaps more rows
        // than the true large shift, so ranking by raw matched count collapses onto ~0. Ranking
        // by match *fraction* must recover the true large shift in both directions.
        let page = monotonic_page(120, 1000);
        let frame_height = 254;
        let previous = frame_at(&page, 300, frame_height);

        // Scrolled down 90px: only shift 90 has a high match fraction; tiny shifts mismatch.
        let down = detect_scroll_shift(&previous, &frame_at(&page, 390, frame_height), true, 80);
        assert!(down.confident);
        assert_eq!(down.shift, 90, "down: must not collapse onto a tiny shift");

        // Scrolled up 90px.
        let up = detect_scroll_shift(&previous, &frame_at(&page, 210, frame_height), false, 80);
        assert!(up.confident);
        assert_eq!(up.shift, 90, "up: must not collapse onto a tiny shift");
    }

    #[test]
    fn detect_scroll_shift_rejects_non_overlapping_frames() {
        // Two completely unrelated frames (scrolled far past a frame height) share no real
        // alignment, so the detector must not confidently report a shift.
        let page = monotonic_page(120, 2000);
        let frame_height = 254;
        let previous = frame_at(&page, 0, frame_height);
        let current = frame_at(&page, 1500, frame_height);
        let measurement = detect_scroll_shift(&previous, &current, true, 30);
        assert!(
            !measurement.confident,
            "non-overlapping frames must be rejected, got shift={}",
            measurement.shift
        );
    }

    #[test]
    fn boundary_region_match_can_compare_partial_rows() {
        let frame_height = 100;
        let page = synthetic_page(20, frame_height);
        let frame = frame_at(&page, 0, frame_height);
        let mut stitched = frame.clone();
        stitched.put_pixel(0, 75, Rgba([0, 0, 0, 255]));

        assert!(frame_matches_stitched_region(&frame, &stitched, 0, 50.0));
        assert!(!frame_matches_stitched_region(&frame, &stitched, 0, 100.0));
    }

    #[test]
    fn free_scroll_recovers_shift_from_image_at_boundary() {
        let frame_height = 400;
        // Non-periodic content so the row-hash match has a single unambiguous alignment.
        let page = monotonic_page(120, 1000);
        let mut session = session_with(frame_at(&page, 0, frame_height), frame_height);

        // direction_hint < 0 means content moved down on screen (macOS convention). The actual
        // shift (200) is recovered from the row-hash match, not from the hint.
        append_frame_from_free_scroll(&mut session, frame_at(&page, 200, frame_height), -1, 180);
        append_frame_from_free_scroll(&mut session, frame_at(&page, 0, frame_height), 1, 180);

        let mut boundary_frame = frame_at(&page, 0, frame_height);
        boundary_frame.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        append_frame_from_free_scroll(&mut session, boundary_frame, 1, 180);

        assert_eq!(
            session.stitched_range,
            CaptureRange {
                top: 0,
                bottom: 600
            }
        );
        assert_eq!(session.current_y, 0);
    }

    #[test]
    fn bottom_first_capture_can_prepend_new_rows_when_scrolling_above_origin() {
        let frame_height = 300;
        let page = synthetic_page(120, 1000);
        let mut session = session_with(frame_at(&page, 300, frame_height), frame_height);

        append_frame_with_delta(&mut session, frame_at(&page, 450, frame_height), 150);
        append_frame_with_delta(&mut session, frame_at(&page, 600, frame_height), 150);
        append_frame_with_delta(&mut session, frame_at(&page, 450, frame_height), -150);
        append_frame_with_delta(&mut session, frame_at(&page, 300, frame_height), -150);
        append_frame_with_delta(&mut session, frame_at(&page, 150, frame_height), -150);

        assert_eq!(
            session.stitched_range,
            CaptureRange {
                top: -150,
                bottom: 600
            }
        );
        assert_eq!(session.current_y, -150);
        assert_eq!(session.stitched.height(), 750);
        for y in 0..session.stitched.height() {
            for x in 0..session.stitched.width() {
                assert_eq!(
                    session.stitched.get_pixel(x, y),
                    page.get_pixel(x, 150 + y),
                    "mismatch at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn delta_scroll_preserves_existing_overlap_rows() {
        let frame_height = 300;
        let page = synthetic_page(120, 1000);
        let mut session = session_with(frame_at(&page, 0, frame_height), frame_height);
        let mut frame = frame_at(&page, 150, frame_height);

        for y in 0..150 {
            for x in 0..frame.width() {
                frame.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }

        append_frame_with_delta(&mut session, frame, 150);

        assert_eq!(
            session.stitched_range,
            CaptureRange {
                top: 0,
                bottom: 450
            }
        );
        for y in 0..300 {
            for x in 0..session.stitched.width() {
                assert_eq!(
                    session.stitched.get_pixel(x, y),
                    page.get_pixel(x, y),
                    "existing row was overwritten at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn reverse_scroll_prepends_missing_top_rows() {
        let frame_height = 300;
        let page = synthetic_page(120, 1000);
        let mut session = session_with(frame_at(&page, 300, frame_height), frame_height);
        session.current_y = 300;
        session.stitched_range = CaptureRange {
            top: 300,
            bottom: 600,
        };

        let frame = frame_at(&page, 150, frame_height);
        merge_frame_by_range(&mut session, frame, 150);

        assert_eq!(
            session.stitched_range,
            CaptureRange {
                top: 150,
                bottom: 600,
            }
        );
        assert_eq!(session.stitched.height(), 450);
        for y in 0..session.stitched.height() {
            for x in 0..session.stitched.width() {
                assert_eq!(
                    session.stitched.get_pixel(x, y),
                    page.get_pixel(x, 150 + y),
                    "mismatch at ({x}, {y})"
                );
            }
        }
    }
}
