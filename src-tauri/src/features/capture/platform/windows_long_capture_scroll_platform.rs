//! Windows long-capture scroll input controller.

use std::{
    ptr::null_mut,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicI64, Ordering},
        mpsc::{self, Sender},
    },
    thread,
    time::Duration,
};

use tauri::AppHandle;
use windows_sys::{
    Win32::{
        Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
            WindowsAndMessaging::{
                CallNextHookEx, DispatchMessageW, EnumWindows, GetWindowRect, HC_ACTION,
                IsWindowVisible, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, PM_REMOVE, PeekMessageW,
                PostMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
                WH_MOUSE_LL, WHEEL_DELTA, WM_MOUSEWHEEL, WindowFromPoint,
            },
        },
    },
    core::BOOL,
};

use crate::{
    error::FlickError,
    features::capture::{
        long_capture::{long_log, monotonic_millis, screenshot_editor_window},
        platform,
    },
    models::SelectionRect,
};

use super::{ScrollControllerOptions, ScrollTarget, windows_platform};

const WHEEL_SPEED_FACTOR: f64 = 0.35;
const MAX_WHEEL_DELTA_PER_EVENT: f64 = WHEEL_DELTA as f64;
const RATE_WINDOW_MS: i64 = 150;
const RATE_MAX_DELTA_PER_WINDOW: f64 = WHEEL_DELTA as f64 * 2.0;
const BUTTON_SCROLL_DELTA_PER_STEP: i32 = WHEEL_DELTA as i32 / 3;
const BUTTON_SCROLL_INTERVAL: Duration = Duration::from_millis(70);

struct HookState {
    selection: SelectionRect,
    stop: Arc<AtomicBool>,
    last_scroll_millis: Arc<AtomicI64>,
    last_scroll_delta: Arc<AtomicI64>,
    should_throttle_scroll: Arc<dyn Fn() -> bool + Send + Sync>,
    rate_window: Vec<(i64, f64)>,
    wheel_sender: Sender<WheelCommand>,
}

struct WheelCommand {
    x: i32,
    y: i32,
    delta: i32,
}

fn hook_state() -> &'static Mutex<Option<HookState>> {
    static STATE: OnceLock<Mutex<Option<HookState>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

pub(super) fn start_scroll_controller(options: ScrollControllerOptions) {
    long_log(format!(
        "scroll_controller/windows: start session={} selection=({},{} {}x{})",
        options.session_id,
        options.selection.x,
        options.selection.y,
        options.selection.width,
        options.selection.height
    ));
    thread::spawn(move || run_scroll_controller(options));
}

