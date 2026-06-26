//! macOS long-capture scroll input controller.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    thread,
    time::Duration,
};

use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes, kCFRunLoopDefaultMode};
use core_graphics::base::boolean_t;
use core_graphics::display::CGDisplay;
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CGMouseButton, CallbackResult, EventField, ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use tauri::AppHandle;

use crate::{
    error::FlickError,
    features::capture::{
        long_capture::{monotonic_millis, screenshot_editor_window},
        platform,
    },
    models::SelectionRect,
};

use super::{ScrollControllerOptions, ScrollTarget};

/// Factor applied to each forwarded wheel delta, slowing the page so per-frame scroll stays within
/// the stitcher's alignable range.
const SCROLL_SPEED_FACTOR: f64 = 0.35;
/// Hard cap on the pixels a single forwarded wheel event may move the page.
const MAX_SCROLL_PX_PER_EVENT: f64 = 120.0;
/// Sentinel written to a synthetic scroll event's user-data field so the event tap recognizes its
/// own posted events and ignores them.
const SYNTHETIC_SCROLL_TAG: i64 = 0x466c_6b53; // "FlkS"
const BUTTON_SCROLL_PX_PER_STEP: i32 = 36;
const BUTTON_SCROLL_INTERVAL: Duration = Duration::from_millis(70);
/// Real-wheel forwarding is rate-limited to one fixed step per this interval, producing a constant
/// scroll speed regardless of how fast the user spins the wheel. A steady per-frame delta is what
/// lets the stitcher avoid the integer-multiple mis-match on periodic text.
const FIXED_SCROLL_INTERVAL_MS: i64 = 70;

pub(super) fn start_scroll_controller(options: ScrollControllerOptions) {
    thread::spawn(move || run_scroll_controller(options));
}

pub(super) fn start_button_scroll(
    app: AppHandle,
    session_id: String,
    selection: SelectionRect,
    target: ScrollTarget,
    direction: i32,
    stop: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
) -> Result<(), FlickError> {
    let _ = target;
    if running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(());
    }
    stop.store(false, Ordering::SeqCst);
    thread::spawn(move || {
        run_button_scroll_loop(app, session_id, selection, direction, stop, running);
    });
    Ok(())
}

fn run_button_scroll_loop(
    app: AppHandle,
    session_id: String,
    selection: SelectionRect,
    direction: i32,
    stop: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
) {
    let source = match CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
        Ok(source) => source,
        Err(_) => {
            running.store(false, Ordering::SeqCst);
            return;
        }
    };
    let original_location = match CGEvent::new(source.clone()) {
        Ok(event) => event.location(),
        Err(_) => {
            running.store(false, Ordering::SeqCst);
            return;
        }
    };
    let target_location = core_graphics::geometry::CGPoint::new(
        selection.x as f64 + selection.width as f64 / 2.0,
        selection.y as f64 + selection.height as f64 / 2.0,
    );
    let signed = direction.signum() * BUTTON_SCROLL_PX_PER_STEP;
    let display = CGDisplay::main();

    set_editor_cursor_passthrough(&app, &session_id, true);
    let _ = display.hide_cursor();
    let _ = CGDisplay::warp_mouse_cursor_position(target_location);

    while !stop.load(Ordering::SeqCst) && left_mouse_button_down() {
        if let Ok(scroll) =
            CGEvent::new_scroll_event(source.clone(), ScrollEventUnit::PIXEL, 1, signed, 0, 0)
        {
            scroll
                .set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SYNTHETIC_SCROLL_TAG);
            scroll.set_location(target_location);
            scroll.post(CGEventTapLocation::HID);
        }
        thread::sleep(BUTTON_SCROLL_INTERVAL);
    }

    let _ = CGDisplay::warp_mouse_cursor_position(original_location);
    let _ = display.show_cursor();
    set_editor_cursor_passthrough(&app, &session_id, false);
    stop.store(true, Ordering::SeqCst);
    running.store(false, Ordering::SeqCst);
}

