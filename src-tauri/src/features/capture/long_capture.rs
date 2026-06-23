use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
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
// Sampling cadence. Capturing + stitching one frame costs ~40-50ms, so the practical floor is
// around there; 30ms targets "as fast as processing allows" rather than idling between frames.
// Denser sampling halves the per-frame scroll displacement, which keeps the inter-frame overlap
// large enough to align reliably even when the user scrolls fast.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(30);
/// How long scrolling must be quiet (covering trackpad inertia) before the sampling loop stops.
const SCROLL_IDLE_STOP: Duration = Duration::from_millis(280);
/// Number of columns sampled per row when building its content hash.
const SHIFT_SAMPLE_COLS: u32 = 64;
/// Number of equal-width segments a row is divided into for its luminance feature vector. Each
/// segment stores the mean luminance of its columns, which averages out sub-pixel / Retina
/// resampling jitter that an exact hash cannot absorb.
const ROW_FEATURE_SEGMENTS: usize = 32;
/// Full feature length: segment means concatenated with the vertical gradient (this row's segment
/// means minus the previous row's). The gradient captures vertical texture, which differs between
/// adjacent text lines and so suppresses neighbor-row cross-matching that plain means cannot.
const ROW_FEATURE_LEN: usize = ROW_FEATURE_SEGMENTS * 2;
/// Minimum per-frame scroll (px) accepted while a scroll direction is known. Below this, a match is
/// almost certainly the self-similar near-neighbor artifact (the page offset by ~1 line still
/// correlates at ~1000‰) rather than real motion, and accepting it self-locks the search at a crawl.
/// Kept small so genuinely slow (e.g. inertia tail) scrolling still registers; ±1..±3 px is dropped.
const MIN_SCROLL_DELTA: i64 = 4;
/// Zero-mean normalized cross-correlation (ZNCC) above this threshold counts two rows as the same
/// content. It is invariant to overall brightness/contrast shifts, so the absolute jitter from
/// smooth scrolling on Retina displays does not break the match.
const ROW_CORR_THRESHOLD: f32 = 0.90;
/// A row whose feature vector has standard deviation below this has no distinguishing structure to
/// correlate against and is treated as blank for matching purposes.
const ROW_FEATURE_MIN_STDDEV: f32 = 2.0;
/// Minimum number of continuously matched rows required to trust a frame's located position.
const MIN_QUALITY_ROWS: u32 = 6;
/// Minimum rows the frame must overlap the stitch to be locatable; also caps how far the frame may
/// hang off either end (i.e. the largest growth a single frame can add). Also the minimum overlap
/// required to accept a delta: large enough that the mean-correlation score is statistically
/// trustworthy (a handful of rows can correlate by chance), small enough that a fast scroll leaving
/// only a sliver of overlap can still be aligned.
const MIN_OVERLAP_ROWS: i64 = 80;
/// Consecutive failed locates after which the scroll-speed prior (`last_shift`) is cleared, so a
/// poisoned prediction can't avalanche into an unrecoverable run of dropped frames.
const STALL_RESET_FAILURES: u32 = 3;
/// How far (px) the drift-correction search looks around the accumulated estimate when re-anchoring
/// a frame to the stitched image. Wide enough to absorb realistic accumulated error, narrow enough
/// not to snap onto a distant self-similar region.
const CORRECTION_SEARCH_RADIUS: i64 = 48;
/// Minimum mean per-row correlation (permille) for a drift correction to be trusted. Above the
/// accept floor: a correction should only fire on a clearly-better anchor, never a marginal one.
const CORRECTION_MIN_CORR_PERMILLE: u32 = 950;
/// Search radius (px) for stall recovery — must cover how far a fast scroll can jump between frames.
/// Wider than the drift-correction radius since recovery runs only after a stall, not every frame.
const STALL_RECOVERY_RADIUS: i64 = 1200;
/// Minimum mean per-row correlation (permille) over the non-trivial overlap to accept a delta. A
/// coincidental self-similar offset can pass the binary match permilles yet have a clearly lower
/// mean correlation than the true alignment; this floor rejects those low-confidence matches.
const MIN_AVG_CORR_PERMILLE: u32 = 850;
/// Fraction of the frame's non-trivial rows that must match before accepting a last/next delta.
const MIN_RELATIVE_NONTRIVIAL_FRACTION: f64 = 0.25;

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

/// Shared reference to a captured frame. `Arc` lets the same pixels flow capture → compute → merge
/// without cloning the (large) image at each hand-off.
#[cfg(target_os = "macos")]
type SharedFrame = Arc<ImageBuffer<Rgba<u8>, Vec<u8>>>;

/// Stage 1 item: a captured frame plus the previous frame, awaiting relative-delta computation. The
/// previous frame travels with it so a compute worker can measure the inter-frame shift without any
/// shared accumulated state — that is what makes the (expensive) delta search parallelizable.
#[cfg(target_os = "macos")]
struct RawJob {
    index: u64,
    session_id: String,
    prev_frame: Option<SharedFrame>,
    frame: SharedFrame,
    direction: i64,
}

/// Stage 2 item: the result of a compute worker — the measured relative delta (None if the frame
/// couldn't be aligned to its predecessor). Merged strictly in `index` order.
#[cfg(target_os = "macos")]
struct ComputedJob {
    session_id: String,
    frame: SharedFrame,
    /// Relative shift from the previous frame, or None if alignment failed.
    delta: Option<i64>,
    direction: i64,
}

/// Number of parallel delta-compute workers. The relative-delta search (~80ms on a tall frame) is
/// the throughput bottleneck; running it on several frames at once lets the pipeline keep up with the
/// ~45ms capture cadence, so the queue stops backing up and scroll no longer has to be throttled.
#[cfg(target_os = "macos")]
const COMPUTE_WORKERS: usize = 3;

/// Capacity of the raw (stage-1) queue. Deep enough that no captured frame is ever dropped while the
/// compute workers catch up; backpressure throttles scroll long before this fills.
#[cfg(target_os = "macos")]
const FRAME_QUEUE_CAPACITY: usize = 30;

/// Raw-queue depth at which the event tap starts dropping wheel events (closed-loop speed limit).
#[cfg(target_os = "macos")]
const FRAME_QUEUE_BACKPRESSURE_NUM: usize = FRAME_QUEUE_CAPACITY / 2;

/// Stage 1: capture thread → compute workers. FIFO; any idle worker takes the next frame.
#[cfg(target_os = "macos")]
static RAW_QUEUE: std::sync::OnceLock<(Mutex<VecDeque<RawJob>>, Condvar)> = std::sync::OnceLock::new();

