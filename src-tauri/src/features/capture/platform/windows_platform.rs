//! Windows-specific capture-session behavior.

#[path = "windows_frozen_overlay_platform.rs"]
mod frozen_overlay;
#[path = "windows_overlay_platform.rs"]
mod overlay;

use std::{
    ptr::null_mut,
    sync::{Mutex, OnceLock},
    thread,
    time::Duration,
};

use tauri::{AppHandle, Manager, State};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    UI::{
        Input::KeyboardAndMouse::VK_ESCAPE,
        WindowsAndMessaging::{
            CallNextHookEx, HC_ACTION, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE,
            PeekMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL,
            WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN,
        },
    },
};

use crate::{
    app::{AppState, windows::emit_capture_status},
    error::FlickError,
    models::SelectionRect,
    services::CachedScreenCapture,
};
use overlay::{OverlayVisuals, collect_overlay_setup};

const POLL_INTERVAL: Duration = Duration::from_millis(8);
const DRAG_THRESHOLD: f64 = 4.0;
const BORDER_THICKNESS: u32 = 2;
const DIM_ALPHA: f32 = 0.22;
const BORDER_COLOR: [u8; 4] = [0, 102, 204, 255];
const CROSSHAIR_COLOR: [u8; 4] = [0, 102, 204, 255];
const CROSSHAIR_DASH_LENGTH: u32 = 8;
const CROSSHAIR_GAP_LENGTH: u32 = 6;

fn overlay_visuals() -> OverlayVisuals {
    OverlayVisuals {
        dim_alpha: DIM_ALPHA,
        border_thickness: BORDER_THICKNESS,
        border_color: BORDER_COLOR,
        crosshair_color: CROSSHAIR_COLOR,
        crosshair_dash_length: CROSSHAIR_DASH_LENGTH,
        crosshair_gap_length: CROSSHAIR_GAP_LENGTH,
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
    input_state: InputState,
    hook_handles: Option<InputHookHandles>,
}

#[derive(Debug, Default, Clone, Copy)]
struct InputState {
    left_down: bool,
    right_down: bool,
    escape_pressed: bool,
}

#[derive(Debug, Clone, Copy)]
struct InputHookHandles {
    mouse_hook: isize,
    keyboard_hook: isize,
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
            .map_err(|_| FlickError::Message("windows capture runtime mutex poisoned".into()))?;
        if runtime.active_session_id.is_some() {
            return Err(FlickError::Message("capture session already active".into()));
        }
        runtime.next_session_id += 1;
        runtime.active_session_id = Some(runtime.next_session_id);
        runtime.next_session_id
    };

    if let Err(error) = cache_frozen_desktop_snapshots(app, state) {
        clear_active_session();
        return Err(error);
    }

    let app_handle = app.clone();
    thread::spawn(move || run_native_capture_session(app_handle, session_id));
    Ok(())
}

pub fn cancel_interactive_capture_session(_app: &AppHandle, _state: &State<'_, AppState>) {
    uninstall_input_hooks();
    clear_active_session();
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
    uninstall_input_hooks();
    clear_active_session();
    frozen_overlay::hide_native_overlay(app)?;
    let mut snapshots = state
        .capture_snapshots
        .lock()
        .map_err(|_| FlickError::Message("capture snapshot mutex poisoned".into()))?;
    Ok(std::mem::take(&mut *snapshots))
}

pub fn finalize_capture_session(
    app: &AppHandle,
    _state: &State<'_, AppState>,
    _restore_previous_frontmost: bool,
) {
    uninstall_input_hooks();
    let _ = frozen_overlay::hide_native_overlay(app);
}

pub fn restore_after_failed_capture(
    app: &AppHandle,
    state: &State<'_, AppState>,
    restore_previous_frontmost: bool,
) {
    finalize_capture_session(app, state, restore_previous_frontmost);
}

pub fn cleanup_after_cancel(app: &AppHandle, state: &State<'_, AppState>) {
    uninstall_input_hooks();
    clear_active_session();
    let _ = frozen_overlay::hide_native_overlay(app);
    if let Ok(mut snapshots) = state.capture_snapshots.lock() {
        snapshots.clear();
    }
}

fn run_native_capture_session(app: AppHandle, session_id: u64) {
    if !is_active_session(session_id) {
        return;
    }

    let snapshots = match app
        .state::<AppState>()
        .capture_snapshots
        .lock()
        .map(|guard| guard.clone())
    {
        Ok(snapshots) if !snapshots.is_empty() => snapshots,
        Ok(_) => {
            emit_capture_status(&app, "capture-error", "missing frozen desktop snapshots");
            let _ = crate::features::capture::cancel_capture(&app);
            return;
        }
        Err(_) => {
            emit_capture_status(&app, "capture-error", "capture snapshot mutex poisoned");
            let _ = crate::features::capture::cancel_capture(&app);
            return;
        }
    };

    if let Err(error) = frozen_overlay::show_native_overlay(&snapshots, overlay_visuals()) {
        if is_active_session(session_id) {
            emit_capture_status(&app, "capture-error", error.to_string());
            let _ = crate::features::capture::cancel_capture(&app);
        }
        return;
    }

    if let Err(error) = install_input_hooks(session_id) {
        emit_capture_status(&app, "capture-error", error.to_string());
        let _ = crate::features::capture::cancel_capture(&app);
        return;
    }

    run_native_capture_loop(app, session_id);
}