fn run_scroll_controller(options: ScrollControllerOptions) {
    let ScrollControllerOptions {
        app,
        session_id,
        selection,
        stop,
        cursor_passthrough,
        last_scroll_millis,
        last_scroll_delta,
        target,
        should_throttle_scroll,
    } = options;

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
    let cursor_passthrough_for_tap = cursor_passthrough.clone();
    let last_scroll_millis_for_tap = last_scroll_millis.clone();
    let last_scroll_delta_for_tap = last_scroll_delta.clone();
    let throttle_for_tap = should_throttle_scroll.clone();

    // Synthetic-scroll setup: drop the user's real wheel events and post discrete, pixel-unit,
    // momentum-free scroll events. Real wheel events carry phase/velocity that the OS can turn into
    // inertia, which makes the page move farther than the stitcher can safely follow.
    struct SendSynth {
        source: CGEventSource,
        pid: i32,
    }
    unsafe impl Send for SendSynth {}
    let synth = match (
        CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok(),
        target.pid,
    ) {
        (Some(source), Some(pid)) => Some(SendSynth { source, pid }),
        _ => None,
    };
    // Timestamp of the last synthetic scroll we forwarded. Combined with FIXED_SCROLL_INTERVAL_MS this
    // turns the user's variable-speed wheel into a constant-rate, fixed-step scroll: at most one fixed
    // step per interval. A constant per-frame delta removes the speed variation that lets the row
    // correlation pick an integer multiple of the true shift (the duplicate-stitch / big-jump bug).
    let last_emit_ms = Arc::new(AtomicI64::new(0));

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
                if event.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA)
                    == SYNTHETIC_SCROLL_TAG
                {
                    return CallbackResult::Keep;
                }
                let delta_y = scroll_delta_y(event);
                if delta_y == 0.0 {
                    return CallbackResult::Keep;
                }

                let Some(synth) = synth.as_ref() else {
                    scale_scroll_event(event, SCROLL_SPEED_FACTOR, MAX_SCROLL_PX_PER_EVENT);
                    last_scroll_millis_for_tap.store(monotonic_millis(), Ordering::SeqCst);
                    last_scroll_delta_for_tap
                        .store(delta_y.signum().round() as i64, Ordering::SeqCst);
                    return CallbackResult::Keep;
                };

                let now = monotonic_millis();
                last_scroll_millis_for_tap.store(now, Ordering::SeqCst);
                last_scroll_delta_for_tap.store(delta_y.signum().round() as i64, Ordering::SeqCst);

                if throttle_for_tap() {
                    return CallbackResult::Drop;
                }

                // Fixed-step, rate-limited forwarding: convert the user's variable-speed wheel into a
                // constant scroll. Forward exactly one fixed step (BUTTON_SCROLL_PX_PER_STEP) at most
                // once per FIXED_SCROLL_INTERVAL_MS; drop wheel events that arrive inside the interval.
                // This keeps the per-frame scroll delta steady so the stitcher never sees a varying
                // shift it could confuse with an integer multiple on periodic text.
                let prev_emit = last_emit_ms.load(Ordering::SeqCst);
                if prev_emit != 0 && now - prev_emit < FIXED_SCROLL_INTERVAL_MS {
                    return CallbackResult::Drop;
                }
                last_emit_ms.store(now, Ordering::SeqCst);

                let signed = BUTTON_SCROLL_PX_PER_STEP * (delta_y.signum() as i32);
                if signed != 0 {
                    if let Ok(scroll) = CGEvent::new_scroll_event(
                        synth.source.clone(),
                        ScrollEventUnit::PIXEL,
                        1,
                        signed,
                        0,
                        0,
                    ) {
                        scroll.set_integer_value_field(
                            EventField::EVENT_SOURCE_USER_DATA,
                            SYNTHETIC_SCROLL_TAG,
                        );
                        scroll.set_location(location);
                        scroll.post(CGEventTapLocation::HID);
                        let _ = synth.pid;
                    }
                }
                return CallbackResult::Drop;
            }

            CallbackResult::Keep
        },
    ) {
        Ok(tap) => tap,
        Err(()) => {
            return;
        }
    };

    let source = match tap.mach_port().create_runloop_source(0) {
        Ok(source) => source,
        Err(()) => {
            return;
        }
    };

    let run_loop = CFRunLoop::get_current();
    run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
    tap.enable();

    while !stop.load(Ordering::SeqCst) {
        CFRunLoop::run_in_mode(
            unsafe { kCFRunLoopDefaultMode },
            Duration::from_millis(50),
            true,
        );
    }

    set_editor_cursor_passthrough(&app, &session_id, false);
    platform::set_overlay_mouse_passthrough(&app, false);
    platform::set_overlay_capture_sharing(&app, true);
    if let Some((_, window)) = screenshot_editor_window(&app, &session_id) {
        platform::set_window_capture_sharing(&window, true);
    }
}

fn point_in_selection(x: f64, y: f64, selection: &SelectionRect) -> bool {
    x >= selection.x as f64
        && x <= (selection.x + selection.width as i32) as f64
        && y >= selection.y as f64
        && y <= (selection.y + selection.height as i32) as f64
}

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

fn scale_scroll_event(event: &core_graphics::event::CGEvent, factor: f64, max_px: f64) {
    let clamp = |v: f64| -> f64 {
        let scaled = v * factor;
        scaled.clamp(-max_px, max_px)
    };
    let point = event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1);
    if point != 0 {
        let out = clamp(point as f64).round() as i64;
        let out = if out == 0 { point.signum() } else { out };
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1, out);
    }
    let fixed =
        event.get_double_value_field(EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1);
    if fixed != 0.0 {
        event.set_double_value_field(
            EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1,
            clamp(fixed),
        );
    }
    let line = event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1);
    if line != 0 {
        let out = ((line as f64) * factor).round() as i64;
        let out = if out == 0 { line.signum() } else { out };
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1, out);
    }
}

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

fn set_editor_cursor_passthrough(app: &AppHandle, session_id: &str, passthrough: bool) {
    if let Some((_, window)) = screenshot_editor_window(app, session_id) {
        let _ = window.set_ignore_cursor_events(passthrough);
    }
}

fn left_mouse_button_down() -> bool {
    unsafe {
        CGEventSourceButtonState(CGEventSourceStateID::HIDSystemState, CGMouseButton::Left) != 0
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceButtonState(state_id: CGEventSourceStateID, button: CGMouseButton)
    -> boolean_t;
}
