//! macOS-specific capture-session behavior.

#[path = "macos_frozen_overlay.rs"]
mod frozen_overlay;
#[path = "macos_overlay.rs"]
mod overlay;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    sync::{Mutex, OnceLock},
    thread,
    time::Duration,
};

use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes, kCFRunLoopDefaultMode};
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult,
};
use objc2_app_kit::NSEvent;
use objc2_foundation::NSInteger;
use tauri::{AppHandle, Manager, State};

use crate::services::screen_capture::macos_frozen;
use crate::{
    app::{AppState, windows::emit_capture_status},
    error::FlickError,
    models::SelectionRect,
    services::CachedScreenCapture,
};
use overlay::{OverlayVisuals, collect_overlay_setup};

const POLL_INTERVAL: Duration = Duration::from_millis(16);
const DRAG_THRESHOLD: f64 = 4.0;
const BORDER_THICKNESS: f64 = 3.0;
const DIM_ALPHA: f64 = 0.1;
const ESCAPE_KEY_CODE: u16 = 0x35;
const EVENT_SOURCE_STATE_COMBINED_SESSION: i32 = 0;

fn overlay_visuals() -> OverlayVisuals {
    OverlayVisuals {
        dim_alpha: DIM_ALPHA,
        border_thickness: BORDER_THICKNESS as u32,
    }
}

#[derive(Debug, Clone)]
struct CursorPosition {
    x: f64,
    y: f64,
}

#[derive(Debug, Default)]
struct NativeCaptureRuntime {
    next_session_id: u64,
    active_session_id: Option<u64>,
    input_tap_stop: Option<std::sync::Arc<AtomicBool>>,
    tap_left_down: bool,
    tap_right_down: bool,
    tap_cursor_raw: Option<(f64, f64)>,
}

fn native_runtime() -> &'static Mutex<NativeCaptureRuntime> {
    static RUNTIME: OnceLock<Mutex<NativeCaptureRuntime>> = OnceLock::new();
    RUNTIME.get_or_init(|| Mutex::new(NativeCaptureRuntime::default()))
}

pub fn begin_interactive_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    let session_id = {
        let mut runtime = native_runtime()
            .lock()
            .map_err(|_| FlickError::Message("native capture runtime mutex poisoned".into()))?;
        runtime.next_session_id += 1;
        runtime.active_session_id = Some(runtime.next_session_id);
        runtime.next_session_id
    };

    if let Err(error) = cache_frozen_desktop_snapshot(app, state) {
        clear_active_session();
        return Err(error);
    }

    if let Err(error) = install_input_event_tap(app, session_id) {
        clear_active_session();
        return Err(error);
    }

    let app_handle = app.clone();
    thread::spawn(move || run_native_capture_session(app_handle, session_id));

    Ok(())
}

pub fn cancel_interactive_capture_session(_app: &AppHandle, _state: &State<'_, AppState>) {
    clear_active_session();
}

fn current_global_cursor_position(app: &AppHandle) -> Result<CursorPosition, FlickError> {
    let location = NSEvent::mouseLocation();
    current_global_cursor_position_from_nsevent_raw(app, location.x, location.y)
}

fn current_global_cursor_position_from_nsevent_raw(
    app: &AppHandle,
    raw_x: f64,
    raw_y: f64,
) -> Result<CursorPosition, FlickError> {
    let monitors = app.available_monitors()?;
    let logical_monitors = monitors
        .iter()
        .map(|monitor| {
            let scale = monitor.scale_factor();
            let x = monitor.position().x as f64 / scale;
            let y = monitor.position().y as f64 / scale;
            let width = monitor.size().width as f64 / scale;
            let height = monitor.size().height as f64 / scale;
            (x, y, width, height)
        })
        .collect::<Vec<_>>();
    let min_y = logical_monitors
        .iter()
        .map(|(_, y, _, _)| *y)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let max_y = logical_monitors
        .iter()
        .map(|(_, y, _, height)| y + height)
        .reduce(f64::max)
        .unwrap_or(0.0);

    Ok(CursorPosition {
        x: raw_x,
        y: max_y - raw_y + min_y,
    })
}