fn run_native_capture_loop(app: AppHandle, session_id: u64) {
    let mut drag_anchor: Option<CursorPosition> = None;
    let mut dragging = false;
    let mut active_selection: Option<SelectionRect> = None;
    let mut left_was_down = false;
    let mut right_was_down = false;

    loop {
        frozen_overlay::pump_native_overlay_messages();

        if !is_active_session(session_id) {
            break;
        }

        if take_escape_pressed() {
            let _ = crate::features::capture::cancel_capture(&app);
            break;
        }

        let cursor = match current_global_cursor_position() {
            Ok(cursor) => cursor,
            Err(_) => {
                thread::sleep(POLL_INTERVAL);
                continue;
            }
        };

        let input_state = current_input_state();
        let left_down = input_state.left_down;
        let right_down = input_state.right_down;

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
                } else if active_selection.is_some() {
                    active_selection = None;
                }
                let _ = frozen_overlay::update_highlight(&app, active_selection.clone());
            }
        } else if active_selection.take().is_some() {
            let _ = frozen_overlay::update_highlight(&app, None);
        }

        if left_down {
            let _ = frozen_overlay::update_crosshair(&app, None);
        } else {
            let _ = frozen_overlay::update_crosshair(&app, Some((cursor.x, cursor.y)));
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

fn current_global_cursor_position() -> Result<CursorPosition, FlickError> {
    let mut point = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
    let ok = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) };
    if ok == 0 {
        return Err(FlickError::Message("failed to read cursor position".into()));
    }

    Ok(CursorPosition {
        x: point.x as f64,
        y: point.y as f64,
    })
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

fn cache_frozen_desktop_snapshots(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    let overlay = collect_overlay_setup(app)?;
    let snapshots = overlay
        .geometry
        .iter()
        .map(frozen_overlay::capture_desktop_snapshot)
        .collect::<anyhow::Result<Vec<_>>>()
        .map_err(FlickError::from)?;

    let mut guard = state
        .capture_snapshots
        .lock()
        .map_err(|_| FlickError::Message("capture snapshot mutex poisoned".into()))?;
    guard.clear();
    guard.extend(snapshots);
    Ok(())
}

fn install_input_hooks(_session_id: u64) -> Result<(), FlickError> {
    ensure_thread_message_queue();
    reset_input_state();

    let mouse_hook =
        unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), null_mut(), 0) };
    if mouse_hook.is_null() {
        return Err(FlickError::Message(
            "failed to install low-level mouse hook".into(),
        ));
    }

    let keyboard_hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), null_mut(), 0) };
    if keyboard_hook.is_null() {
        unsafe {
            UnhookWindowsHookEx(mouse_hook);
        }
        return Err(FlickError::Message(
            "failed to install low-level keyboard hook".into(),
        ));
    }

    if let Ok(mut runtime) = native_runtime().lock() {
        runtime.hook_handles = Some(InputHookHandles {
            mouse_hook: mouse_hook as isize,
            keyboard_hook: keyboard_hook as isize,
        });
    }
    Ok(())
}

fn uninstall_input_hooks() {
    let handles = match native_runtime().lock() {
        Ok(mut runtime) => {
            runtime.input_state = InputState::default();
            runtime.hook_handles.take()
        }
        Err(_) => None,
    };

    if let Some(handles) = handles {
        unsafe {
            if handles.mouse_hook != 0 {
                UnhookWindowsHookEx(handles.mouse_hook as _);
            }
            if handles.keyboard_hook != 0 {
                UnhookWindowsHookEx(handles.keyboard_hook as _);
            }
        }
    }
}

fn ensure_thread_message_queue() {
    let mut message = MSG {
        hwnd: null_mut(),
        message: 0,
        wParam: 0,
        lParam: 0,
        time: 0,
        pt: windows_sys::Win32::Foundation::POINT { x: 0, y: 0 },
    };
    unsafe {
        PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
    }
}

fn reset_input_state() {
    if let Ok(mut runtime) = native_runtime().lock() {
        runtime.input_state = InputState::default();
    }
}

fn current_input_state() -> InputState {
    native_runtime()
        .lock()
        .map(|runtime| runtime.input_state)
        .unwrap_or_default()
}

fn take_escape_pressed() -> bool {
    match native_runtime().lock() {
        Ok(mut runtime) => {
            let pressed = runtime.input_state.escape_pressed;
            runtime.input_state.escape_pressed = false;
            pressed
        }
        Err(_) => false,
    }
}

unsafe extern "system" fn low_level_mouse_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code == HC_ACTION as i32 && lparam != 0 {
        let action = match wparam as u32 {
            WM_LBUTTONDOWN => Some(("left_button", true)),
            WM_LBUTTONUP => Some(("left_button", false)),
            WM_RBUTTONDOWN => Some(("right_button", true)),
            WM_RBUTTONUP => Some(("right_button", false)),
            _ => None,
        };

        if let Some((label, down)) = action {
            if let Ok(mut runtime) = native_runtime().lock() {
                match label {
                    "left_button" => runtime.input_state.left_down = down,
                    "right_button" => runtime.input_state.right_down = down,
                    _ => {}
                }
            }
        }
    }

    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code == HC_ACTION as i32 && lparam != 0 {
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if (wparam as u32 == WM_KEYDOWN || wparam as u32 == WM_SYSKEYDOWN)
            && info.vkCode == VK_ESCAPE as u32
        {
            if let Ok(mut runtime) = native_runtime().lock() {
                runtime.input_state.escape_pressed = true;
            }
        }
    }

    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

fn clear_active_session() {
    if let Ok(mut runtime) = native_runtime().lock() {
        runtime.active_session_id = None;
        runtime.input_state = InputState::default();
    }
}

fn is_active_session(session_id: u64) -> bool {
    native_runtime()
        .lock()
        .map(|runtime| runtime.active_session_id == Some(session_id))
        .unwrap_or(false)
}