pub(super) fn start_button_scroll(
    app: tauri::AppHandle,
    session_id: String,
    selection: SelectionRect,
    _target: ScrollTarget,
    direction: i32,
    stop: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
) -> Result<(), FlickError> {
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

fn run_scroll_controller(options: ScrollControllerOptions) {
    let ScrollControllerOptions {
        app,
        session_id,
        selection,
        stop,
        cursor_passthrough: _,
        last_scroll_millis,
        last_scroll_delta,
        target: _,
        should_throttle_scroll,
    } = options;

    platform::set_overlay_capture_sharing(&app, false);
    platform::set_overlay_mouse_passthrough(&app, true);
    if let Some((_, window)) = screenshot_editor_window(&app, &session_id) {
        platform::set_window_capture_sharing(&window, false);
        windows_platform::set_window_mouse_passthrough(&window, false);
        let _ = window.set_ignore_cursor_events(false);
    }

    let skip_hwnds = collect_own_window_handles(&app, &session_id);
    let (wheel_sender, wheel_receiver) = mpsc::channel::<WheelCommand>();
    {
        let app = app.clone();
        let session_id = session_id.clone();
        let stop = stop.clone();
        let skip_hwnds = skip_hwnds.clone();
        thread::spawn(move || {
            long_log("scroll_controller/windows: wheel worker start");
            while !stop.load(Ordering::SeqCst) {
                match wheel_receiver.recv_timeout(Duration::from_millis(50)) {
                    Ok(command) => {
                        send_wheel_to_point(command.x, command.y, command.delta, &skip_hwnds);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            restore_editor_cursor(&app, &session_id);
            long_log("scroll_controller/windows: wheel worker stopped");
        });
    }

    if let Ok(mut state) = hook_state().lock() {
        *state = Some(HookState {
            selection,
            stop: stop.clone(),
            last_scroll_millis,
            last_scroll_delta,
            should_throttle_scroll,
            rate_window: Vec::new(),
            wheel_sender,
        });
    }

    let hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), null_mut(), 0) };
    if hook.is_null() {
        let error = unsafe { GetLastError() };
        long_log(format!(
            "scroll_controller/windows: failed to install WH_MOUSE_LL hook last_error={error}"
        ));
        clear_hook_state();
        return;
    }

    long_log("scroll_controller/windows: WH_MOUSE_LL hook installed");
    let mut message = MSG {
        hwnd: null_mut(),
        message: 0,
        wParam: 0,
        lParam: 0,
        time: 0,
        pt: POINT { x: 0, y: 0 },
    };

    while !stop.load(Ordering::SeqCst) {
        while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        thread::sleep(Duration::from_millis(8));
    }

    unsafe {
        UnhookWindowsHookEx(hook);
    }
    restore_editor_cursor(&app, &session_id);
    platform::set_overlay_mouse_passthrough(&app, false);
    platform::set_overlay_capture_sharing(&app, true);
    if let Some((_, window)) = screenshot_editor_window(&app, &session_id) {
        windows_platform::set_window_mouse_passthrough(&window, false);
        platform::set_window_capture_sharing(&window, true);
    }
    clear_hook_state();
    long_log("scroll_controller/windows: stopped");
}

fn run_button_scroll_loop(
    app: AppHandle,
    session_id: String,
    selection: SelectionRect,
    direction: i32,
    stop: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
) {
    let target_x = selection.x + (selection.width / 2) as i32;
    let target_y = selection.y + (selection.height / 2) as i32;
    let wheel_delta = -direction.signum() * BUTTON_SCROLL_DELTA_PER_STEP;
    let skip_hwnds = collect_own_window_handles(&app, &session_id);

    restore_editor_cursor(&app, &session_id);
    long_log(format!(
        "scroll_controller/windows: button scroll loop start direction={direction} wheel_delta={wheel_delta}"
    ));

    while !stop.load(Ordering::SeqCst) && left_mouse_button_down() {
        send_wheel_to_point(target_x, target_y, wheel_delta, &skip_hwnds);
        thread::sleep(BUTTON_SCROLL_INTERVAL);
    }

    restore_editor_cursor(&app, &session_id);
    stop.store(true, Ordering::SeqCst);
    running.store(false, Ordering::SeqCst);
    long_log("scroll_controller/windows: button scroll loop stopped");
}

unsafe extern "system" fn low_level_mouse_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code < HC_ACTION as i32 || lparam == 0 {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    if wparam as u32 != WM_MOUSEWHEEL {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }

    let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
    if (info.flags & LLMHF_INJECTED) != 0 {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }

    let should_drop = hook_state()
        .lock()
        .ok()
        .and_then(|mut guard| {
            guard
                .as_mut()
                .map(|state| handle_mouse_wheel_event(state, info))
        })
        .unwrap_or(false);

    if should_drop {
        return 1;
    }
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

fn handle_mouse_wheel_event(state: &mut HookState, info: &MSLLHOOKSTRUCT) -> bool {
    if state.stop.load(Ordering::SeqCst) {
        return false;
    }

    let inside_selection = point_in_selection(info.pt.x as f64, info.pt.y as f64, &state.selection);
    long_log(format!(
        "scroll_controller/windows: wheel hook pt=({}, {}) selection=({},{} {}x{}) inside={} injected_flags={} extra={:#x}",
        info.pt.x,
        info.pt.y,
        state.selection.x,
        state.selection.y,
        state.selection.width,
        state.selection.height,
        inside_selection,
        info.flags,
        info.dwExtraInfo
    ));
    if !inside_selection {
        return false;
    }

    let raw_delta = ((info.mouseData >> 16) as u16) as i16 as i32;
    if raw_delta == 0 {
        return false;
    }

    let now = monotonic_millis();
    state.last_scroll_millis.store(now, Ordering::SeqCst);
    state
        .last_scroll_delta
        .store(-(raw_delta.signum() as i64), Ordering::SeqCst);

    if (state.should_throttle_scroll)() {
        long_log(format!(
            "scroll_controller/windows: wheel throttled raw_delta={raw_delta}"
        ));
        return true;
    }

    state
        .rate_window
        .retain(|(timestamp, _)| now - *timestamp < RATE_WINDOW_MS);
    let used: f64 = state.rate_window.iter().map(|(_, delta)| *delta).sum();
    let remaining = (RATE_MAX_DELTA_PER_WINDOW - used).max(0.0);
    let want = ((raw_delta as f64) * WHEEL_SPEED_FACTOR)
        .clamp(-MAX_WHEEL_DELTA_PER_EVENT, MAX_WHEEL_DELTA_PER_EVENT);
    let emit_abs = want.abs().min(remaining);
    if emit_abs <= 0.0 {
        return true;
    }

    state.rate_window.push((now, emit_abs));
    let mut emit = (emit_abs * want.signum()).round() as i32;
    if emit == 0 {
        emit = raw_delta.signum();
    }
    long_log(format!(
        "scroll_controller/windows: wheel inside selection raw_delta={raw_delta} emit_delta={emit}"
    ));
    if state
        .wheel_sender
        .send(WheelCommand {
            x: info.pt.x,
            y: info.pt.y,
            delta: emit,
        })
        .is_err()
    {
        long_log("scroll_controller/windows: wheel worker channel closed");
    }
    true
}

fn send_wheel_to_point(x: i32, y: i32, delta: i32, skip_hwnds: &[usize]) {
    let started = monotonic_millis();
    let before_hwnd = unsafe { WindowFromPoint(POINT { x, y }) };
    let target_hwnd = find_scroll_target_window(x, y, skip_hwnds);
    long_log(format!(
        "scroll_controller/windows: wheel target pt=({x},{y}) delta={delta} before_hwnd={:#x} target_hwnd={:#x} skipped={}",
        before_hwnd as usize,
        target_hwnd as usize,
        skip_hwnds.len()
    ));

    if target_hwnd.is_null() {
        long_log("scroll_controller/windows: wheel target missing; dropped");
        return;
    }

    post_wheel_message(target_hwnd, x, y, delta);

    let restored_hwnd = unsafe { WindowFromPoint(POINT { x, y }) };
    let elapsed = monotonic_millis() - started;
    long_log(format!(
        "scroll_controller/windows: wheel restore target hwnd={:#x} elapsed_ms={elapsed}",
        restored_hwnd as usize
    ));
}

fn post_wheel_message(hwnd: HWND, x: i32, y: i32, delta: i32) {
    let wparam = make_wparam(0, delta as i16 as u16);
    let lparam = make_lparam(x, y);
    let ok = unsafe { PostMessageW(hwnd, WM_MOUSEWHEEL, wparam, lparam) };
    if ok == 0 {
        let error = unsafe { GetLastError() };
        long_log(format!(
            "scroll_controller/windows: PostMessage WM_MOUSEWHEEL failed hwnd={:#x} delta={delta} last_error={error}",
            hwnd as usize
        ));
    } else {
        long_log(format!(
            "scroll_controller/windows: PostMessage WM_MOUSEWHEEL ok hwnd={:#x} delta={delta}",
            hwnd as usize
        ));
    }
}

fn make_wparam(low: u16, high: u16) -> WPARAM {
    (((high as u32) << 16) | low as u32) as WPARAM
}

fn make_lparam(x: i32, y: i32) -> LPARAM {
    (((y as u16 as u32) << 16) | x as u16 as u32) as LPARAM
}

fn collect_own_window_handles(app: &AppHandle, session_id: &str) -> Vec<usize> {
    let mut handles: Vec<usize> = windows_platform::overlay_window_handles()
        .into_iter()
        .map(|hwnd| hwnd as usize)
        .collect();
    if let Some((_, window)) = screenshot_editor_window(app, session_id) {
        if let Ok(hwnd) = window.hwnd() {
            handles.push(hwnd.0 as usize);
        }
    }
    handles.sort_unstable();
    handles.dedup();
    long_log(format!(
        "scroll_controller/windows: own windows skipped for wheel target count={}",
        handles.len()
    ));
    handles
}

struct HitTestData<'a> {
    x: i32,
    y: i32,
    skip_hwnds: &'a [usize],
    hwnd: HWND,
}

fn find_scroll_target_window(x: i32, y: i32, skip_hwnds: &[usize]) -> HWND {
    let mut data = HitTestData {
        x,
        y,
        skip_hwnds,
        hwnd: null_mut(),
    };
    unsafe {
        EnumWindows(
            Some(enum_hit_test_window),
            (&mut data as *mut HitTestData<'_>) as LPARAM,
        );
    }

    if data.hwnd.is_null() {
        return null_mut();
    }
    data.hwnd
}

unsafe extern "system" fn enum_hit_test_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let data = unsafe { &mut *(lparam as *mut HitTestData<'_>) };
    if data.skip_hwnds.contains(&(hwnd as usize)) {
        return 1;
    }
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return 1;
    }

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return 1;
    }
    if data.x < rect.left || data.x >= rect.right || data.y < rect.top || data.y >= rect.bottom {
        return 1;
    }

    data.hwnd = hwnd;
    0
}

fn point_in_selection(x: f64, y: f64, selection: &SelectionRect) -> bool {
    x >= selection.x as f64
        && x <= (selection.x + selection.width as i32) as f64
        && y >= selection.y as f64
        && y <= (selection.y + selection.height as i32) as f64
}

fn restore_editor_cursor(app: &AppHandle, session_id: &str) {
    set_editor_passthrough(app, session_id, false);
}

fn set_editor_passthrough(app: &AppHandle, session_id: &str, passthrough: bool) {
    if let Some((label, window)) = screenshot_editor_window(app, session_id) {
        long_log(format!(
            "scroll_controller/windows: set editor passthrough label={label} passthrough={passthrough}"
        ));
        windows_platform::set_window_mouse_passthrough(&window, passthrough);
        let _ = window.set_ignore_cursor_events(passthrough);
    }
}

fn left_mouse_button_down() -> bool {
    unsafe { GetAsyncKeyState(VK_LBUTTON as i32) < 0 }
}

fn clear_hook_state() {
    if let Ok(mut state) = hook_state().lock() {
        *state = None;
    }
}