/// Stage 2: compute workers → merge thread. Keyed by `index` so the single merge thread can consume
/// strictly in order regardless of which worker finished first.
#[cfg(target_os = "macos")]
static COMPUTED_QUEUE: std::sync::OnceLock<(Mutex<std::collections::BTreeMap<u64, ComputedJob>>, Condvar)> =
    std::sync::OnceLock::new();

/// Depth of the raw queue, mirrored as an atomic so the event tap reads backpressure without locking.
#[cfg(target_os = "macos")]
static FRAME_QUEUE_LEN: AtomicUsize = AtomicUsize::new(0);

/// Monotonic capture index. Stamped on every RawJob so the merge thread can reassemble strict order
/// after frames are processed out-of-order by the parallel compute workers.
#[cfg(target_os = "macos")]
static FRAME_INDEX: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Best-effort scroll-speed prior shared with compute workers (they have no per-frame accumulated
/// state). Only seeds the search hint, so an approximate value is fine.
#[cfg(target_os = "macos")]
static SHARED_LAST_SHIFT: AtomicUsize = AtomicUsize::new(0);

#[cfg(target_os = "macos")]
fn raw_queue() -> &'static (Mutex<VecDeque<RawJob>>, Condvar) {
    RAW_QUEUE.get_or_init(|| (Mutex::new(VecDeque::new()), Condvar::new()))
}

#[cfg(target_os = "macos")]
fn computed_queue() -> &'static (Mutex<std::collections::BTreeMap<u64, ComputedJob>>, Condvar) {
    COMPUTED_QUEUE.get_or_init(|| (Mutex::new(std::collections::BTreeMap::new()), Condvar::new()))
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
    let update = encode_build(collect_build_inputs(&session, PreviewUpdate::Replace))?;
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
                // Free-scroll model: the wheel event passes through to the real target window so the
                // user scrolls it directly. Speed is limited by closed-loop backpressure: if the
                // stitch worker is falling behind, the frame queue fills, and we drop wheel events
                // until it drains. This throttles by *actual* processing capacity, unlike the old
                // open-loop budget that throttled forwarded scroll yet couldn't see the page's real
                // (inertia-amplified) pixel movement.
                let delta_y = scroll_delta_y(event);
                if delta_y == 0.0 {
                    return CallbackResult::Keep;
                }

                let now = monotonic_millis();
                let queue_len = FRAME_QUEUE_LEN.load(Ordering::SeqCst);
                if queue_len >= FRAME_QUEUE_BACKPRESSURE_NUM {
                    // Stitching can't keep up; drop the event so the page slows until the queue
                    // drains. Don't update the scroll timestamp — a dropped event isn't real motion.
                    long_log(format!(
                        "scroll_backpressure: drop delta_y={delta_y:.2} queue_len={queue_len} threshold={FRAME_QUEUE_BACKPRESSURE_NUM}"
                    ));
                    return CallbackResult::Drop;
                }

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
                // Let the (unscaled) event reach the target window — the user scrolls naturally.
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

    // Start the pipeline consumers for this session: N parallel delta-compute workers and one merge
    // thread. They outlive individual capture loops and drain the queues independently.
    spawn_pipeline_workers(app.clone());

    // Capture loop (producer): capture a frame and enqueue a RawJob for the compute workers. Its
    // cadence is bound only by capture time (~45ms). Backpressure (event-tap dropping wheel events
    // when the raw queue fills) keeps the producer from outrunning the pipeline.
    thread::spawn(move || {
        long_log("sampling: capture loop start");
        // Read the (fixed) selection once so the per-frame capture never locks `sessions`.
        let selection = {
            match sessions().lock().ok().and_then(|guard| {
                guard.get(&session_id).map(|session| session.selection.clone())
            }) {
                Some(selection) => selection,
                None => {
                    long_log("sampling: session gone before capture loop start");
                    capture_pending.store(false, Ordering::SeqCst);
                    return;
                }
            }
        };
        let mut prev_frame: Option<SharedFrame> = None;
        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }

            let idle_ms = monotonic_millis() - last_scroll_millis.load(Ordering::SeqCst);
            // Stop capturing once scrolling (including trackpad inertia) has been quiet long enough.
            // The loop restarts on the next wheel event.
            if idle_ms > SCROLL_IDLE_STOP.as_millis() as i64 {
                long_log(format!("sampling: idle {idle_ms}ms, stopping capture loop"));
                break;
            }

            let direction = last_scroll_delta.load(Ordering::SeqCst);
            let tick_started = Instant::now();
            match capture_and_enqueue_frame(&session_id, &selection, direction, prev_frame.clone()) {
                Ok(frame) => prev_frame = Some(frame),
                Err(error) => long_log(format!("sampling: capture failed {error}")),
            }

            // Pace to the target capture interval, accounting for capture time already spent.
            let spent = tick_started.elapsed();
            if let Some(remaining) = SAMPLE_INTERVAL.checked_sub(spent) {
                thread::sleep(remaining);
            }
        }
        capture_pending.store(false, Ordering::SeqCst);
        // Wake the workers so they can re-check liveness even if the queues are empty.
        raw_queue().1.notify_all();
        computed_queue().1.notify_all();
        long_log("sampling: capture loop stopped");
    });
}