fn current_global_cursor_position_from_tap_raw(
    app: &AppHandle,
    raw_x: f64,
    raw_y: f64,
) -> Result<CursorPosition, FlickError> {
    let monitors = app.available_monitors()?;
    let logical_monitors = monitors
        .iter()
        .map(|monitor| {
            let scale = monitor.scale_factor();
            let x = monitor.position().x as f64 / scale;
            let y = monitor.position().y as f64 / scale;
            let width = monitor.size().width as f64 / scale;
            let height = monitor.size().height as f64 / scale;
            (x, y, width, height)
        })
        .collect::<Vec<_>>();
    let min_y = logical_monitors
        .iter()
        .map(|(_, y, _, _)| *y)
        .reduce(f64::min)
        .unwrap_or(0.0);

    Ok(CursorPosition {
        x: raw_x,
        y: raw_y + min_y,
    })
}

pub fn prepare_for_capture_session(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    Ok(())
}

pub fn complete_ui_before_capture_processing(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<Vec<CachedScreenCapture>, FlickError> {
    clear_active_session();
    hide_native_overlay(app)?;
    let snapshots = {
        let mut guard = state
            .capture_snapshots
            .lock()
            .map_err(|_| FlickError::Message("capture snapshot mutex poisoned".into()))?;
        std::mem::take(&mut *guard)
    };
    Ok(snapshots)
}

pub fn finalize_capture_session(
    app: &AppHandle,
    _state: &State<'_, AppState>,
    _restore_previous_frontmost: bool,
) {
    let _ = hide_native_overlay(app);
}

pub fn restore_after_failed_capture(
    app: &AppHandle,
    state: &State<'_, AppState>,
    restore_previous_frontmost: bool,
) {
    finalize_capture_session(app, state, restore_previous_frontmost);
}

pub fn cleanup_after_cancel(app: &AppHandle, state: &State<'_, AppState>) {
    clear_active_session();
    let _ = hide_native_overlay(app);
    let _ = state;
}

fn run_native_capture_loop(app: AppHandle, session_id: u64) {
    let mut drag_anchor: Option<CursorPosition> = None;
    let mut dragging = false;
    let mut active_selection: Option<SelectionRect> = None;
    let mut left_was_down = false;
    let mut right_was_down = false;

    loop {
        if !is_active_session(session_id) {
            break;
        }

        if escape_key_is_down() {
            let _ = crate::features::capture::cancel_capture(&app);
            break;
        }

        let cursor = match current_global_cursor_position(&app) {
            Ok(cursor) => cursor,
            Err(_) => {
                thread::sleep(POLL_INTERVAL);
                continue;
            }
        };

        let (left_down, right_down, tap_cursor_raw) = capture_input_state();
        let cursor = if let Some((raw_x, raw_y)) = tap_cursor_raw {
            current_global_cursor_position_from_tap_raw(&app, raw_x, raw_y).unwrap_or(cursor)
        } else {
            cursor
        };

        if right_down && !right_was_down {
            let _ = crate::features::capture::cancel_capture(&app);
            break;
        }

        if left_down && !left_was_down {
            drag_anchor = Some(cursor.clone());
            dragging = false;
            active_selection = None;
        }

        if left_down {
            if let Some(anchor) = drag_anchor.as_ref() {
                let drag_rect = selection_from_points(anchor, &cursor);
                if is_selection_large_enough(&drag_rect, DRAG_THRESHOLD) {
                    dragging = true;
                    active_selection = Some(normalize_selection(drag_rect));
                } else {
                    active_selection = None;
                }
                let _ = update_highlight(app.app_handle(), active_selection.clone());
            }
        } else {
            if active_selection.is_some() {
                active_selection = None;
                let _ = update_highlight(app.app_handle(), None);
            }
        }

        if left_down {
            let _ = hide_crosshair(app.app_handle());
        } else {
            let _ = update_crosshair(app.app_handle(), &cursor);
        }

        if !left_down && left_was_down {
            clear_active_session();
            let final_selection = if dragging {
                drag_anchor
                    .as_ref()
                    .map(|anchor| normalize_selection(selection_from_points(anchor, &cursor)))
                    .filter(|selection| selection.width >= 2 && selection.height >= 2)
            } else {
                None
            };

            if let Some(selection) = final_selection {
                let state = app.state::<AppState>();
                if let Err(error) =
                    crate::features::capture::complete_capture(&app, &state, selection)
                {
                    emit_capture_status(&app, "capture-error", error.to_string());
                }
            } else {
                let _ = crate::features::capture::cancel_capture(&app);
            }
            break;
        }

        left_was_down = left_down;
        right_was_down = right_down;
        thread::sleep(POLL_INTERVAL);
    }
}

fn run_native_capture_session(app: AppHandle, session_id: u64) {
    if !is_active_session(session_id) {
        return;
    }

    if let Err(error) = show_native_overlay(&app) {
        if is_active_session(session_id) {
            emit_capture_status(&app, "capture-error", error.to_string());
            let _ = crate::features::capture::cancel_capture(&app);
        }
        return;
    }

    run_native_capture_loop(app, session_id);
}

fn selection_from_points(start: &CursorPosition, end: &CursorPosition) -> SelectionRect {
    let x = start.x.min(end.x);
    let y = start.y.min(end.y);
    let width = (start.x - end.x).abs();
    let height = (start.y - end.y).abs();

    SelectionRect {
        x: x.floor() as i32,
        y: y.floor() as i32,
        width: width.ceil() as u32,
        height: height.ceil() as u32,
    }
}

fn normalize_selection(selection: SelectionRect) -> SelectionRect {
    SelectionRect {
        x: selection.x,
        y: selection.y,
        width: selection.width.max(1),
        height: selection.height.max(1),
    }
}

fn is_selection_large_enough(selection: &SelectionRect, threshold: f64) -> bool {
    selection.width as f64 >= threshold || selection.height as f64 >= threshold
}

fn show_native_overlay(app: &AppHandle) -> Result<(), FlickError> {
    let snapshots = app
        .state::<AppState>()
        .capture_snapshots
        .lock()
        .map_err(|_| FlickError::Message("capture snapshot mutex poisoned".into()))?
        .clone();
    if snapshots.is_empty() {
        return Err(FlickError::Message(
            "missing frozen desktop snapshots".into(),
        ));
    }
    frozen_overlay::show_native_overlay(app, &snapshots, overlay_visuals())
}

fn hide_native_overlay(app: &AppHandle) -> Result<(), FlickError> {
    frozen_overlay::hide_native_overlay(app)
}

fn update_highlight(app: &AppHandle, selection: Option<SelectionRect>) -> Result<(), FlickError> {
    frozen_overlay::update_highlight(app, selection)
}

fn update_crosshair(app: &AppHandle, cursor: &CursorPosition) -> Result<(), FlickError> {
    frozen_overlay::update_crosshair(app, cursor)
}

fn hide_crosshair(app: &AppHandle) -> Result<(), FlickError> {
    frozen_overlay::hide_crosshair(app)
}

fn cache_frozen_desktop_snapshot(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    let overlay = collect_overlay_setup(app)?;
    let handles = overlay
        .geometry
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, bounds)| {
            thread::spawn(move || {
                let result = macos_frozen::capture_desktop_snapshot(&bounds);
                (index, result)
            })
        })
        .collect::<Vec<_>>();
    let mut indexed = Vec::with_capacity(handles.len());
    for handle in handles {
        let (index, snapshot) = handle
            .join()
            .map_err(|_| FlickError::Message("capture snapshot worker panicked".into()))?;
        indexed.push((index, snapshot?));
    }
    indexed.sort_by_key(|(index, _)| *index);
    let snapshots = indexed
        .into_iter()
        .map(|(_, snapshot)| snapshot)
        .collect::<Vec<_>>();
    let mut guard = state
        .capture_snapshots
        .lock()
        .map_err(|_| FlickError::Message("capture snapshot mutex poisoned".into()))?;
    guard.clear();
    guard.extend(snapshots);
    Ok(())
}

