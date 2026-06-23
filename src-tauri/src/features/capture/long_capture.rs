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
/// Length of one scroll-throttle budget window. Deliberately shorter than `SAMPLE_INTERVAL` so a
/// The throttle window matches the sampling interval, so one captured frame corresponds to one
/// budget window. (A previous shorter window let two windows' budgets land inside a single frame,
/// doubling the effective per-frame motion and overrunning `max_shift` on fast flings.)
const THROTTLE_WINDOW_MS: i64 = 60;
/// Physical pixels of scroll allowed per window, as a fraction of the (physical) frame height.
/// `max_shift` is ~0.75·height, so 0.40·height keeps even a frame that straddles a window boundary
/// (~1.5 budgets ≈ 0.60·height) safely under `max_shift`, guaranteeing the frames still overlap.
/// Lower = safer against drops but more scroll "drag"; higher = less drag.
const THROTTLE_MAX_SHIFT_FRACTION: f64 = 0.40;
/// Number of columns sampled per row when building its content hash.
const SHIFT_SAMPLE_COLS: u32 = 64;
/// Rows whose sampled pixels span less than this (weighted) luminance range are treated as
/// blank/trivial and excluded from alignment scoring.
const TRIVIAL_ROW_LUMA_RANGE: u32 = 48;
/// Minimum number of matched non-trivial rows required to trust a frame's located position.
const MIN_QUALITY_ROWS: u32 = 6;
/// Minimum rows the frame must overlap the stitch to be locatable; also caps how far the frame may
/// hang off either end (i.e. the largest growth a single frame can add).
const MIN_OVERLAP_ROWS: i64 = 24;
/// Minimum match fraction (parts per thousand) of matched-vs-comparable rows for a trusted
/// located position. High enough to reject misaligned content, low enough to tolerate the few
/// rows that legitimately differ at the edges.
const MIN_MATCH_PERMILLE: u32 = 750;

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
    /// Consecutive frames that failed absolute location. Diagnostic only; successful location
    /// resets it.
    failed_locate_count: u32,
    /// Set while a real-wheel capture worker is waiting/capturing after a scroll.
    capture_pending: Arc<AtomicBool>,
    /// Set when the session ends; the scroll watcher thread observes this and exits.
    stop: Arc<AtomicBool>,
}

/// Most recently observed capture scale (physical pixels per logical point), ×1000, written by
/// the sampling loop and read by the event-tap throttle. Scroll-wheel deltas arrive in logical
/// points but the stitcher works in physical pixels, so the throttle needs this to size its
/// budget in the same units the shift detector uses. Starts at 1000 (scale 1.0) until measured.
#[cfg(target_os = "macos")]
static CAPTURE_SCALE_X1000: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1000);

#[cfg(target_os = "macos")]
fn capture_scale() -> f64 {
    (CAPTURE_SCALE_X1000.load(Ordering::SeqCst) as f64 / 1000.0).clamp(0.5, 4.0)
}