/// Spawn the pipeline (compute workers + merge thread) if not already running.
///
/// Not tied to one session's `stop` flag (that would race across sessions). Threads exit after their
/// queue stays idle for a grace period; the next captured frame re-spawns them via `PIPELINE_RUNNING`.
/// Per-frame work no-ops for frames whose session has ended.
#[cfg(target_os = "macos")]
fn spawn_pipeline_workers(app: AppHandle) {
    static PIPELINE_RUNNING: AtomicBool = AtomicBool::new(false);
    if PIPELINE_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    // Shared liveness: set false only when the merge thread decides the pipeline is idle. Compute
    // workers observe it to exit together.
    let running = Arc::new(AtomicBool::new(true));

    // Compute workers (parallel): raw frame pair -> relative delta -> computed queue (keyed by index).
    for worker_id in 0..COMPUTE_WORKERS {
        let running = running.clone();
        thread::spawn(move || {
            long_log(format!("compute worker {worker_id}: start"));
            let (raw_lock, raw_cvar) = raw_queue();
            let (comp_lock, comp_cvar) = computed_queue();
            loop {
                let job = {
                    let mut queue = match raw_lock.lock() {
                        Ok(guard) => guard,
                        Err(_) => break,
                    };
                    while queue.is_empty() && running.load(Ordering::SeqCst) {
                        let (guard, _) = match raw_cvar.wait_timeout(queue, Duration::from_millis(200))
                        {
                            Ok(result) => result,
                            Err(_) => return,
                        };
                        queue = guard;
                    }
                    let item = queue.pop_front();
                    FRAME_QUEUE_LEN.store(queue.len(), Ordering::SeqCst);
                    item
                };
                let Some(job) = job else {
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }
                    continue;
                };

                let last_shift = SHARED_LAST_SHIFT.load(Ordering::SeqCst) as u32;
                let delta = match &job.prev_frame {
                    Some(prev) => compute_relative_delta(prev, &job.frame, job.direction, last_shift),
                    // First frame of a run has no predecessor; treat as a fresh base (delta 0).
                    None => Some(0),
                };
                {
                    if let Ok(mut map) = comp_lock.lock() {
                        map.insert(
                            job.index,
                            ComputedJob {
                                session_id: job.session_id,
                                frame: job.frame,
                                delta,
                                direction: job.direction,
                            },
                        );
                    }
                }
                comp_cvar.notify_all();
            }
            long_log(format!("compute worker {worker_id}: stopped"));
        });
    }

    // Merge thread (single): consume computed jobs strictly in index order and stitch them.
    thread::spawn(move || {
        long_log("merge thread: start");
        let (comp_lock, comp_cvar) = computed_queue();
        // `next_index` is unset until the first job arrives; then it tracks strict ordering.
        let mut next_index: Option<u64> = None;
        let mut idle_waits = 0u32;
        const MAX_IDLE_WAITS: u32 = 5;
        loop {
            let job = {
                let mut map = match comp_lock.lock() {
                    Ok(guard) => guard,
                    Err(_) => break,
                };
                loop {
                    // Initialize / resync to the smallest available index so we never wait forever
                    // for an index that will never come (e.g. a fresh burst starting at a higher one).
                    if next_index.map_or(true, |n| !map.contains_key(&n)) {
                        if let Some((&smallest, _)) = map.iter().next() {
                            if next_index.map_or(true, |n| smallest >= n) {
                                next_index = Some(smallest);
                            }
                        }
                    }
                    if next_index.is_some_and(|n| map.contains_key(&n)) || idle_waits >= MAX_IDLE_WAITS
                    {
                        break;
                    }
                    let (guard, timeout) =
                        match comp_cvar.wait_timeout(map, Duration::from_millis(200)) {
                            Ok(result) => result,
                            Err(_) => return,
                        };
                    map = guard;
                    if timeout.timed_out() {
                        idle_waits += 1;
                    }
                }
                next_index.and_then(|n| map.remove(&n))
            };

            match job {
                Some(job) => {
                    idle_waits = 0;
                    next_index = Some(next_index.map_or(1, |n| n + 1));
                    if let Err(error) = merge_pipeline_job(&app, job) {
                        long_log(format!("merge thread: frame failed {error}"));
                    }
                }
                None => break,
            }
        }
        running.store(false, Ordering::SeqCst);
        // Wake compute workers so they observe `running=false` and exit too.
        raw_queue().1.notify_all();
        PIPELINE_RUNNING.store(false, Ordering::SeqCst);
        long_log("merge thread: stopped");
    });
}

/// Producer: capture one live frame and enqueue a `RawJob` (with its predecessor) for the compute
/// workers. Returns the captured frame (shared) so the caller can pass it as the next frame's
/// predecessor. Does NOT lock `sessions` — the selection is fixed and read once by the caller.
#[cfg(target_os = "macos")]
fn capture_and_enqueue_frame(
    session_id: &str,
    selection: &SelectionRect,
    direction: i64,
    prev_frame: Option<SharedFrame>,
) -> Result<SharedFrame, FlickError> {
    let capture_started = Instant::now();
    let frame = capture_live_frame_with_editor_hidden(selection)?;
    let capture_ms = capture_started.elapsed().as_millis();

    if selection.height > 0 {
        let scale = (frame.height() as f64) / (selection.height as f64);
        CAPTURE_SCALE_X1000.store((scale * 1000.0).round() as i64, Ordering::SeqCst);
    }

    let frame: SharedFrame = Arc::new(frame);
    let index = FRAME_INDEX.fetch_add(1, Ordering::SeqCst);
    let (lock, cvar) = raw_queue();
    {
        let mut queue = lock
            .lock()
            .map_err(|_| FlickError::Message("raw queue mutex poisoned".into()))?;
        // With early backpressure (50%) and a deep buffer, the queue should never reach capacity. If
        // it ever does, dropping a frame loses content (a seam), so log it loudly.
        if queue.len() >= FRAME_QUEUE_CAPACITY {
            long_log(format!(
                "sampling: QUEUE FULL ({}), dropping oldest unstitched frame — content seam likely",
                queue.len()
            ));
            queue.pop_front();
        }
        queue.push_back(RawJob {
            index,
            session_id: session_id.to_string(),
            prev_frame,
            frame: frame.clone(),
            direction,
        });
        FRAME_QUEUE_LEN.store(queue.len(), Ordering::SeqCst);
    }
    cvar.notify_one();
    long_log(format!(
        "sampling: enqueued index={index} capture_ms={capture_ms} queue_len={}",
        FRAME_QUEUE_LEN.load(Ordering::SeqCst)
    ));
    Ok(frame)
}

