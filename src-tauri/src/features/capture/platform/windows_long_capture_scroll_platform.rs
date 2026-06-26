//! Windows long-capture scroll input controller.

use std::{
    ptr::null_mut,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicPtr, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::Duration,
};

use tauri::AppHandle;
use windows_sys::Win32::{
    Foundation::{GetLastError, LPARAM, LRESULT, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT,
            SendInput, VK_LBUTTON,
        },
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, LLMHF_INJECTED, MSG,
            MSLLHOOKSTRUCT, PostThreadMessageW, SetWindowsHookExW, TranslateMessage,
            UnhookWindowsHookEx, WH_MOUSE_LL, WHEEL_DELTA, WM_APP, WM_MOUSEWHEEL,
        },
    },
};

use crate::{
    error::FlickError,
    features::capture::{
        long_capture::{monotonic_millis, screenshot_editor_window},
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
const SYNTHETIC_SCROLL_TAG: usize = 0x464c_4b31; // "FLK1"
const STOP_HOOK_MESSAGE: u32 = WM_APP + 0x4c4b;

#[derive(Clone)]
struct ScrollWorkerState {
    app: AppHandle,
    session_id: String,
    last_scroll_millis: Arc<AtomicI64>,
    last_scroll_delta: Arc<AtomicI64>,
    should_throttle_scroll: Arc<dyn Fn() -> bool + Send + Sync>,
}

struct WheelCommand {
    delta: i32,
}

struct HookAtomicState {
    active: AtomicBool,
    selection_x: AtomicI32,
    selection_y: AtomicI32,
    selection_width: AtomicI32,
    selection_height: AtomicI32,
    sender: AtomicPtr<SyncSender<WheelCommand>>,
}

fn hook_state() -> &'static HookAtomicState {
    static STATE: OnceLock<HookAtomicState> = OnceLock::new();
    STATE.get_or_init(|| HookAtomicState {
        active: AtomicBool::new(false),
        selection_x: AtomicI32::new(0),
        selection_y: AtomicI32::new(0),
        selection_width: AtomicI32::new(0),
        selection_height: AtomicI32::new(0),
        sender: AtomicPtr::new(null_mut()),
    })
}

pub(super) fn start_scroll_controller(options: ScrollControllerOptions) {
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

    let (wheel_sender, wheel_receiver) = mpsc::sync_channel::<WheelCommand>(512);
    let worker_state = ScrollWorkerState {
        app: app.clone(),
        session_id: session_id.clone(),
        last_scroll_millis,
        last_scroll_delta,
        should_throttle_scroll,
    };
    {
        let app = app.clone();
        let session_id = session_id.clone();
        let stop = stop.clone();
        thread::spawn(move || {
            let mut rate_window = Vec::new();
            while !stop.load(Ordering::SeqCst) {
                match wheel_receiver.recv_timeout(Duration::from_millis(50)) {
                    Ok(command) => {
                        handle_wheel_command(&worker_state, &mut rate_window, command);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            restore_editor_cursor(&app, &session_id);
        });
    }

    install_hook_state(selection, wheel_sender);

    let hook_thread_id = unsafe { GetCurrentThreadId() };
    let hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), null_mut(), 0) };
    if hook.is_null() {
        let error = unsafe { GetLastError() };
        clear_hook_state();
        return;
    }

    {
        let stop = stop.clone();
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(20));
            }
            unsafe {
                PostThreadMessageW(hook_thread_id, STOP_HOOK_MESSAGE, 0, 0);
            }
        });
    }

    let mut message = MSG {
        hwnd: null_mut(),
        message: 0,
        wParam: 0,
        lParam: 0,
        time: 0,
        pt: Default::default(),
    };

    while !stop.load(Ordering::SeqCst) {
        let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        if result <= 0 || message.message == STOP_HOOK_MESSAGE {
            break;
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
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
}

fn run_button_scroll_loop(
    app: AppHandle,
    session_id: String,
    _selection: SelectionRect,
    direction: i32,
    stop: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
) {
    let wheel_delta = direction.signum() * BUTTON_SCROLL_DELTA_PER_STEP;

    restore_editor_cursor(&app, &session_id);

    while !stop.load(Ordering::SeqCst) && left_mouse_button_down() {
        let command = WheelCommand { delta: wheel_delta };
        dispatch_wheel_to_underlying_window(&app, &session_id, command.delta);
        thread::sleep(BUTTON_SCROLL_INTERVAL);
    }

    restore_editor_cursor(&app, &session_id);
    stop.store(true, Ordering::SeqCst);
    running.store(false, Ordering::SeqCst);
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
    if info.dwExtraInfo == SYNTHETIC_SCROLL_TAG || (info.flags & LLMHF_INJECTED) != 0 {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }

    if enqueue_wheel_from_hook(info) {
        return 1;
    }
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

fn enqueue_wheel_from_hook(info: &MSLLHOOKSTRUCT) -> bool {
    let state = hook_state();
    if !state.active.load(Ordering::Relaxed) {
        return false;
    }

    let x = state.selection_x.load(Ordering::Relaxed);
    let y = state.selection_y.load(Ordering::Relaxed);
    let width = state.selection_width.load(Ordering::Relaxed);
    let height = state.selection_height.load(Ordering::Relaxed);
    if !point_in_selection(info.pt.x, info.pt.y, x, y, width, height) {
        return false;
    }

    let raw_delta = ((info.mouseData >> 16) as u16) as i16 as i32;
    if raw_delta == 0 {
        return false;
    }

    let sender_ptr = state.sender.load(Ordering::Acquire);
    if !sender_ptr.is_null() {
        let sender = unsafe { &*sender_ptr };
        if sender.try_send(WheelCommand { delta: raw_delta }).is_err() {
            return true;
        }
    }
    true
}

fn handle_wheel_command(
    state: &ScrollWorkerState,
    rate_window: &mut Vec<(i64, f64)>,
    command: WheelCommand,
) {
    let raw_delta = command.delta;
    let now = monotonic_millis();
    state.last_scroll_millis.store(now, Ordering::SeqCst);
    state
        .last_scroll_delta
        .store(raw_delta.signum() as i64, Ordering::SeqCst);

    if (state.should_throttle_scroll)() {
        return;
    }

    rate_window.retain(|(timestamp, _)| now - *timestamp < RATE_WINDOW_MS);
    let used: f64 = rate_window.iter().map(|(_, delta)| *delta).sum();
    let remaining = (RATE_MAX_DELTA_PER_WINDOW - used).max(0.0);
    let want = ((raw_delta as f64) * WHEEL_SPEED_FACTOR)
        .clamp(-MAX_WHEEL_DELTA_PER_EVENT, MAX_WHEEL_DELTA_PER_EVENT);
    let emit_abs = want.abs().min(remaining);
    if emit_abs <= 0.0 {
        return;
    }

    rate_window.push((now, emit_abs));
    let mut emit = (emit_abs * want.signum()).round() as i32;
    if emit == 0 {
        emit = raw_delta.signum();
    }
    dispatch_wheel_to_underlying_window(&state.app, &state.session_id, emit);
}

fn dispatch_wheel_to_underlying_window(app: &AppHandle, session_id: &str, delta: i32) {
    set_editor_passthrough(app, session_id, true, false);
    inject_wheel(delta);
    set_editor_passthrough(app, session_id, false, false);
}

fn inject_wheel(delta: i32) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                mouseData: delta as u32,
                dwFlags: MOUSEEVENTF_WHEEL,
                dwExtraInfo: SYNTHETIC_SCROLL_TAG,
                ..MOUSEINPUT::default()
            },
        },
    };
    let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    if sent != 1 {
        let error = unsafe { GetLastError() };
    }
}

