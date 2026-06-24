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
    Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT,
            SendInput, VK_LBUTTON,
        },
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetClassNameW, GetForegroundWindow, GetMessageW,
            GetWindowTextW, GetWindowThreadProcessId, HC_ACTION, LLMHF_INJECTED, MSG,
            MSLLHOOKSTRUCT, PostThreadMessageW, SetWindowsHookExW, TranslateMessage,
            UnhookWindowsHookEx, WH_MOUSE_LL, WHEEL_DELTA, WM_APP, WM_MOUSEWHEEL, WindowFromPoint,
        },
    },
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
const SYNTHETIC_SCROLL_TAG: usize = 0x464c_4b31; // "FLK1"
const STOP_HOOK_MESSAGE: u32 = WM_APP + 0x4c4b;

#[derive(Clone)]
struct ScrollWorkerState {
    app: AppHandle,
    session_id: String,
    last_scroll_millis: Arc<AtomicI64>,
    last_scroll_delta: Arc<AtomicI64>,
    should_throttle_scroll: Arc<dyn Fn() -> bool + Send + Sync>,
    stats: Arc<ScrollStats>,
}

struct WheelCommand {
    x: i32,
    y: i32,
    delta: i32,
    flags: u32,
    extra: usize,
}

struct ScrollStats {
    raw_count: AtomicI64,
    injected_count: AtomicI64,
    dropped_count: AtomicI64,
    last_log_millis: AtomicI64,
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

    let (wheel_sender, wheel_receiver) = mpsc::sync_channel::<WheelCommand>(512);
    let worker_state = ScrollWorkerState {
        app: app.clone(),
        session_id: session_id.clone(),
        last_scroll_millis,
        last_scroll_delta,
        should_throttle_scroll,
        stats: Arc::new(ScrollStats {
            raw_count: AtomicI64::new(0),
            injected_count: AtomicI64::new(0),
            dropped_count: AtomicI64::new(0),
            last_log_millis: AtomicI64::new(0),
        }),
    };
    {
        let app = app.clone();
        let session_id = session_id.clone();
        let stop = stop.clone();
        thread::spawn(move || {
            long_log("scroll_controller/windows: wheel worker start");
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
            long_log("scroll_controller/windows: wheel worker stopped");
        });
    }

    install_hook_state(selection, wheel_sender);

    let hook_thread_id = unsafe { GetCurrentThreadId() };
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

    restore_editor_cursor(&app, &session_id);
    long_log(format!(
        "scroll_controller/windows: button scroll loop start direction={direction} wheel_delta={wheel_delta} target=({target_x},{target_y})"
    ));

    while !stop.load(Ordering::SeqCst) && left_mouse_button_down() {
        let command = WheelCommand {
            x: target_x,
            y: target_y,
            delta: wheel_delta,
            flags: 0,
            extra: 0,
        };
        inject_wheel_through_editor(&app, &session_id, &command, wheel_delta);
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
        if sender
            .try_send(WheelCommand {
                x: info.pt.x,
                y: info.pt.y,
                delta: raw_delta,
                flags: info.flags,
                extra: info.dwExtraInfo,
            })
            .is_err()
        {
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
    state.stats.raw_count.fetch_add(1, Ordering::Relaxed);
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
        state.stats.dropped_count.fetch_add(1, Ordering::Relaxed);
        maybe_log_scroll_stats(state, now);
        return;
    }

    rate_window.push((now, emit_abs));
    let mut emit = (emit_abs * want.signum()).round() as i32;
    if emit == 0 {
        emit = raw_delta.signum();
    }
    inject_wheel_through_editor(&state.app, &state.session_id, &command, emit);
    state.stats.injected_count.fetch_add(1, Ordering::Relaxed);
    maybe_log_scroll_stats(state, now);
}

fn maybe_log_scroll_stats(state: &ScrollWorkerState, now: i64) {
    let previous = state.stats.last_log_millis.load(Ordering::Relaxed);
    if now - previous < 1000 {
        return;
    }
    if state
        .stats
        .last_log_millis
        .compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    long_log(format!(
        "scroll_controller/windows: wheel stats raw={} injected={} dropped={}",
        state.stats.raw_count.load(Ordering::Relaxed),
        state.stats.injected_count.load(Ordering::Relaxed),
        state.stats.dropped_count.load(Ordering::Relaxed)
    ));
}

fn inject_wheel_through_editor(
    app: &AppHandle,
    session_id: &str,
    command: &WheelCommand,
    delta: i32,
) {
    let before = describe_window_at(command.x, command.y);
    set_editor_passthrough_quiet(app, session_id, true);
    let passthrough = describe_window_at(command.x, command.y);
    let result = inject_wheel(delta);
    let after_inject = describe_window_at(command.x, command.y);
    set_editor_passthrough_quiet(app, session_id, false);
    let restored = describe_window_at(command.x, command.y);
    long_log(format!(
        "scroll_controller/windows: wheel dispatch pt=({}, {}) raw_delta={} emit_delta={} flags={:#x} extra={:#x} before={} passthrough={} after_inject={} restored={} sendinput={}",
        command.x,
        command.y,
        command.delta,
        delta,
        command.flags,
        command.extra,
        before,
        passthrough,
        after_inject,
        restored,
        result
    ));
}

fn inject_wheel(delta: i32) -> String {
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
        format!("failed sent={sent} error={error}")
    } else {
        format!("ok sent={sent}")
    }
}

fn describe_window_at(x: i32, y: i32) -> String {
    let hwnd = unsafe { WindowFromPoint(POINT { x, y }) };
    describe_window("point", hwnd)
}

fn describe_window(label: &str, hwnd: HWND) -> String {
    if hwnd.is_null() {
        return format!("{label}:hwnd=0x0");
    }

    let class_name = window_class_name(hwnd);
    let title = window_title(hwnd);
    let mut process_id = 0u32;
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
    let foreground = unsafe { GetForegroundWindow() };

    format!(
        "{label}:hwnd={:#x} class='{}' title='{}' tid={} pid={} foreground={}",
        hwnd as usize,
        class_name,
        title,
        thread_id,
        process_id,
        hwnd == foreground
    )
}

fn window_class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..len as usize])
}

fn window_title(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..len as usize])
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

fn set_editor_passthrough_quiet(app: &AppHandle, session_id: &str, passthrough: bool) {
    if let Some((_, window)) = screenshot_editor_window(app, session_id) {
        windows_platform::set_window_mouse_passthrough_quiet(&window, passthrough);
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