fn clear_active_session() {
    if let Ok(mut runtime) = native_runtime().lock() {
        runtime.active_session_id = None;
        if let Some(stop) = runtime.input_tap_stop.take() {
            stop.store(true, Ordering::SeqCst);
        }
        runtime.tap_left_down = false;
        runtime.tap_right_down = false;
        runtime.tap_cursor_raw = None;
    }
}

fn escape_key_is_down() -> bool {
    unsafe { CGEventSourceKeyState(EVENT_SOURCE_STATE_COMBINED_SESSION, ESCAPE_KEY_CODE) }
}

fn is_active_session(session_id: u64) -> bool {
    native_runtime()
        .lock()
        .map(|runtime| runtime.active_session_id == Some(session_id))
        .unwrap_or(false)
}

fn capture_input_state() -> (bool, bool, Option<(f64, f64)>) {
    native_runtime()
        .lock()
        .map(|runtime| {
            (
                runtime.tap_left_down,
                runtime.tap_right_down,
                runtime.tap_cursor_raw,
            )
        })
        .unwrap_or((false, false, None))
}

fn update_tap_input_state(event_type: CGEventType, raw_x: f64, raw_y: f64) {
    if let Ok(mut runtime) = native_runtime().lock() {
        runtime.tap_cursor_raw = Some((raw_x, raw_y));
        match event_type {
            CGEventType::LeftMouseDown => runtime.tap_left_down = true,
            CGEventType::LeftMouseUp => runtime.tap_left_down = false,
            CGEventType::RightMouseDown => runtime.tap_right_down = true,
            CGEventType::RightMouseUp => runtime.tap_right_down = false,
            CGEventType::OtherMouseDown => {}
            CGEventType::OtherMouseUp => {}
            CGEventType::MouseMoved => {}
            CGEventType::ScrollWheel => {}
            CGEventType::LeftMouseDragged
            | CGEventType::RightMouseDragged
            | CGEventType::OtherMouseDragged => {}
            _ => {}
        }
    }
}