fn point_in_selection(
    x: i32,
    y: i32,
    selection_x: i32,
    selection_y: i32,
    width: i32,
    height: i32,
) -> bool {
    x >= selection_x
        && x <= selection_x.saturating_add(width)
        && y >= selection_y
        && y <= selection_y.saturating_add(height)
}

fn restore_editor_cursor(app: &AppHandle, session_id: &str) {
    set_editor_passthrough(app, session_id, false, true);
}

fn set_editor_passthrough(app: &AppHandle, session_id: &str, passthrough: bool, log_change: bool) {
    if let Some((_, window)) = screenshot_editor_window(app, session_id) {
        if log_change {
            windows_platform::set_window_mouse_passthrough(&window, passthrough);
        } else {
            windows_platform::set_window_mouse_passthrough_quiet(&window, passthrough);
        }
        let _ = window.set_ignore_cursor_events(passthrough);
    }
}

fn left_mouse_button_down() -> bool {
    unsafe { GetAsyncKeyState(VK_LBUTTON as i32) < 0 }
}

fn install_hook_state(selection: SelectionRect, sender: SyncSender<WheelCommand>) {
    let state = hook_state();
    state.selection_x.store(selection.x, Ordering::Relaxed);
    state.selection_y.store(selection.y, Ordering::Relaxed);
    state
        .selection_width
        .store(selection.width as i32, Ordering::Relaxed);
    state
        .selection_height
        .store(selection.height as i32, Ordering::Relaxed);
    let sender_ptr = Box::into_raw(Box::new(sender));
    let old = state.sender.swap(sender_ptr, Ordering::AcqRel);
    if !old.is_null() {
        unsafe {
            drop(Box::from_raw(old));
        }
    }
    state.active.store(true, Ordering::Release);
}

fn clear_hook_state() {
    let state = hook_state();
    state.active.store(false, Ordering::Relaxed);
    let old = state.sender.swap(null_mut(), Ordering::AcqRel);
    if !old.is_null() {
        unsafe {
            drop(Box::from_raw(old));
        }
    }
}