enum PreviewUpdate {
    Replace,
    Append {
        rows: u32,
        image: ImageBuffer<Rgba<u8>, Vec<u8>>,
    },
    /// New rows added at the top (scrolling up). Sent incrementally so the growing preview isn't
    /// re-encoded every frame, which is what made scrolling up heavy and laggy versus scrolling
    /// down (which already used `Append`).
    Prepend {
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
        failed_locate_count: 0,
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

    // Scroll-speed throttle state. We cap how much the page may scroll per throttle window so a
    // fast fling can't move further than the stitcher can follow between frames. `window_start`
    // marks the current budget window; `emitted` is how much scroll (PHYSICAL pixels) has already
    // been let through this window. Budgeting in physical pixels matches `max_shift`, which is
    // what the detector can actually recover. The window is short (half the sampling interval) so
    // a single captured frame straddles at most ~2 windows, keeping per-frame motion well under
    // `max_shift` even across a window boundary.
    let selection_height_logical = selection.height.max(1) as f64;
    let throttle_window_start_for_tap = Arc::new(AtomicI64::new(0));
    let throttle_emitted_for_tap = Arc::new(AtomicI64::new(0));

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
                // Free-scroll model: the wheel event passes through to the real target window so
                // the user scrolls it directly. But we throttle the scroll speed: within each
                // sampling window the page may move at most `max_delta_per_window`, so a fast
                // fling can't outrun the stitcher and drop content.
                let delta_y = scroll_delta_y(event);
                if delta_y == 0.0 {
                    return CallbackResult::Keep;
                }

                let now = monotonic_millis();
                // Reset the budget at the start of each throttle window (== one sampling frame).
                let window_start = throttle_window_start_for_tap.load(Ordering::SeqCst);
                if now - window_start >= THROTTLE_WINDOW_MS {
                    throttle_window_start_for_tap.store(now, Ordering::SeqCst);
                    throttle_emitted_for_tap.store(0, Ordering::SeqCst);
                }

                // Budget in physical pixels: the page may move at most this far per window before
                // the next frame is captured. `max_shift ≈ height * 0.75`; the safety factor keeps
                // it under that even if a frame straddles a window boundary.
                let scale = capture_scale();
                let max_physical_per_window =
                    (selection_height_logical * scale * THROTTLE_MAX_SHIFT_FRACTION).max(1.0);
                let emitted = throttle_emitted_for_tap.load(Ordering::SeqCst) as f64;
                let budget_physical = (max_physical_per_window - emitted).max(0.0);
                // Convert this event's logical delta to physical pixels for budgeting.
                let magnitude_physical = delta_y.abs() * scale;

                if budget_physical <= 0.0 {
                    // Budget spent; swallow the event. The remaining scroll continues next window.
                    return CallbackResult::Drop;
                }

                let allowed_physical = magnitude_physical.min(budget_physical);
                let factor = if magnitude_physical > 0.0 {
                    allowed_physical / magnitude_physical
                } else {
                    1.0
                };
                throttle_emitted_for_tap.store(
                    (emitted + allowed_physical).round() as i64,
                    Ordering::SeqCst,
                );

                last_scroll_millis_for_tap.store(now, Ordering::SeqCst);
                last_scroll_delta_for_tap.store(delta_y.signum().round() as i64, Ordering::SeqCst);
                ensure_sampling_running(
                    app_for_tap.clone(),
                    session_for_tap.clone(),
                    capture_pending_for_tap.clone(),
                    stop_for_tap.clone(),
                    last_scroll_millis_for_tap.clone(),
                    last_scroll_delta_for_tap.clone(),
                );

                if factor < 1.0 {
                    // Throttled: scale the event down to the remaining budget before it reaches
                    // the target window.
                    scale_scroll_event(event, factor);
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

/// Scale every vertical-axis delta field of a scroll event by `factor` (0..1). All three delta
/// representations are scaled together so the target app sees a consistent, throttled scroll
/// regardless of which field it reads.
#[cfg(target_os = "macos")]
fn scale_scroll_event(event: &core_graphics::event::CGEvent, factor: f64) {
    let factor = factor.clamp(0.0, 1.0);

    let point = event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1);
    if point != 0 {
        let scaled = ((point as f64) * factor).round() as i64;
        // Preserve direction even when rounding would hit zero, so the wheel still registers.
        let scaled = if scaled == 0 { point.signum() } else { scaled };
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1, scaled);
    }

    let fixed =
        event.get_double_value_field(EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1);
    if fixed != 0.0 {
        event.set_double_value_field(
            EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1,
            fixed * factor,
        );
    }

    let line = event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1);
    if line != 0 {
        let scaled = ((line as f64) * factor).round() as i64;
        let scaled = if scaled == 0 { line.signum() } else { scaled };
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1, scaled);
    }
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

    // Publish the capture scale (physical px per logical point) so the throttle can size its
    // budget in physical pixels — the same units the shift detector and `max_shift` use.
    if selection.height > 0 {
        let scale = (frame.height() as f64) / (selection.height as f64);
        CAPTURE_SCALE_X1000.store((scale * 1000.0).round() as i64, Ordering::SeqCst);
    }

    let update = {
        let mut guard = sessions()
            .lock()
            .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?;
        let session = guard
            .get_mut(session_id)
            .ok_or_else(|| FlickError::Message("long capture session not found".into()))?;
        let preview_update = append_frame_from_free_scroll(session, frame, direction);
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
        long_log(format!("prepare_edit: editor label={label} restore cursor"));
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
        long_log(format!(
            "open_edit_window: show existing window label={label}"
        ));
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
    long_log(format!(
        "open_edit_window: cleanup old capture window before frontend show label={label}"
    ));
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

fn append_frame_from_free_scroll(
    session: &mut LongCaptureSession,
    frame: ImageBuffer<Rgba<u8>, Vec<u8>>,
    direction_hint: i64,
) -> PreviewUpdate {
    if frame.width() != session.stitched.width() {
        long_log("append_free: width changed, replacing stitched image");
        return reset_stitched_to_frame(session, frame);
    }

    let frame_sig = RowSignatures::from_frame(&frame);
    let stitched_sig = RowSignatures::from_frame(&session.stitched);

    // Hint the search toward where the frame should be, biased by the last position and the wheel
    // direction (macOS: negative delta scrolls content down → frame top moves to larger y).
    let dir = if direction_hint < 0 {
        1
    } else if direction_hint > 0 {
        -1
    } else {
        0
    };
    let hint_top_y = session.current_y
        + dir * i64::from(session.last_shift.max(1)).min(i64::from(frame.height()));

    let located = locate_frame_in_stitched(
        &stitched_sig,
        session.stitched_range.top,
        &frame_sig,
        hint_top_y,
    );

    let Some(location) = located.location else {
        // No confident overlap with the stitch (scrolled past the captured region). Drop the
        // frame without touching the stitch or current_y; a later overlapping frame re-anchors.
        session.failed_locate_count = session.failed_locate_count.saturating_add(1);
        long_log(format!(
            "append_free: no confident location, frame dropped failures={} reason={} direction_hint={} dir={} last_shift={} hint_top_y={} current_y={} stitched=[{}, {}) frame_h={} stitched_h={} frame_nontrivial={} stitched_nontrivial={} search_offset=[{}, {}] hint_offset={} best_top_y={} best_offset={} best_matched={} best_conflicts={} best_comparable={} best_match_permille={}",
            session.failed_locate_count,
            located.diagnostics.reject_reason,
            direction_hint,
            dir,
            session.last_shift,
            hint_top_y,
            session.current_y,
            session.stitched_range.top,
            session.stitched_range.bottom,
            located.diagnostics.frame_h,
            located.diagnostics.stitched_h,
            located.diagnostics.frame_nontrivial,
            located.diagnostics.stitched_nontrivial,
            located.diagnostics.lo,
            located.diagnostics.hi,
            located.diagnostics.hint_offset,
            located.diagnostics.best_top_y,
            located.diagnostics.best_offset,
            located.diagnostics.best_matched,
            located.diagnostics.best_conflicts,
            located.diagnostics.best_comparable,
            located.diagnostics.best_match_permille,
        ));
        return PreviewUpdate::None;
    };
    session.failed_locate_count = 0;

    let new_y = location.top_y;
    // Track scroll speed for the next frame's search hint.
    let moved = (new_y - session.current_y).unsigned_abs();
    if moved > 0 {
        session.last_shift = moved.min(u64::from(frame.height())) as u32;
    }
    long_log(format!(
        "append_free: located top_y={new_y} current_y={} matched={} conflicts={} stitched=[{}, {}) direction_hint={} dir={} last_shift={} hint_top_y={} hint_error={}",
        session.current_y,
        location.matched,
        location.conflicts,
        session.stitched_range.top,
        session.stitched_range.bottom,
        direction_hint,
        dir,
        session.last_shift,
        hint_top_y,
        new_y - hint_top_y
    ));

    merge_frame_by_range(session, frame, new_y)
}

/// Per-row content signature of an image. Each row gets a hash (FNV-1a over quantized RGB of
/// sampled columns) plus a "trivial" flag for blank/near-uniform rows. Used to locate a frame
/// inside the stitched image: matching rows by hash, ignoring blank rows that carry no position
/// information.
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

    fn nontrivial_rows(&self) -> u32 {
        self.trivial.iter().filter(|trivial| !**trivial).count() as u32
    }
}