/// Merge-thread consumer: apply one computed job (relative delta already measured) to the stitch and
/// emit a preview. The session lock is held only for the cheap stitch + input gathering; the
/// expensive PNG/base64 encoding runs without it.
#[cfg(target_os = "macos")]
fn merge_pipeline_job(app: &AppHandle, job: ComputedJob) -> Result<(), FlickError> {
    let total_started = Instant::now();
    let ComputedJob {
        session_id,
        frame,
        delta,
        direction,
    } = job;
    // Move the pixels out of the Arc (clone only if another ref still holds it — normally not).
    let frame = Arc::try_unwrap(frame).unwrap_or_else(|arc| (*arc).clone());

    let inputs = {
        let mut guard = sessions()
            .lock()
            .map_err(|_| FlickError::Message("long capture mutex poisoned".into()))?;
        let session = match guard.get_mut(&session_id) {
            Some(session) => session,
            None => return Ok(()),
        };
        let preview_update = merge_computed_frame(session, frame, delta, direction);
        if matches!(preview_update, PreviewUpdate::None) {
            long_log("merge thread: no change");
            return Ok(());
        }
        collect_build_inputs(session, preview_update)
    };
    let update = encode_build(inputs)?;
    emit_long_capture_update(app, &session_id, update)?;
    long_log(format!(
        "merge thread: stitched total_ms={}",
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

/// Stage-1 (parallel) work: measure the relative scroll between `prev_frame` and `frame`.
///
/// This is the expensive, self-contained part of stitching — it needs only the two frames, the wheel
/// direction, and a speed hint, with NO accumulated/stitched state — which is exactly why it can run
/// on several frames concurrently. Returns the inter-frame delta (rows), or None if they don't align.
#[cfg(target_os = "macos")]
fn compute_relative_delta(
    prev_frame: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    frame: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    direction_hint: i64,
    last_shift: u32,
) -> Option<i64> {
    if frame.width() != prev_frame.width() {
        return None;
    }
    let prev_sig = RowSignatures::from_frame(prev_frame);
    let frame_sig = RowSignatures::from_frame(frame);
    let dir = if direction_hint < 0 {
        1
    } else if direction_hint > 0 {
        -1
    } else {
        0
    };
    let overlap_result = locate_next_frame_from_last(&prev_sig, &frame_sig, dir, last_shift);
    overlap_result.overlap.map(|overlap| overlap.delta_y)
}

/// Stage-2 (serial, merge thread) work: apply a precomputed relative `delta` to the running stitch.
///
/// All accumulated-state logic lives here, in index order on a single thread: advance `current_y`,
/// drift-correct against the stitched image, recover from stalls, paste the frame, and produce the
/// preview update. `delta == None` means the compute worker couldn't align this frame to its
/// predecessor (fast scroll); we then try a wide stall-recovery anchor before giving up.
#[cfg(target_os = "macos")]
fn merge_computed_frame(
    session: &mut LongCaptureSession,
    frame: ImageBuffer<Rgba<u8>, Vec<u8>>,
    delta: Option<i64>,
    direction_hint: i64,
) -> PreviewUpdate {
    if frame.width() != session.stitched.width() {
        long_log("append_free: width changed, replacing stitched image");
        return reset_stitched_to_frame(session, frame);
    }

    let Some(delta_y) = delta else {
        // Couldn't align to the previous frame. Try a wide re-acquisition against the stitched image
        // (same stall-recovery path as before) before dropping the frame.
        session.failed_locate_count = session.failed_locate_count.saturating_add(1);
        long_log(format!(
            "append_free: no last/next overlap, frame dropped failures={} current_y={} stitched=[{}, {})",
            session.failed_locate_count,
            session.current_y,
            session.stitched_range.top,
            session.stitched_range.bottom,
        ));
        if session.failed_locate_count >= STALL_RESET_FAILURES {
            session.last_shift = 0;
            let frame_sig = RowSignatures::from_frame(&frame);
            if let Some(recovered_y) = correct_y_against_stitched(
                &session.stitched,
                session.stitched_range,
                &frame_sig,
                session.current_y,
                STALL_RECOVERY_RADIUS,
            ) {
                long_log(format!(
                    "append_free: stall recovery current_y={} -> {recovered_y} after {} failures",
                    session.current_y, session.failed_locate_count
                ));
                session.failed_locate_count = 0;
                session.current_y = recovered_y;
                return merge_frame_by_range(session, frame, recovered_y);
            }
        }
        return PreviewUpdate::None;
    };
    session.failed_locate_count = 0;

    let estimated_y = session.current_y + delta_y;
    // Drift correction (serial, against the in-order stitched image): re-anchor the frame near the
    // accumulated estimate to remove the drift that pure accumulation otherwise lets build up.
    let frame_sig = RowSignatures::from_frame(&frame);
    let new_y = correct_y_against_stitched(
        &session.stitched,
        session.stitched_range,
        &frame_sig,
        estimated_y,
        CORRECTION_SEARCH_RADIUS,
    )
    .unwrap_or(estimated_y);
    if new_y != estimated_y {
        long_log(format!(
            "append_free: drift correction estimated_y={estimated_y} -> new_y={new_y} (shift {})",
            new_y - estimated_y
        ));
    }
    // Track scroll speed for the next frame's search hint, using the corrected position.
    let moved = (new_y - session.current_y).unsigned_abs();
    if moved > 0 {
        session.last_shift = moved.min(u64::from(frame.height())) as u32;
        SHARED_LAST_SHIFT.store(session.last_shift as usize, Ordering::SeqCst);
    }
    let _ = direction_hint;
    long_log(format!(
        "append_free: located via last/next top_y={new_y} current_y={} delta_y={} stitched=[{}, {}) last_shift={}",
        session.current_y,
        delta_y,
        session.stitched_range.top,
        session.stitched_range.bottom,
        session.last_shift,
    ));

    merge_frame_by_range(session, frame, new_y)
}

/// Re-anchor `frame` against the already-stitched image to correct accumulated drift or recover from
/// a stall.
///
/// Searches stitched placements within `radius` of the accumulated `estimated_y` and returns the one
/// whose mean per-row correlation is highest, but only if that match is both confident
/// (>= `CORRECTION_MIN_CORR_PERMILLE`) and the frame is fully inside the stitched range (a partial
/// frame at the growing edge has no full reference to anchor against — those keep the accumulated
/// estimate). Returns `None` when no confident in-range anchor exists. A small radius does cheap
/// drift correction every frame; a large radius re-acquires position after a fast scroll outran the
/// frame-to-frame search.
fn correct_y_against_stitched(
    stitched: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    stitched_range: CaptureRange,
    frame_sig: &RowSignatures,
    estimated_y: i64,
    radius: i64,
) -> Option<i64> {
    let frame_h = frame_sig.len() as i64;
    let stitched_h = stitched.height() as i64;
    if frame_h == 0 || frame_h > stitched_h {
        return None;
    }

    // Only correct when the frame lies fully within already-stitched content; at the growing edge the
    // reference is incomplete and the accumulated estimate must stand.
    let est_offset = estimated_y - stitched_range.top;
    let max_offset = stitched_h - frame_h;
    if est_offset < 0 || est_offset > max_offset {
        return None;
    }

    let lo = (est_offset - radius).clamp(0, max_offset);
    let hi = (est_offset + radius).clamp(0, max_offset);
    // Window of stitched rows any candidate placement touches.
    let stitched_sig =
        RowSignatures::from_image_window(stitched, lo as u32, (hi + frame_h).min(stitched_h) as u32);
    let win_rows = stitched_sig.len() as i64;
    if win_rows < frame_h {
        return None;
    }

    let mut best_offset: Option<i64> = None;
    let mut best_corr = 0.0_f32;
    let mut best_distance = i64::MAX;
    for local in 0..=(win_rows - frame_h) {
        let mut sum_corr = 0.0_f32;
        let mut rows = 0_u32;
        for r in 0..frame_h {
            let f = r as usize;
            let s = (local + r) as usize;
            if !frame_sig.trivial[f] || !stitched_sig.trivial[s] {
                sum_corr += frame_sig.row_corr(f, &stitched_sig, s);
                rows += 1;
            }
        }
        if rows == 0 {
            continue;
        }
        let corr = sum_corr / rows as f32;
        let offset = lo + local;
        let distance = (offset - est_offset).abs();
        // Prefer higher correlation; on near-ties prefer the placement closest to the estimate so a
        // far self-similar location can't pull the anchor away.
        if corr > best_corr + 0.001 || ((corr - best_corr).abs() <= 0.001 && distance < best_distance)
        {
            best_corr = corr;
            best_offset = Some(offset);
            best_distance = distance;
        }
    }

    let offset = best_offset?;
    if (best_corr * 1000.0).round() as u32 >= CORRECTION_MIN_CORR_PERMILLE {
        Some(stitched_range.top + offset)
    } else {
        None
    }
}

/// Per-row content signature of an image. Each row carries:
/// - an exact `hash` (FNV-1a over quantized RGB) used as a fast equality bonus / tie-breaker,
/// - a segment-mean luminance `feature` vector matched via ZNCC, which tolerates the sub-pixel
///   jitter that smooth scrolling on Retina produces (and which defeats the exact hash),
/// - precomputed `mean`/`inv_norm` of each feature vector so correlation is a single dot product,
/// - a `trivial` flag for blank rows (too little structure to anchor an alignment on).
struct RowSignatures {
    hashes: Vec<u64>,
    /// Zero-mean feature vector per row: ROW_FEATURE_SEGMENTS segment means followed by the vertical
    /// gradient (this row's means minus the previous row's), the whole thing centered to zero mean.
    features: Vec<[f32; ROW_FEATURE_LEN]>,
    /// `1.0 / sqrt(sum(feature[i]^2))` per row, or 0.0 for a flat (trivial) row.
    inv_norm: Vec<f32>,
    trivial: Vec<bool>,
}

impl RowSignatures {
    /// Segment-mean luminance + exact hash for a single image row.
    fn row_seg_means_and_hash(
        raw: &[u8],
        width: u32,
        row: u32,
        col_step: u32,
    ) -> ([f32; ROW_FEATURE_SEGMENTS], u64) {
        let row_stride = (width * 4) as usize;
        let base = row as usize * row_stride;
        // FNV-1a over quantized RGB of the sampled columns gives a content-sensitive hash.
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;

        // Segment-mean luminance: divide the row into ROW_FEATURE_SEGMENTS equal-width bands and
        // average each band. Averaging absorbs sub-pixel resampling jitter that a single-pixel
        // sample (or an exact hash) would not.
        let mut seg_sum = [0.0_f32; ROW_FEATURE_SEGMENTS];
        let mut seg_count = [0_u32; ROW_FEATURE_SEGMENTS];
        for px in 0..width {
            let idx = base + (px as usize) * 4;
            let luma = (u32::from(raw[idx]) * 2
                + u32::from(raw[idx + 1]) * 5
                + u32::from(raw[idx + 2])) as f32
                / 8.0;
            let seg = if width <= 1 {
                0
            } else {
                ((px as usize) * ROW_FEATURE_SEGMENTS / (width as usize)).min(ROW_FEATURE_SEGMENTS - 1)
            };
            seg_sum[seg] += luma;
            seg_count[seg] += 1;
        }
        let mut means = [0.0_f32; ROW_FEATURE_SEGMENTS];
        for (slot, (sum, count)) in means.iter_mut().zip(seg_sum.iter().zip(seg_count.iter())) {
            *slot = if *count > 0 { *sum / *count as f32 } else { 0.0 };
        }

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
            col += col_step;
        }
        (means, hash)
    }

    /// Build the centered feature (means + vertical gradient) and its norm for one row.
    fn build_feature(
        means: &[f32; ROW_FEATURE_SEGMENTS],
        prev: &[f32; ROW_FEATURE_SEGMENTS],
    ) -> ([f32; ROW_FEATURE_LEN], f32, bool) {
        let mut feature = [0.0_f32; ROW_FEATURE_LEN];
        let mut mean = 0.0_f32;
        for seg in 0..ROW_FEATURE_SEGMENTS {
            let value = means[seg];
            let gradient = means[seg] - prev[seg];
            feature[seg] = value;
            feature[ROW_FEATURE_SEGMENTS + seg] = gradient;
            mean += value + gradient;
        }
        mean /= ROW_FEATURE_LEN as f32;

        // Center the feature and precompute its L2 norm so ZNCC reduces to a dot product later.
        let mut sum_sq = 0.0_f32;
        for slot in feature.iter_mut() {
            *slot -= mean;
            sum_sq += *slot * *slot;
        }
        let stddev = (sum_sq / ROW_FEATURE_LEN as f32).sqrt();
        let is_trivial = stddev < ROW_FEATURE_MIN_STDDEV;
        let inv = if is_trivial || sum_sq <= 0.0 {
            0.0
        } else {
            1.0 / sum_sq.sqrt()
        };
        (feature, inv, is_trivial)
    }

    fn from_frame(frame: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> Self {
        Self::from_image_window(frame, 0, frame.height())
    }

    /// Compute signatures for rows `[row_start, row_end)` of `frame` only. The vertical gradient of
    /// `row_start` needs the row above it, so one extra leading row is sampled (when available) and
    /// then dropped — this keeps a windowed signature identical to the same rows from `from_frame`.
    fn from_image_window(
        frame: &ImageBuffer<Rgba<u8>, Vec<u8>>,
        row_start: u32,
        row_end: u32,
    ) -> Self {
        let width = frame.width();
        let height = frame.height();
        let row_start = row_start.min(height);
        let row_end = row_end.min(height);
        let col_step = (width / SHIFT_SAMPLE_COLS).max(1);
        let raw = frame.as_raw();

        // Sample from one row before the window (if any) so the gradient at row_start is correct.
        let sample_start = row_start.saturating_sub(1);
        let count = (row_end.saturating_sub(row_start)) as usize;
        let mut hashes = Vec::with_capacity(count);
        let mut features = Vec::with_capacity(count);
        let mut inv_norm = Vec::with_capacity(count);
        let mut trivial = Vec::with_capacity(count);

        let mut prev_means: Option<[f32; ROW_FEATURE_SEGMENTS]> = None;
        for row in sample_start..row_end {
            let (means, hash) = Self::row_seg_means_and_hash(raw, width, row, col_step);
            // The first sampled row mirrors from_frame's behavior of using itself as its own prev.
            let prev = prev_means.as_ref().unwrap_or(&means);
            let (feature, inv, is_trivial) = Self::build_feature(&means, prev);
            if row >= row_start {
                hashes.push(hash);
                features.push(feature);
                inv_norm.push(inv);
                trivial.push(is_trivial);
            }
            prev_means = Some(means);
        }
        Self {
            hashes,
            features,
            inv_norm,
            trivial,
        }
    }

    fn len(&self) -> u32 {
        self.hashes.len() as u32
    }

    fn nontrivial_rows(&self) -> u32 {
        self.trivial.iter().filter(|trivial| !**trivial).count() as u32
    }

    /// Zero-mean normalized cross-correlation of two rows' feature vectors. The features are already
    /// centered, so this is `dot(a, b) * inv_norm(a) * inv_norm(b)`. Flat rows (inv_norm == 0)
    /// return 0.0 and never match by correlation.
    fn row_corr(&self, row: usize, other: &Self, other_row: usize) -> f32 {
        let na = self.inv_norm[row];
        let nb = other.inv_norm[other_row];
        if na == 0.0 || nb == 0.0 {
            return 0.0;
        }
        let dot: f32 = self.features[row]
            .iter()
            .zip(other.features[other_row].iter())
            .map(|(a, b)| a * b)
            .sum();
        dot * na * nb
    }

    fn row_feature_matches(&self, row: usize, other: &Self, other_row: usize) -> bool {
        self.hashes[row] == other.hashes[other_row]
            || self.row_corr(row, other, other_row) >= ROW_CORR_THRESHOLD
    }
}

// `delta_y` is the only field consumed now (the pipeline split dropped the verbose per-frame log);
// the rest remain for diagnostics.
#[allow(dead_code)]
struct RelativeFrameOverlap {
    /// `next_top_y - last_top_y` in physical pixels.
    delta_y: i64,
    /// Number of rows in the theoretical overlap window.
    overlap_rows: u32,
    /// Total rows whose features match (ZNCC) at the selected relative position.
    matched: u32,
    /// Matched rows where at least one side is non-trivial. This keeps blank areas from being the only
    /// evidence, while still letting blank rows participate in the match ratio.
    nontrivial_matched: u32,
    /// Rows in the overlap where at least one side is non-trivial.
    nontrivial_overlap_rows: u32,
    /// `matched / overlap_rows * 1000`.
    match_permille: u32,
    /// `nontrivial_matched / nontrivial_overlap_rows * 1000`.
    nontrivial_match_permille: u32,
    /// Longest continuous run of feature-matched rows.
    contiguous: u32,
    /// Longest continuous run of exact-hash-matched rows; tie-break bonus only.
    hash_contiguous: u32,
    /// Mean per-row correlation over the non-trivial overlap, in permille (0..1000). Primary key.
    avg_corr_permille: u32,
}

// Several fields are diagnostic-only (populated for potential logging); keep them for debuggability.
#[allow(dead_code)]
struct RelativeOverlapResult {
    overlap: Option<RelativeFrameOverlap>,
    best_delta_y: i64,
    best_overlap_rows: u32,
    best_matched: u32,
    best_nontrivial_matched: u32,
    best_nontrivial_overlap_rows: u32,
    best_match_permille: u32,
    best_nontrivial_match_permille: u32,
    best_contiguous: u32,
    similar_delta_y: i64,
    similar_overlap_rows: u32,
    similar_matched: u32,
    similar_nontrivial_matched: u32,
    similar_match_permille: u32,
    similar_nontrivial_match_permille: u32,
    predicted_delta_y: i64,
    min_contiguous: u32,
    min_nontrivial_matched: u32,
}

fn locate_next_frame_from_last(
    last_sig: &RowSignatures,
    next_sig: &RowSignatures,
    dir: i64,
    last_shift: u32,
) -> RelativeOverlapResult {
    let last_h = last_sig.len() as i64;
    let next_h = next_sig.len() as i64;
    if last_h == 0 || next_h == 0 {
        return RelativeOverlapResult {
            overlap: None,
            best_delta_y: 0,
            best_overlap_rows: 0,
            best_matched: 0,
            best_nontrivial_matched: 0,
            best_nontrivial_overlap_rows: 0,
            best_match_permille: 0,
            best_nontrivial_match_permille: 0,
            best_contiguous: 0,
            similar_delta_y: 0,
            similar_overlap_rows: 0,
            similar_matched: 0,
            similar_nontrivial_matched: 0,
            similar_match_permille: 0,
            similar_nontrivial_match_permille: 0,
            predicted_delta_y: 0,
            min_contiguous: 0,
            min_nontrivial_matched: 0,
        };
    }

    // Minimum overlap required to *accept* a delta. Kept modest (an absolute floor, not a fraction of
    // the frame) so a fast scroll that leaves only a small overlap can still be aligned — the old
    // 0.5-frame floor capped delta at half the frame height, which made fast scrolls snap to the
    // boundary delta where self-similar content happens to match. Confidence now comes from continuous
    // correlation quality, not from forcing a large overlap.
    let min_contiguous = (MIN_OVERLAP_ROWS as u32)
        .min(last_sig.len())
        .min(next_sig.len())
        .max(MIN_QUALITY_ROWS);
    let min_nontrivial_matched = ((last_sig.nontrivial_rows().min(next_sig.nontrivial_rows())
        as f64)
        * MIN_RELATIVE_NONTRIVIAL_FRACTION)
        .ceil() as u32;
    let min_nontrivial_matched = min_nontrivial_matched.max(MIN_QUALITY_ROWS);
    // Search the widest range the accept-floor allows, so fast scrolls (large delta, small overlap)
    // are reachable.
    let max_delta = last_h - i64::from(min_contiguous);
    if max_delta < 1 {
        return RelativeOverlapResult {
            overlap: None,
            best_delta_y: 0,
            best_overlap_rows: 0,
            best_matched: 0,
            best_nontrivial_matched: 0,
            best_nontrivial_overlap_rows: 0,
            best_match_permille: 0,
            best_nontrivial_match_permille: 0,
            best_contiguous: 0,
            similar_delta_y: 0,
            similar_overlap_rows: 0,
            similar_matched: 0,
            similar_nontrivial_matched: 0,
            similar_match_permille: 0,
            similar_nontrivial_match_permille: 0,
            predicted_delta_y: 0,
            min_contiguous,
            min_nontrivial_matched,
        };
    }

    let preferred_sign = if dir > 0 {
        1
    } else if dir < 0 {
        -1
    } else {
        0
    };
    let predicted = preferred_sign * i64::from(last_shift.max(1)).min(max_delta);
    let mut best: Option<RelativeFrameOverlap> = None;
    let mut best_distance = i64::MAX;
    let mut diagnostic_best_delta_y = 0_i64;
    let mut diagnostic_best_overlap_rows = 0_u32;
    let mut diagnostic_best_matched = 0_u32;
    let mut diagnostic_best_nontrivial_matched = 0_u32;
    let mut diagnostic_best_nontrivial_overlap_rows = 0_u32;
    let mut diagnostic_best_match_permille = 0_u32;
    let mut diagnostic_best_nontrivial_match_permille = 0_u32;
    let mut diagnostic_best_contiguous = 0_u32;
    let mut diagnostic_similar_delta_y = 0_i64;
    let mut diagnostic_similar_overlap_rows = 0_u32;
    let mut diagnostic_similar_matched = 0_u32;
    let mut diagnostic_similar_nontrivial_matched = 0_u32;
    let mut diagnostic_similar_match_permille = 0_u32;
    let mut diagnostic_similar_nontrivial_match_permille = 0_u32;

    for delta_y in -max_delta..=max_delta {
        if delta_y == 0 {
            continue;
        }
        // Reject tiny deltas whenever a scroll direction is known. The sampling loop only runs while
        // the wheel is active, so the page really moved by more than a pixel; a delta of ±1..a few px
        // is the self-similar near-neighbor artifact (the whole page offset by one line still matches
        // at ~1000‰) and, with a poisoned prediction, it self-locks the search there — the page keeps
        // scrolling but `current_y` only crawls ±1, corrupting the stitch. Dropping these frames is
        // safe: the overlap is nearly a full frame, so the next (larger) frame still overlaps.
        if preferred_sign != 0 && delta_y.abs() < MIN_SCROLL_DELTA {
            continue;
        }

        let last_start = delta_y.max(0);
        let next_start = (-delta_y).max(0);
        let overlap = (last_h - last_start).min(next_h - next_start);
        if overlap <= 0 {
            continue;
        }

        let mut matched = 0_u32;
        let mut nontrivial_matched = 0_u32;
        let mut nontrivial_overlap_rows = 0_u32;
        let mut similar_matched = 0_u32;
        let mut similar_nontrivial_matched = 0_u32;
        let mut contiguous = 0_u32;
        let mut current_run = 0_u32;
        // Longest run of feature-matched rows. This is the primary contiguity evidence now that
        // exact-hash runs are unreliable under smooth-scroll jitter.
        let mut similar_contiguous = 0_u32;
        let mut similar_current_run = 0_u32;
        // Continuous correlation summed over non-trivial rows. Unlike the binary "matches/doesn't"
        // permille (which saturates at 1000‰ across a whole range of deltas on self-similar pages),
        // the real alignment has a distinctly higher mean correlation than a coincidental one, so this
        // is the key that breaks the small-vs-large-delta tie.
        let mut sum_corr = 0.0_f32;
        for row in 0..overlap {
            let last_row = (last_start + row) as usize;
            let next_row = (next_start + row) as usize;
            let has_nontrivial_evidence =
                !last_sig.trivial[last_row] || !next_sig.trivial[next_row];
            if has_nontrivial_evidence {
                nontrivial_overlap_rows += 1;
                sum_corr += last_sig.row_corr(last_row, next_sig, next_row);
            }
            if last_sig.row_feature_matches(last_row, next_sig, next_row) {
                similar_matched += 1;
                if has_nontrivial_evidence {
                    similar_nontrivial_matched += 1;
                }
                similar_current_run += 1;
                similar_contiguous = similar_contiguous.max(similar_current_run);
            } else {
                similar_current_run = 0;
            }
            if last_sig.hashes[last_row] == next_sig.hashes[next_row] {
                matched += 1;
                if has_nontrivial_evidence {
                    nontrivial_matched += 1;
                }
                current_run += 1;
                contiguous = contiguous.max(current_run);
            } else {
                current_run = 0;
            }
        }
        let overlap_rows = overlap as u32;
        let match_permille = matched.saturating_mul(1000) / overlap_rows.max(1);
        let nontrivial_match_permille =
            nontrivial_matched.saturating_mul(1000) / nontrivial_overlap_rows.max(1);
        let similar_match_permille = similar_matched.saturating_mul(1000) / overlap_rows.max(1);
        let similar_nontrivial_match_permille =
            similar_nontrivial_matched.saturating_mul(1000) / nontrivial_overlap_rows.max(1);
        // Mean per-row correlation over the non-trivial overlap, in permille (0..1000). This is the
        // primary alignment-quality signal; the binary permilles above are kept for the accept gate.
        let avg_corr_permille = if nontrivial_overlap_rows > 0 {
            ((sum_corr / nontrivial_overlap_rows as f32).clamp(0.0, 1.0) * 1000.0).round() as u32
        } else {
            0
        };

        if (
            nontrivial_match_permille,
            nontrivial_matched,
            match_permille,
            matched,
            contiguous,
        ) > (
            diagnostic_best_nontrivial_match_permille,
            diagnostic_best_nontrivial_matched,
            diagnostic_best_match_permille,
            diagnostic_best_matched,
            diagnostic_best_contiguous,
        ) {
            diagnostic_best_delta_y = delta_y;
            diagnostic_best_overlap_rows = overlap_rows;
            diagnostic_best_matched = matched;
            diagnostic_best_nontrivial_matched = nontrivial_matched;
            diagnostic_best_nontrivial_overlap_rows = nontrivial_overlap_rows;
            diagnostic_best_match_permille = match_permille;
            diagnostic_best_nontrivial_match_permille = nontrivial_match_permille;
            diagnostic_best_contiguous = contiguous;
        }

        if (
            similar_nontrivial_match_permille,
            similar_nontrivial_matched,
            similar_match_permille,
            similar_matched,
        ) > (
            diagnostic_similar_nontrivial_match_permille,
            diagnostic_similar_nontrivial_matched,
            diagnostic_similar_match_permille,
            diagnostic_similar_matched,
        ) {
            diagnostic_similar_delta_y = delta_y;
            diagnostic_similar_overlap_rows = overlap_rows;
            diagnostic_similar_matched = similar_matched;
            diagnostic_similar_nontrivial_matched = similar_nontrivial_matched;
            diagnostic_similar_match_permille = similar_match_permille;
            diagnostic_similar_nontrivial_match_permille = similar_nontrivial_match_permille;
        }

        // Accept gate uses the feature (ZNCC) metrics: exact hashes are too brittle under
        // sub-pixel / Retina scroll jitter and would reject every real frame. Exact-hash counts are
        // kept only as a tie-break bonus below.
        //
        // Accept is driven by mean correlation (`avg_corr_permille`), which stays high on a genuine
        // alignment even when the overlap is thin (fast scroll) and the *binary* per-row match ratio
        // is noisy from sub-pixel jitter. The old gate required a 650‰ binary match over a large
        // overlap, so fast scrolls (small overlap, binary ratio ~350-600‰ but avg_corr ~950‰+) were
        // rejected and the capture stalled. We keep only: enough overlapping non-trivial rows to be
        // statistically meaningful, and a high mean correlation.
        if nontrivial_overlap_rows < MIN_QUALITY_ROWS
            || similar_nontrivial_matched < MIN_QUALITY_ROWS
            || (overlap as i64) < i64::from(min_contiguous)
            || avg_corr_permille < MIN_AVG_CORR_PERMILLE
        {
            continue;
        }

        // Direction gate: the wheel direction is ground truth. Once a scroll direction is known
        // (preferred_sign != 0), a delta of the opposite sign is a wrong-direction false match —
        // repetitive page structure can make the page look like it jumped backwards. Reject it so a
        // small overlap doesn't get stitched in reverse.
        if preferred_sign != 0 && delta_y.signum() != preferred_sign {
            continue;
        }

        let distance = (delta_y - predicted).abs();
        // Rank primarily by mean correlation: it is continuous, so the true alignment scores
        // measurably higher than a coincidental self-similar offset, even when both saturate the
        // binary match permilles at ~1000‰. Closeness to the predicted scroll (`-distance`) is the
        // tie-break that keeps the estimate stable across frames; the binary permilles and contiguity
        // only break remaining ties. Notably there is NO `|delta|` preference — that previously pushed
        // the choice to the boundary delta where self-similar content matched.
        let better = match best.as_ref() {
            None => true,
            Some(current) => {
                (
                    avg_corr_permille,
                    -distance,
                    similar_nontrivial_match_permille,
                    similar_match_permille,
                    similar_contiguous,
                    contiguous,
                ) > (
                    current.avg_corr_permille,
                    -best_distance,
                    current.nontrivial_match_permille,
                    current.match_permille,
                    current.contiguous,
                    current.hash_contiguous,
                )
            }
        };
        if better {
            best = Some(RelativeFrameOverlap {
                delta_y,
                overlap_rows,
                matched: similar_matched,
                nontrivial_matched: similar_nontrivial_matched,
                nontrivial_overlap_rows,
                match_permille: similar_match_permille,
                nontrivial_match_permille: similar_nontrivial_match_permille,
                contiguous: similar_contiguous,
                hash_contiguous: contiguous,
                avg_corr_permille,
            });
            best_distance = distance;
        }
    }

    RelativeOverlapResult {
        overlap: best,
        best_delta_y: diagnostic_best_delta_y,
        best_overlap_rows: diagnostic_best_overlap_rows,
        best_matched: diagnostic_best_matched,
        best_nontrivial_matched: diagnostic_best_nontrivial_matched,
        best_nontrivial_overlap_rows: diagnostic_best_nontrivial_overlap_rows,
        best_match_permille: diagnostic_best_match_permille,
        best_nontrivial_match_permille: diagnostic_best_nontrivial_match_permille,
        best_contiguous: diagnostic_best_contiguous,
        similar_delta_y: diagnostic_similar_delta_y,
        similar_overlap_rows: diagnostic_similar_overlap_rows,
        similar_matched: diagnostic_similar_matched,
        similar_nontrivial_matched: diagnostic_similar_nontrivial_matched,
        similar_match_permille: diagnostic_similar_match_permille,
        similar_nontrivial_match_permille: diagnostic_similar_nontrivial_match_permille,
        predicted_delta_y: predicted,
        min_contiguous,
        min_nontrivial_matched,
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
        // move the viewport there. Also write the full accepted frame into the stitch; the next
        // iteration strictly re-anchors `last_frame` against the stitched image, so the stitch must
        // contain the exact pixels of the last accepted frame, not only the rows that grew it.
        let moved = frame_y != session.current_y;
        image::imageops::overlay(&mut session.stitched, &frame, 0, frame_y - old_range.top);
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

    image::imageops::overlay(&mut grown, &frame, 0, frame_range.top - new_range.top);

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
/// Owned image data + metadata gathered under the session lock, ready to be encoded without it.
/// Encoding (PNG + base64) is the expensive part and must not hold the lock or it serializes with
/// the capture thread.
#[cfg(target_os = "macos")]
struct BuildInputs {
    current_frame: ImageBuffer<Rgba<u8>, Vec<u8>>,
    /// Full-stitched preview image, only for Replace.
    replace_preview: Option<ImageBuffer<Rgba<u8>, Vec<u8>>>,
    append: Option<(u32, ImageBuffer<Rgba<u8>, Vec<u8>>)>,
    prepend: Option<(u32, ImageBuffer<Rgba<u8>, Vec<u8>>)>,
    width: u32,
    frame_height: u32,
    total_height: u32,
    scroll_offset: i32,
    max_offset: i32,
}

/// Cheap phase, run under the session lock: clone the images the encoder needs and copy scalars.
#[cfg(target_os = "macos")]
fn collect_build_inputs(
    session: &LongCaptureSession,
    preview_update: PreviewUpdate,
) -> BuildInputs {
    let mut replace_preview = None;
    let mut append = None;
    let mut prepend = None;
    match preview_update {
        PreviewUpdate::Replace => replace_preview = Some(preview_image(&session.stitched)),
        PreviewUpdate::Append { rows, image } => append = Some((rows, preview_image(&image))),
        PreviewUpdate::Prepend { rows, image } => prepend = Some((rows, preview_image(&image))),
        PreviewUpdate::OffsetOnly | PreviewUpdate::None => {}
    }
    BuildInputs {
        current_frame: session.last_frame.clone(),
        replace_preview,
        append,
        prepend,
        width: session.stitched.width(),
        frame_height: session.last_frame.height(),
        total_height: session.stitched.height(),
        scroll_offset: (session.current_y - session.stitched_range.top) as i32,
        max_offset: (session.stitched_range.bottom
            - session.stitched_range.top
            - i64::from(session.last_frame.height()))
        .max(0) as i32,
    }
}

/// Expensive phase, run WITHOUT the lock: PNG-encode + base64 the gathered images.
#[cfg(target_os = "macos")]
fn encode_build(inputs: BuildInputs) -> Result<LongCaptureUpdate, FlickError> {
    let total_started = Instant::now();
    let current_frame_data_url = image_to_data_url(&inputs.current_frame)?;
    let mut preview_data_url = String::new();
    let mut preview_append_data_url = String::new();
    let mut preview_append_rows = 0;
    let mut preview_prepend_data_url = String::new();
    let mut preview_prepend_rows = 0;
    if let Some(preview) = inputs.replace_preview {
        preview_data_url = image_to_data_url(&preview)?;
    }
    if let Some((rows, image)) = inputs.append {
        preview_append_rows = rows;
        preview_append_data_url = image_to_data_url(&image)?;
    }
    if let Some((rows, image)) = inputs.prepend {
        preview_prepend_rows = rows;
        preview_prepend_data_url = image_to_data_url(&image)?;
    }
    long_log(format!(
        "encode_build: total_ms={} current_len={} preview_len={} append_len={} append_rows={} prepend_len={} prepend_rows={}",
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
        width: inputs.width,
        frame_height: inputs.frame_height,
        total_height: inputs.total_height,
        scroll_offset: inputs.scroll_offset,
        min_offset: 0,
        max_offset: inputs.max_offset,
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