fn install_input_event_tap(app: &AppHandle, session_id: u64) -> Result<(), FlickError> {
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    {
        let mut runtime = native_runtime()
            .lock()
            .map_err(|_| FlickError::Message("native capture runtime mutex poisoned".into()))?;
        runtime.input_tap_stop = Some(stop.clone());
    }

    let app_handle = app.clone();
    thread::spawn(move || run_input_event_tap(app_handle, session_id, stop));
    Ok(())
}

fn run_input_event_tap(_app: AppHandle, session_id: u64, stop: std::sync::Arc<AtomicBool>) {
    let event_types = vec![
        CGEventType::LeftMouseDown,
        CGEventType::LeftMouseUp,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDown,
        CGEventType::RightMouseUp,
        CGEventType::RightMouseDragged,
        CGEventType::OtherMouseDown,
        CGEventType::OtherMouseUp,
        CGEventType::OtherMouseDragged,
        CGEventType::MouseMoved,
        CGEventType::ScrollWheel,
    ];

    let tap = match CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        event_types,
        move |_proxy, event_type, _event| {
            let location = _event.location();
            update_tap_input_state(event_type, location.x, location.y);
            if is_active_session(session_id) {
                CallbackResult::Drop
            } else {
                CallbackResult::Keep
            }
        },
    ) {
        Ok(tap) => tap,
        Err(()) => return,
    };

    let source = match tap.mach_port().create_runloop_source(0) {
        Ok(source) => source,
        Err(()) => return,
    };

    let run_loop = CFRunLoop::get_current();
    run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
    tap.enable();

    while is_active_session(session_id) && !stop.load(Ordering::SeqCst) {
        let _ = CFRunLoop::run_in_mode(
            unsafe { kCFRunLoopDefaultMode },
            Duration::from_millis(50),
            true,
        );
    }

    run_loop.remove_source(&source, unsafe { kCFRunLoopCommonModes });
}

unsafe extern "C" {
    fn CGShieldingWindowLevel() -> NSInteger;
    fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
}

fn shielding_window_level() -> NSInteger {
    unsafe { CGShieldingWindowLevel() }
}
