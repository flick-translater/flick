//! macOS-specific capture-session behavior.

use tauri::{AppHandle, Manager, State};

use crate::{
    app::AppState, error::FlickError, models::CursorPosition, services::CachedScreenCapture,
};

pub fn current_global_cursor_position(app: &AppHandle) -> Result<CursorPosition, FlickError> {
    use objc2_app_kit::NSEvent;

    let location = NSEvent::mouseLocation();
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
        x: location.x,
        y: max_y - location.y + min_y,
    })
}

pub fn prepare_for_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    // Remember the previously focused app so plain screenshot mode can hand focus back cleanly.
    remember_previous_frontmost_app(state);

    if let Some(window) = app.get_webview_window("main") {
        let is_visible = window.is_visible().unwrap_or(false);
        let is_minimized = window.is_minimized().unwrap_or(false);
        let is_focused = window.is_focused().unwrap_or(false);

        if is_visible && !is_minimized && !is_focused {
            suppress_main_window_for_capture(app, state);
        }

        if !is_visible || is_minimized {
            let _ = window.hide();
            let _ = app.show();
        }
    }

    Ok(())
}

pub fn complete_ui_before_capture_processing(
    app: &AppHandle,
    _state: &State<'_, AppState>,
) -> Result<Vec<CachedScreenCapture>, FlickError> {
    use objc2_app_kit::NSWindow;

    super::super::session::emit_capture_event_to_windows(app, "capture-ended", "finished");
    let overlays = app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| crate::app::windows::is_capture_window_label(label))
        .map(|(_, overlay)| overlay)
        .collect::<Vec<_>>();
    // Hide overlays on the main thread before taking the actual screenshot.
    for overlay in overlays {
        let ns_window = overlay.ns_window()? as usize;
        app.run_on_main_thread(move || {
            let window: &NSWindow = unsafe { &*(ns_window as *mut std::ffi::c_void).cast() };
            window.orderOut(None);
        })?;
    }

    Ok(Vec::new())
}

pub fn finalize_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
    restore_previous_frontmost: bool,
) {
    restore_main_window_after_capture(app, state);
    if restore_previous_frontmost {
        restore_previous_frontmost_app(state);
    }
}

pub fn restore_after_failed_capture(
    app: &AppHandle,
    state: &State<'_, AppState>,
    restore_previous_frontmost: bool,
) {
    finalize_capture_session(app, state, restore_previous_frontmost);
}

pub fn cleanup_after_cancel(app: &AppHandle, state: &State<'_, AppState>) {
    restore_main_window_after_capture(app, state);
    restore_previous_frontmost_app(state);
}

fn remember_previous_frontmost_app(state: &State<'_, AppState>) {
    use objc2_app_kit::{NSRunningApplication, NSWorkspace};

    let workspace = NSWorkspace::sharedWorkspace();
    let current_pid = NSRunningApplication::currentApplication().processIdentifier();
    let previous_pid = workspace
        .frontmostApplication()
        .map(|app| app.processIdentifier())
        .filter(|pid| *pid != current_pid);

    if let Ok(mut guard) = state.capture_previous_frontmost_pid.lock() {
        *guard = previous_pid;
    }
}

fn restore_previous_frontmost_app(state: &State<'_, AppState>) {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};

    let previous_pid = match state.capture_previous_frontmost_pid.lock() {
        Ok(mut guard) => guard.take(),
        Err(_) => None,
    };

    if let Some(app) =
        previous_pid.and_then(NSRunningApplication::runningApplicationWithProcessIdentifier)
    {
        let _ = app.activateWithOptions(NSApplicationActivationOptions::empty());
    }
}

fn suppress_main_window_for_capture(app: &AppHandle, state: &State<'_, AppState>) {
    let should_suppress = match state.capture_main_window_suppressed.lock() {
        Ok(mut guard) => {
            if *guard {
                false
            } else {
                *guard = true;
                true
            }
        }
        Err(_) => false,
    };

    if should_suppress {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_focusable(false);
            let _ = window.set_always_on_bottom(true);
        }
    }
}

fn restore_main_window_after_capture(app: &AppHandle, state: &State<'_, AppState>) {
    let should_restore = match state.capture_main_window_suppressed.lock() {
        Ok(mut guard) => {
            let value = *guard;
            *guard = false;
            value
        }
        Err(_) => false,
    };

    if should_restore {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_always_on_bottom(false);
            let _ = window.set_focusable(true);
        }
    }
}
