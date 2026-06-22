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

/// Hide the editor window before a live capture for this long so the OS has time to
/// composite the frame without the editor on top of it.
const WINDOW_HIDE_DELAY: Duration = Duration::from_millis(70);
const PRE_SCROLL_HIDE_DELAY: Duration = Duration::from_millis(50);
const SCROLL_SETTLE_DELAY: Duration = Duration::from_millis(180);
const FINALIZE_CAPTURE_WAIT_TIMEOUT: Duration = Duration::from_millis(1500);
const FINALIZE_CAPTURE_WAIT_POLL: Duration = Duration::from_millis(25);
const LONG_PREVIEW_WIDTH: u32 = 240;
const CONTROLLED_SCROLL_STEP_RATIO: f64 = 0.72;
const SHIFT_SAMPLE_COLS: u32 = 24;
const SHIFT_SAMPLE_ROWS: u32 = 64;
const BOUNDARY_COMPARE_ROWS_PERCENT: f64 = 100.0;

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

pub fn scroll_long_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    delta_y: f64,
) -> Result<LongCaptureUpdate, FlickError> {
    long_log(format!(
        "scroll: enter session={session_id} delta_y={delta_y}"
    ));
    let selection = {
        let guard = sessions()
            .lock()
            .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?;
        guard
            .get(&session_id)
            .map(|session| session.selection.clone())
            .ok_or_else(|| FlickError::Message("long capture session not found".into()))?
    };
    long_log(format!(
        "scroll: selection x={} y={} width={} height={}",
        selection.x, selection.y, selection.width, selection.height
    ));

    let editor_window = screenshot_editor_window(&app, &session_id);
    long_log(format!(
        "scroll: editor window found={} label={}",
        editor_window.is_some(),
        editor_window
            .as_ref()
            .map(|(label, _)| label.as_str())
            .unwrap_or("<none>")
    ));
    if let Some((_, window)) = editor_window.as_ref() {
        long_log("scroll: hide editor window start");
        let _ = window.hide();
        long_log("scroll: hide editor window complete");
    }
    long_log("scroll: hide overlay start");
    platform::hide_overlay_for_live_capture(&app, &state);
    long_log("scroll: hide overlay complete");
    thread::sleep(PRE_SCROLL_HIDE_DELAY);
    long_log("scroll: platform scroll start");
    let target_pid = long_capture_target_pid(&state);
    long_log(format!("scroll: target pid={target_pid:?}"));
    if let Err(error) = platform::scroll_for_long_capture(delta_y, &selection, target_pid) {
        long_log(format!("scroll: platform scroll failed: {error}"));
        platform::restore_overlay_after_live_capture(&app, &state, &selection);
        if let Some((_, window)) = editor_window.as_ref() {
            let _ = window.show();
            let _ = window.set_focus();
        }
        return Err(error);
    }
    long_log("scroll: platform scroll complete");
    thread::sleep(SCROLL_SETTLE_DELAY);
    long_log("scroll: capture live frame start");
    let frame = capture_live_frame_with_editor_hidden(&selection)?;
    long_log(format!(
        "scroll: capture live frame complete {}x{}",
        frame.width(),
        frame.height()
    ));
    long_log("scroll: restore overlay start");
    platform::restore_overlay_after_live_capture(&app, &state, &selection);
    long_log("scroll: restore overlay complete");
    if let Some((_, window)) = editor_window.as_ref() {
        long_log("scroll: show editor window start");
        let _ = window.show();
        let _ = window.set_focus();
        long_log("scroll: show editor window complete");
    }
    let mut guard = sessions()
        .lock()
        .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?;
    let session = guard
        .get_mut(&session_id)
        .ok_or_else(|| FlickError::Message("long capture session not found".into()))?;
    let before_height = session.stitched.height();
    let expected_shift = expected_shift_pixels(session, &frame, delta_y.abs().max(1.0));
    let signed_delta = if delta_y >= 0.0 {
        i64::from(expected_shift)
    } else {
        -i64::from(expected_shift)
    };
    let preview_update = append_frame_with_delta(session, frame, signed_delta);
    long_log(format!(
        "scroll: append complete before_height={} after_height={} last_frame={}x{}",
        before_height,
        session.stitched.height(),
        session.last_frame.width(),
        session.last_frame.height()
    ));
    let update = build_update(session, preview_update)?;
    long_log(format!(
        "scroll: update built total_height={} offset={} preview_len={}",
        update.total_height,
        update.scroll_offset,
        update.preview_data_url.len()
    ));
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
    let event_types = vec![CGEventType::MouseMoved, CGEventType::ScrollWheel];
    let app_for_tap = app.clone();
    let session_for_tap = session_id.clone();
    let selection_for_tap = selection.clone();
    let stop_for_tap = stop.clone();
    let capture_pending_for_tap = capture_pending.clone();
    let cursor_passthrough_for_tap = cursor_passthrough.clone();
    let last_scroll_millis_for_tap = last_scroll_millis.clone();
    let last_scroll_delta_for_tap = last_scroll_delta.clone();
    let ignore_controlled_scroll = Arc::new(AtomicBool::new(false));
    let ignore_controlled_scroll_for_tap = ignore_controlled_scroll.clone();

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
                if ignore_controlled_scroll_for_tap.load(Ordering::SeqCst) {
                    return CallbackResult::Keep;
                }
                let delta_y = scroll_delta_y(event);
                last_scroll_millis_for_tap.store(monotonic_millis(), Ordering::SeqCst);
                last_scroll_delta_for_tap.store(delta_y.signum().round() as i64, Ordering::SeqCst);
                if !capture_pending_for_tap.load(Ordering::SeqCst) {
                    long_log(format!(
                        "real_scroll_watcher: controlled wheel trigger session={session_for_tap} delta_y={delta_y} location={},{} pending=false",
                        location.x, location.y
                    ));
                }
                schedule_real_scroll_capture(
                    app_for_tap.clone(),
                    session_for_tap.clone(),
                    capture_pending_for_tap.clone(),
                    stop_for_tap.clone(),
                    last_scroll_millis_for_tap.clone(),
                    last_scroll_delta_for_tap.clone(),
                    target_pid,
                    ignore_controlled_scroll.clone(),
                );
                return CallbackResult::Drop;
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

#[cfg(target_os = "macos")]
fn schedule_real_scroll_capture(
    app: AppHandle,
    session_id: String,
    capture_pending: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    last_scroll_millis: Arc<AtomicI64>,
    last_scroll_delta: Arc<AtomicI64>,
    target_pid: Option<i32>,
    ignore_controlled_scroll: Arc<AtomicBool>,
) {
    if capture_pending
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    thread::spawn(move || {
        loop {
            let wait_started = Instant::now();
            loop {
                if stop.load(Ordering::SeqCst) {
                    capture_pending.store(false, Ordering::SeqCst);
                    return;
                }
                let elapsed_since_scroll =
                    monotonic_millis() - last_scroll_millis.load(Ordering::SeqCst);
                let remaining = SCROLL_SETTLE_DELAY
                    .as_millis()
                    .saturating_sub(elapsed_since_scroll.max(0) as u128);
                if remaining == 0 {
                    break;
                }
                thread::sleep(Duration::from_millis(remaining.min(20) as u64));
            }
            let captured_scroll_millis = last_scroll_millis.load(Ordering::SeqCst);
            long_log(format!(
                "real_scroll_capture: debounce complete waited_ms={} scroll_marker={captured_scroll_millis}",
                wait_started.elapsed().as_millis()
            ));
            if stop.load(Ordering::SeqCst) {
                capture_pending.store(false, Ordering::SeqCst);
                return;
            }

            let direction = last_scroll_delta.load(Ordering::SeqCst);
            if let Err(error) = capture_after_controlled_scroll(
                app.clone(),
                &session_id,
                direction,
                target_pid,
                ignore_controlled_scroll.clone(),
            ) {
                long_log(format!("real_scroll_watcher: capture failed {error}"));
            }

            let latest_scroll_millis = last_scroll_millis.load(Ordering::SeqCst);
            if latest_scroll_millis <= captured_scroll_millis || stop.load(Ordering::SeqCst) {
                capture_pending.store(false, Ordering::SeqCst);
                return;
            }
            long_log(format!(
                "real_scroll_capture: scroll changed during capture; continue latest_marker={latest_scroll_millis} captured_marker={captured_scroll_millis}"
            ));
        }
    });
}

#[cfg(target_os = "macos")]
fn capture_after_controlled_scroll(
    app: AppHandle,
    session_id: &str,
    direction: i64,
    target_pid: Option<i32>,
    ignore_controlled_scroll: Arc<AtomicBool>,
) -> Result<(), FlickError> {
    let total_started = Instant::now();
    long_log(format!(
        "real_scroll_capture: start controlled session={session_id} direction={direction}"
    ));
    let selection = {
        let guard = sessions()
            .lock()
            .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?;
        guard
            .get(session_id)
            .map(|session| session.selection.clone())
            .ok_or_else(|| FlickError::Message("long capture session not found".into()))?
    };
    if direction == 0 {
        long_log("real_scroll_capture: zero direction, skip");
        return Ok(());
    }

    let logical_step = controlled_scroll_step_points(&selection);
    let content_down = wheel_direction_moves_content_down(direction);
    let scroll_delta_y = controlled_scroll_delta_y(direction, logical_step);
    long_log(format!(
        "real_scroll_capture: controlled scroll logical_step={logical_step:.1} delta_y={scroll_delta_y:.1} content_down={content_down} target_pid={target_pid:?}"
    ));
    ignore_controlled_scroll.store(true, Ordering::SeqCst);
    let scroll_result = platform::scroll_for_long_capture(scroll_delta_y, &selection, target_pid);
    thread::sleep(Duration::from_millis(30));
    ignore_controlled_scroll.store(false, Ordering::SeqCst);
    scroll_result?;
    thread::sleep(SCROLL_SETTLE_DELAY);

    let editor_window = screenshot_editor_window(&app, session_id);
    if let Some((_, window)) = editor_window.as_ref() {
        long_log("real_scroll_capture: exclude editor window from capture start");
        platform::set_window_capture_sharing(window, false);
        long_log("real_scroll_capture: exclude editor window from capture complete");
    }
    long_log("real_scroll_capture: exclude overlay from capture start");
    platform::set_overlay_capture_sharing(&app, false);
    long_log("real_scroll_capture: exclude overlay from capture complete");
    thread::sleep(WINDOW_HIDE_DELAY);
    let capture_started = Instant::now();
    let frame_result = capture_live_frame_with_editor_hidden(&selection);
    let capture_ms = capture_started.elapsed().as_millis();
    long_log("real_scroll_capture: restore capture sharing start");
    platform::set_overlay_capture_sharing(&app, true);
    if let Some((_, window)) = editor_window.as_ref() {
        platform::set_window_capture_sharing(window, true);
    }
    long_log("real_scroll_capture: restore capture sharing complete");
    let frame = frame_result?;
    long_log(format!(
        "real_scroll_capture: captured frame {}x{} capture_ms={}",
        frame.width(),
        frame.height(),
        capture_ms
    ));

    let update = {
        let stitch_started = Instant::now();
        let mut guard = sessions()
            .lock()
            .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?;
        let session = guard
            .get_mut(session_id)
            .ok_or_else(|| FlickError::Message("long capture session not found".into()))?;
        // macOS real wheel events report negative axis-1 deltas for scrolling down.
        let expected_shift = expected_shift_pixels(session, &frame, logical_step);
        let preview_update =
            append_frame_with_expected_shift(session, frame, content_down, expected_shift);
        let stitch_ms = stitch_started.elapsed().as_millis();
        long_log(format!("real_scroll_capture: stitch_ms={stitch_ms}"));
        let update_started = Instant::now();
        let update = build_update(session, preview_update)?;
        long_log(format!(
            "real_scroll_capture: build_update_ms={}",
            update_started.elapsed().as_millis()
        ));
        update
    };
    let emit_started = Instant::now();
    emit_long_capture_update(&app, session_id, update)?;
    long_log(format!(
        "real_scroll_capture: update emitted emit_ms={} total_ms={}",
        emit_started.elapsed().as_millis(),
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
        session.current_y = boundary_y;
        session.last_frame = frame;
        long_log(format!(
            "append_delta: frame matches stitched boundary y={boundary_y}, not appending"
        ));
        return PreviewUpdate::None;
    }

    long_log(format!(
        "append_delta: merge signed_delta={signed_delta} new_y={new_y} current_y={}",
        session.current_y
    ));
    merge_frame_by_range(session, frame, new_y)
}

fn append_frame_with_expected_shift(
    session: &mut LongCaptureSession,
    frame: ImageBuffer<Rgba<u8>, Vec<u8>>,
    content_down: bool,
    expected_shift: u32,
) -> PreviewUpdate {
    long_log(format!(
        "append_controlled: start content_down={} expected_shift={} stitched={}x{} last={}x{} frame={}x{}",
        content_down,
        expected_shift,
        session.stitched.width(),
        session.stitched.height(),
        session.last_frame.width(),
        session.last_frame.height(),
        frame.width(),
        frame.height()
    ));
    let expected_shift = expected_shift.min(frame.height());
    let detected_shift =
        detect_scroll_shift(&session.last_frame, &frame, content_down, expected_shift);
    let signed_delta = if content_down {
        i64::from(detected_shift)
    } else {
        -i64::from(detected_shift)
    };
    long_log(format!(
        "append_controlled: detected_shift={} signed_delta={}",
        detected_shift, signed_delta
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

fn detect_scroll_shift(
    previous: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    current: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    content_down: bool,
    expected_shift: u32,
) -> u32 {
    if previous.dimensions() != current.dimensions() {
        return expected_shift;
    }
    if previous.as_raw() == current.as_raw() {
        return 0;
    }

    let max_shift = expected_shift.min(current.height().saturating_sub(1));
    let mut best_shift = 0_u32;
    let mut best_score = u64::MAX;
    for shift in 0..=max_shift {
        let score = shift_match_score(previous, current, content_down, shift);
        if score < best_score
            || (score == best_score
                && shift.abs_diff(expected_shift) < best_shift.abs_diff(expected_shift))
        {
            best_score = score;
            best_shift = shift;
        }
    }
    long_log(format!(
        "detect_shift: content_down={} expected={} best={} score={}",
        content_down, expected_shift, best_shift, best_score
    ));
    best_shift
}

fn shift_match_score(
    previous: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    current: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    content_down: bool,
    shift: u32,
) -> u64 {
    let overlap_height = previous.height().saturating_sub(shift).max(1);
    let row_step = (overlap_height / SHIFT_SAMPLE_ROWS).max(1);
    let col_step = (previous.width() / SHIFT_SAMPLE_COLS).max(1);
    let mut total = 0_u64;
    let mut samples = 0_u64;

    let mut row = 0;
    while row < overlap_height {
        let (previous_y, current_y) = if content_down {
            (row + shift, row)
        } else {
            (row, row + shift)
        };
        let mut col = 0;
        while col < previous.width() {
            let previous_pixel = previous.get_pixel(col, previous_y).0;
            let current_pixel = current.get_pixel(col, current_y).0;
            total +=
                (i32::from(previous_pixel[0]) - i32::from(current_pixel[0])).unsigned_abs() as u64;
            total +=
                (i32::from(previous_pixel[1]) - i32::from(current_pixel[1])).unsigned_abs() as u64;
            total +=
                (i32::from(previous_pixel[2]) - i32::from(current_pixel[2])).unsigned_abs() as u64;
            samples += 3;
            col += col_step;
        }
        row += row_step;
    }

    if samples == 0 {
        return u64::MAX;
    }
    total / samples
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
            if frame.get_pixel(col, row) != stitched.get_pixel(col, stitched_y + row) {
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
        session.current_y = frame_y;
        session.last_frame = frame;
        long_log("merge_range: frame already covered, no stitched growth");
        return PreviewUpdate::None;
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

fn controlled_scroll_step_points(selection: &SelectionRect) -> f64 {
    (selection.height as f64 * CONTROLLED_SCROLL_STEP_RATIO)
        .round()
        .max(1.0)
}

fn wheel_direction_moves_content_down(direction: i64) -> bool {
    #[cfg(target_os = "macos")]
    {
        direction < 0
    }

    #[cfg(not(target_os = "macos"))]
    {
        direction < 0
    }
}

#[cfg(target_os = "macos")]
fn controlled_scroll_delta_y(direction: i64, logical_step: f64) -> f64 {
    if direction < 0 {
        logical_step
    } else {
        -logical_step
    }
}

fn expected_shift_pixels(
    session: &LongCaptureSession,
    frame: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    logical_step: f64,
) -> u32 {
    let scale = if session.selection.height == 0 {
        1.0
    } else {
        frame.height() as f64 / session.selection.height as f64
    };
    let expected = (logical_step * scale).round().max(1.0) as u32;
    let clamped = expected.min(frame.height());
    long_log(format!(
        "append_controlled: scale={scale:.3} logical_step={logical_step:.1} expected_shift_px={expected} clamped={clamped}"
    ));
    clamped
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
    fn controlled_scroll_uses_detected_shift_at_boundary() {
        let frame_height = 400;
        let page = synthetic_page(120, 1000);
        let mut session = session_with(frame_at(&page, 0, frame_height), frame_height);

        append_frame_with_expected_shift(
            &mut session,
            frame_at(&page, 288, frame_height),
            true,
            288,
        );
        append_frame_with_expected_shift(
            &mut session,
            frame_at(&page, 0, frame_height),
            false,
            288,
        );

        let mut boundary_frame = frame_at(&page, 0, frame_height);
        boundary_frame.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        append_frame_with_expected_shift(&mut session, boundary_frame, false, 288);

        assert_eq!(
            session.stitched_range,
            CaptureRange {
                top: 0,
                bottom: 688
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