/// Result of locating a freshly captured frame inside the stitched image.
struct FrameLocation {
    /// Long-capture y-coordinate of the frame's top row. May be negative (frame extends above the
    /// stitched top) or place the frame's bottom below the stitched bottom — those are the cases
    /// that grow the stitch.
    top_y: i64,
    /// How many non-trivial frame rows matched the stitched image at this position.
    matched: u32,
    /// How many non-trivial rows actively conflicted (content vs blank, or differing hashes).
    conflicts: u32,
}

struct LocateResult {
    location: Option<FrameLocation>,
    diagnostics: LocateDiagnostics,
}

struct LocateDiagnostics {
    frame_h: i64,
    stitched_h: i64,
    frame_nontrivial: u32,
    stitched_nontrivial: u32,
    lo: i64,
    hi: i64,
    hint_offset: i64,
    best_top_y: i64,
    best_offset: i64,
    best_matched: u32,
    best_conflicts: u32,
    best_comparable: u32,
    best_match_permille: u32,
    reject_reason: &'static str,
}

/// Locate `frame` inside the stitched image by sliding its row signatures over the stitch and
/// picking the position with the best (matched - conflicts) score.
///
/// This is the heart of the stitcher: instead of accumulating per-frame deltas (which drift when
/// scrolling back and forth), every frame is positioned *absolutely* against the already-stitched
/// content. `hint_top_y` (the previous frame's position, give or take) biases ties so periodic or
/// blank content resolves to the nearest plausible spot. The frame is allowed to hang off either
/// end of the stitch by up to `frame_height - MIN_OVERLAP_ROWS`, which is exactly the case that
/// extends the stitch. Returns `None` when no position overlaps the stitch confidently (the user
/// scrolled past the captured region — that frame is dropped, and a later overlapping frame
/// re-anchors automatically).
fn locate_frame_in_stitched(
    stitched_sig: &RowSignatures,
    stitched_top: i64,
    frame_sig: &RowSignatures,
    hint_top_y: i64,
) -> LocateResult {
    let frame_h = frame_sig.len() as i64;
    let stitched_h = stitched_sig.len() as i64;
    let frame_nontrivial = frame_sig.nontrivial_rows();
    let stitched_nontrivial = stitched_sig.nontrivial_rows();
    let mut diagnostics = LocateDiagnostics {
        frame_h,
        stitched_h,
        frame_nontrivial,
        stitched_nontrivial,
        lo: 0,
        hi: 0,
        hint_offset: hint_top_y - stitched_top,
        best_top_y: stitched_top,
        best_offset: 0,
        best_matched: 0,
        best_conflicts: 0,
        best_comparable: 0,
        best_match_permille: 0,
        reject_reason: "not_evaluated",
    };

    if frame_h == 0 || stitched_h == 0 {
        diagnostics.reject_reason = "empty_frame_or_stitch";
        return LocateResult {
            location: None,
            diagnostics,
        };
    }

    // `offset` is the stitched row index aligned with the frame's top row. Allow the frame to hang
    // off either end, keeping at least MIN_OVERLAP_ROWS in common so there is evidence to match on.
    let min_overlap = (MIN_OVERLAP_ROWS as i64).min(frame_h).max(1);
    let lo = -(frame_h - min_overlap);
    let hi = stitched_h - min_overlap;
    diagnostics.lo = lo;
    diagnostics.hi = hi;
    if hi < lo {
        diagnostics.reject_reason = "invalid_search_window";
        return LocateResult {
            location: None,
            diagnostics,
        };
    }

    let hint_offset = hint_top_y - stitched_top;
    let mut best: Option<FrameLocation> = None;
    let mut best_offset = 0_i64;
    for offset in lo..=hi {
        let mut matched = 0_u32;
        let mut conflicts = 0_u32;
        // Overlapping rows: frame row r aligns with stitched row (offset + r).
        let r_start = (-offset).max(0);
        let r_end = (stitched_h - offset).min(frame_h);
        let mut r = r_start;
        while r < r_end {
            let f = r as usize;
            let s = (offset + r) as usize;
            let (ft, st) = (frame_sig.trivial[f], stitched_sig.trivial[s]);
            if ft && st {
                // blank vs blank: neutral
            } else if ft != st {
                conflicts += 1;
            } else if frame_sig.hashes[f] == stitched_sig.hashes[s] {
                matched += 1;
            } else {
                conflicts += 1;
            }
            r += 1;
        }

        let score = matched as i64 - conflicts as i64;
        let best_score = best
            .as_ref()
            .map(|b| b.matched as i64 - b.conflicts as i64)
            .unwrap_or(i64::MIN);
        let better = score > best_score
            || (score == best_score
                && score > 0
                && (offset - hint_offset).abs() < (best_offset - hint_offset).abs());
        if better {
            best = Some(FrameLocation {
                top_y: stitched_top + offset,
                matched,
                conflicts,
            });
            best_offset = offset;
        }
    }

    let Some(best) = best else {
        diagnostics.reject_reason = "no_candidate";
        return LocateResult {
            location: None,
            diagnostics,
        };
    };
    // Trust the location only if it has enough matched rows and few conflicts relative to matches.
    let comparable = best.matched + best.conflicts;
    diagnostics.best_top_y = best.top_y;
    diagnostics.best_offset = best_offset;
    diagnostics.best_matched = best.matched;
    diagnostics.best_conflicts = best.conflicts;
    diagnostics.best_comparable = comparable;
    diagnostics.best_match_permille = if comparable > 0 {
        best.matched.saturating_mul(1000) / comparable
    } else {
        0
    };
    if best.matched < MIN_QUALITY_ROWS {
        diagnostics.reject_reason = "too_few_matched_rows";
        return LocateResult {
            location: None,
            diagnostics,
        };
    }
    if comparable == 0 {
        diagnostics.reject_reason = "no_comparable_rows";
        return LocateResult {
            location: None,
            diagnostics,
        };
    }
    if best.matched * 1000 < comparable * MIN_MATCH_PERMILLE {
        diagnostics.reject_reason = "match_ratio_too_low";
        return LocateResult {
            location: None,
            diagnostics,
        };
    }
    diagnostics.reject_reason = "accepted";
    LocateResult {
        location: Some(best),
        diagnostics,
    }
}

