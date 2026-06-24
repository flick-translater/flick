//! macOS long-capture scroll input controller.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes, kCFRunLoopDefaultMode};
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult, EventField, ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use tauri::AppHandle;

use crate::{
    features::capture::{
        long_capture::{long_log, monotonic_millis, screenshot_editor_window},
        platform,
    },
    models::SelectionRect,
};

use super::ScrollControllerOptions;

/// Factor applied to each forwarded wheel delta, slowing the page so per-frame scroll stays within
/// the stitcher's alignable range.
const SCROLL_SPEED_FACTOR: f64 = 0.35;
/// Hard cap on the pixels a single forwarded wheel event may move the page.
const MAX_SCROLL_PX_PER_EVENT: f64 = 120.0;
/// Sliding window for the scroll-rate limit.
const RATE_WINDOW_MS: i64 = 150;
/// Absolute cap on scroll pixels per window.
const RATE_MAX_PX_PER_WINDOW: f64 = 200.0;
/// Scroll budget per window as a fraction of the selection's logical height.
const RATE_MAX_FRAME_FRACTION: f64 = 0.5;
/// Sentinel written to a synthetic scroll event's user-data field so the event tap recognizes its
/// own posted events and ignores them.
const SYNTHETIC_SCROLL_TAG: i64 = 0x466c_6b53; // "FlkS"

pub(super) fn start_scroll_controller(options: ScrollControllerOptions) {
    long_log(format!(
        "scroll_controller/macos: start session={} selection=({},{} {}x{})",
        options.session_id,
        options.selection.x,
        options.selection.y,
        options.selection.width,
        options.selection.height
    ));
    thread::spawn(move || run_scroll_controller(options));
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
    let scroll_window = Arc::new(Mutex::new(Vec::<(i64, f64)>::new()));
    let rate_max_px_per_window = ((selection.height as f64) * RATE_MAX_FRAME_FRACTION)
        .min(RATE_MAX_PX_PER_WINDOW)
        .max(40.0);
    long_log(format!(
        "scroll_controller/macos: synth available={} target_pid={:?} rate_max_px={rate_max_px_per_window:.0} window_ms={RATE_WINDOW_MS}",
        synth.is_some(),
        target.pid
    ));

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

                let want = (delta_y.abs() * SCROLL_SPEED_FACTOR).min(MAX_SCROLL_PX_PER_EVENT);
                let emit = if let Ok(mut win) = scroll_window.lock() {
                    win.retain(|(ts, _)| now - *ts < RATE_WINDOW_MS);
                    let used: f64 = win.iter().map(|(_, px)| *px).sum();
                    let remaining = (rate_max_px_per_window - used).max(0.0);
                    let emit = want.min(remaining);
                    if emit > 0.0 {
                        win.push((now, emit));
                    }
                    emit
                } else {
                    0.0
                };
                if emit <= 0.0 {
                    return CallbackResult::Drop;
                }

                let signed = (emit * delta_y.signum()).round() as i32;
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
            long_log("scroll_controller/macos: failed to create event tap");
            return;
        }
    };

    let source = match tap.mach_port().create_runloop_source(0) {
        Ok(source) => source,
        Err(()) => {
            long_log("scroll_controller/macos: failed to create runloop source");
            return;
        }
    };

    let run_loop = CFRunLoop::get_current();
    run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
    tap.enable();
    long_log("scroll_controller/macos: event tap enabled");

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
    long_log("scroll_controller/macos: stopped");
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
    if let Some((label, window)) = screenshot_editor_window(app, session_id) {
        long_log(format!(
            "scroll_controller/macos: set_ignore_cursor_events label={label} passthrough={passthrough}"
        ));
        let _ = window.set_ignore_cursor_events(passthrough);
    }
}