fn reset_stitched_to_frame(
    session: &mut LongCaptureSession,
    frame: ImageBuffer<Rgba<u8>, Vec<u8>>,
) -> PreviewUpdate {
    session.stitched = frame.clone();
    session.stitched_range = CaptureRange::from_top_height(0, frame.height());
    session.current_y = 0;
    session.last_frame = frame;
    session.failed_locate_count = 0;
    PreviewUpdate::Replace
}

/// Re-anchor a frame against the already-stitched image instead of trusting accumulated deltas.
///
/// When scrolling back through covered content, `current_y` is otherwise maintained purely by
/// summing per-frame shifts; any small mis-measurement at a direction change drifts permanently
/// and accumulates, which corrupts the stitch position. Here we slide the frame's row signatures
/// over the stitched image (near the delta-estimated position) and snap `current_y` to the best
/// match, eliminating drift. Returns `None` if no confident match is found, leaving the estimate.
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
        // `frame_y` is already an absolute position from locating the frame in the stitch, so we
        // just move the viewport there — no drift to correct.
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

    let top_prepend = if grows_top {
        let new_rows = (old_range.top - frame_range.top) as u32;
        let prepended = frame_rows(&frame, 0, new_rows);
        image::imageops::overlay(&mut grown, &prepended, 0, frame_range.top - new_range.top);
        Some((new_rows, prepended))
    } else {
        None
    };

    let bottom_append = if grows_bottom {
        let new_rows = (new_range.bottom - old_range.bottom) as u32;
        let src_top = (old_range.bottom - frame_range.top).max(0) as u32;
        let appended = frame_rows(&frame, src_top, new_rows);
        image::imageops::overlay(&mut grown, &appended, 0, old_range.bottom - new_range.top);
        Some((new_rows, appended))
    } else {
        None
    };

    let preview_update = if grows_top && grows_bottom {
        // Grew at both ends in one frame (rare): fall back to a full replace.
        long_log("merge_range: grew both ends, full replace");
        PreviewUpdate::Replace
    } else if let Some((new_rows, prepended)) = top_prepend {
        long_log(format!(
            "merge_range: prepend top old_top={} new_top={} new_rows={}",
            old_range.top, new_range.top, new_rows
        ));
        PreviewUpdate::Prepend {
            rows: new_rows,
            image: prepended,
        }
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
    let (
        preview_data_url,
        preview_append_data_url,
        preview_append_rows,
        preview_prepend_data_url,
        preview_prepend_rows,
        preview_kind,
    ) = match preview_update {
        PreviewUpdate::Replace => {
            let preview = preview_image(&session.stitched);
            let preview_data_url = image_to_data_url(&preview)?;
            long_log(format!(
                "build_update: preview replace len={}",
                preview_data_url.len()
            ));
            (
                preview_data_url,
                String::new(),
                0,
                String::new(),
                0,
                "replace",
            )
        }
        PreviewUpdate::Append { rows, image } => {
            let preview = preview_image(&image);
            let append_data_url = image_to_data_url(&preview)?;
            long_log(format!(
                "build_update: preview append rows={rows} len={}",
                append_data_url.len()
            ));
            (
                String::new(),
                append_data_url,
                rows,
                String::new(),
                0,
                "append",
            )
        }
        PreviewUpdate::Prepend { rows, image } => {
            let preview = preview_image(&image);
            let prepend_data_url = image_to_data_url(&preview)?;
            long_log(format!(
                "build_update: preview prepend rows={rows} len={}",
                prepend_data_url.len()
            ));
            (
                String::new(),
                String::new(),
                0,
                prepend_data_url,
                rows,
                "prepend",
            )
        }
        PreviewUpdate::OffsetOnly => (
            String::new(),
            String::new(),
            0,
            String::new(),
            0,
            "offset_only",
        ),
        PreviewUpdate::None => (String::new(), String::new(), 0, String::new(), 0, "none"),
    };
    long_log(format!(
        "build_update: encode current_ms={} preview_kind={} total_ms={} current_len={} preview_len={} append_len={} append_rows={} prepend_len={} prepend_rows={}",
        current_ms,
        preview_kind,
        total_started.elapsed().as_millis(),
        current_frame_data_url.len(),
        preview_data_url.len(),
        preview_append_data_url.len(),
        preview_append_rows,
        preview_prepend_data_url.len(),
        preview_prepend_rows
    ));
    Ok(LongCaptureUpdate {
        current_frame_data_url,
        preview_data_url,
        preview_append_data_url,
        preview_append_rows,
        preview_prepend_data_url,
        preview_prepend_rows,
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
            failed_locate_count: 0,
            capture_pending: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}
