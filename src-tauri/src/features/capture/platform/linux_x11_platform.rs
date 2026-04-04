//! Linux-specific capture-session behavior.

#[path = "linux_overlay_platform.rs"]
mod overlay;

use std::{
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use tauri::{AppHandle, Manager, State};
use x11rb::{
    COPY_DEPTH_FROM_PARENT, NONE,
    connection::Connection,
    protocol::{
        Event,
        shape::{ConnectionExt as ShapeExt, SK, SO},
        xproto::{
            AtomEnum, ButtonIndex, ClipOrdering, ConfigureWindowAux, ConnectionExt, CreateGCAux,
            CreateWindowAux, EventMask, GrabMode, GrabStatus, KeyButMask, Rectangle, StackMode,
            Window, WindowClass,
        },
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as WrapperConnectionExt,
};

use crate::{
    app::{AppState, windows::emit_capture_status},
    error::FlickError,
    models::SelectionRect,
    services::CachedScreenCapture,
};
use overlay::{OverlaySetup, OverlayVisuals, border_rects, collect_overlay_setup};

const POLL_INTERVAL: Duration = Duration::from_millis(16);
const DRAG_THRESHOLD: f64 = 4.0;
const BORDER_THICKNESS: u32 = 2;
const DIM_ALPHA: f64 = 0.18;
const ESCAPE_KEYCODE: u8 = 9;

fn overlay_visuals() -> OverlayVisuals {
    OverlayVisuals {
        dim_alpha: DIM_ALPHA,
        border_thickness: BORDER_THICKNESS,
    }
}

#[derive(Debug, Clone, Default)]
struct CursorPosition {
    x: i32,
    y: i32,
}

#[derive(Debug, Default)]
struct NativeCaptureRuntime {
    next_session_id: u64,
    active_session_id: Option<u64>,
}

fn native_runtime() -> &'static Mutex<NativeCaptureRuntime> {
    static RUNTIME: OnceLock<Mutex<NativeCaptureRuntime>> = OnceLock::new();
    RUNTIME.get_or_init(|| Mutex::new(NativeCaptureRuntime::default()))
}

struct NativeOverlay {
    windows: Vec<Window>,
    border_windows: [Window; 4],
    dim_gc: u32,
    border_gc: u32,
    opacity_atom: u32,
    desktop_origin: (i32, i32),
}

pub fn begin_interactive_capture_session(
    app: &AppHandle,
    _state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    let overlay = collect_overlay_setup(app)?;
    let session_id = {
        let mut runtime = native_runtime()
            .lock()
            .map_err(|_| FlickError::Message("native capture runtime mutex poisoned".into()))?;
        runtime.next_session_id += 1;
        runtime.active_session_id = Some(runtime.next_session_id);
        runtime.next_session_id
    };

    let app_handle = app.clone();
    thread::spawn(move || run_native_capture_session(app_handle, overlay, session_id));
    Ok(())
}

pub fn cancel_interactive_capture_session(_app: &AppHandle, _state: &State<'_, AppState>) {
    clear_active_session();
}

pub fn prepare_for_capture_session(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    ensure_x11_session()
}

pub fn complete_ui_before_capture_processing(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
) -> Result<Vec<CachedScreenCapture>, FlickError> {
    clear_active_session();
    Ok(Vec::new())
}

pub fn finalize_capture_session(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
    _restore_previous_frontmost: bool,
) {
    clear_active_session();
}

pub fn restore_after_failed_capture(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
    _restore_previous_frontmost: bool,
) {
    clear_active_session();
}

pub fn cleanup_after_cancel(_app: &AppHandle, _state: &State<'_, AppState>) {
    clear_active_session();
}

fn run_native_capture_session(app: AppHandle, overlay: OverlaySetup, session_id: u64) {
    let run = || -> Result<(), FlickError> {
        let (conn, screen_num) =
            x11rb::connect(None).map_err(|error| FlickError::Message(error.to_string()))?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;

        let native_overlay = create_overlay(&conn, root, &overlay, overlay_visuals())?;
        let _cleanup = OverlayCleanup::new(&conn, root, native_overlay);

        let pointer_status = conn
            .grab_pointer(
                false,
                root,
                EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                NONE,
                NONE,
                x11rb::CURRENT_TIME,
            )
            .map_err(|error| FlickError::Message(error.to_string()))?
            .reply()
            .map_err(|error| FlickError::Message(error.to_string()))?;
        if pointer_status.status != GrabStatus::SUCCESS {
            return Err(FlickError::Message(
                "failed to grab pointer for Linux capture".into(),
            ));
        }

        let keyboard_status = conn
            .grab_keyboard(
                false,
                root,
                x11rb::CURRENT_TIME,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )
            .map_err(|error| FlickError::Message(error.to_string()))?
            .reply()
            .map_err(|error| FlickError::Message(error.to_string()))?;
        if keyboard_status.status != GrabStatus::SUCCESS {
            return Err(FlickError::Message(
                "failed to grab keyboard for Linux capture".into(),
            ));
        }

        conn.flush()
            .map_err(|error| FlickError::Message(error.to_string()))?;

        let stop = AtomicBool::new(false);
        let mut drag_anchor: Option<CursorPosition> = None;
        let mut dragging = false;

        while !stop.load(Ordering::Relaxed) {
            if !is_active_session(session_id) {
                break;
            }

            while let Some(event) = conn
                .poll_for_event()
                .map_err(|error| FlickError::Message(error.to_string()))?
            {
                match event {
                    Event::MotionNotify(event) => {
                        let cursor = CursorPosition {
                            x: event.root_x.into(),
                            y: event.root_y.into(),
                        };
                        if event.state.contains(KeyButMask::BUTTON1) {
                            if let Some(anchor) = drag_anchor.as_ref() {
                                let next_selection = {
                                    let selection =
                                        normalize_selection(selection_from_points(anchor, &cursor));
                                    if is_selection_large_enough(&selection, DRAG_THRESHOLD) {
                                        dragging = true;
                                        Some(selection)
                                    } else {
                                        None
                                    }
                                };
                                update_overlay_selection(
                                    &conn,
                                    &_cleanup.overlay,
                                    &overlay,
                                    next_selection.as_ref(),
                                )?;
                            }
                        }
                    }
                    Event::ButtonPress(event) => match event.detail {
                        detail if detail == u8::from(ButtonIndex::M1) => {
                            drag_anchor = Some(CursorPosition {
                                x: event.root_x.into(),
                                y: event.root_y.into(),
                            });
                            dragging = false;
                            update_overlay_selection(&conn, &_cleanup.overlay, &overlay, None)?;
                        }
                        detail if detail == u8::from(ButtonIndex::M3) => {
                            let _ = crate::features::capture::cancel_capture(&app);
                            stop.store(true, Ordering::Relaxed);
                        }
                        _ => {}
                    },
                    Event::ButtonRelease(event) => {
                        if event.detail == u8::from(ButtonIndex::M1) {
                            clear_active_session();
                            let cursor = CursorPosition {
                                x: event.root_x.into(),
                                y: event.root_y.into(),
                            };
                            let final_selection = if dragging {
                                drag_anchor
                                    .as_ref()
                                    .map(|anchor| {
                                        normalize_selection(selection_from_points(anchor, &cursor))
                                    })
                                    .filter(|selection| {
                                        selection.width >= 2 && selection.height >= 2
                                    })
                            } else {
                                None
                            };

                            if let Some(selection) = final_selection {
                                let state = app.state::<AppState>();
                                if let Err(error) = crate::features::capture::complete_capture(
                                    &app, &state, selection,
                                ) {
                                    emit_capture_status(&app, "capture-error", error.to_string());
                                }
                            } else {
                                let _ = crate::features::capture::cancel_capture(&app);
                            }
                            stop.store(true, Ordering::Relaxed);
                        }
                    }
                    Event::KeyPress(event) => {
                        if event.detail == ESCAPE_KEYCODE {
                            let _ = crate::features::capture::cancel_capture(&app);
                            stop.store(true, Ordering::Relaxed);
                        }
                    }
                    _ => {}
                }
            }

            thread::sleep(POLL_INTERVAL);
        }

        Ok(())
    };

    if let Err(error) = run() {
        if is_active_session(session_id) {
            emit_capture_status(&app, "capture-error", error.to_string());
            let _ = crate::features::capture::cancel_capture(&app);
        }
    }
}

fn create_overlay(
    conn: &RustConnection,
    root: Window,
    overlay: &OverlaySetup,
    visuals: OverlayVisuals,
) -> Result<NativeOverlay, FlickError> {
    let screen = &conn.setup().roots[0];
    let dim_gc = conn
        .generate_id()
        .map_err(|error| FlickError::Message(error.to_string()))?;
    let border_gc = conn
        .generate_id()
        .map_err(|error| FlickError::Message(error.to_string()))?;
    let opacity_atom = conn
        .intern_atom(false, b"_NET_WM_WINDOW_OPACITY")
        .map_err(|error| FlickError::Message(error.to_string()))?
        .reply()
        .map_err(|error| FlickError::Message(error.to_string()))?
        .atom;

    conn.create_gc(
        dim_gc,
        root,
        &CreateGCAux::new()
            .foreground(screen.black_pixel)
            .graphics_exposures(0),
    )
    .map_err(|error| FlickError::Message(error.to_string()))?;
    conn.create_gc(
        border_gc,
        root,
        &CreateGCAux::new()
            .foreground(screen.white_pixel)
            .graphics_exposures(0),
    )
    .map_err(|error| FlickError::Message(error.to_string()))?;

    let opacity = ((u32::MAX as f64) * visuals.dim_alpha.clamp(0.0, 1.0)).round() as u32;
    let mut windows = Vec::with_capacity(overlay.geometry.len());

    for bounds in &overlay.geometry {
        let window = conn
            .generate_id()
            .map_err(|error| FlickError::Message(error.to_string()))?;
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            bounds.x as i16,
            bounds.y as i16,
            bounds.width as u16,
            bounds.height as u16,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new()
                .override_redirect(1)
                .background_pixel(screen.black_pixel)
                .event_mask(EventMask::EXPOSURE),
        )
        .map_err(|error| FlickError::Message(error.to_string()))?;
        conn.change_property32(
            x11rb::protocol::xproto::PropMode::REPLACE,
            window,
            opacity_atom,
            AtomEnum::CARDINAL,
            &[opacity],
        )
        .map_err(|error| FlickError::Message(error.to_string()))?;
        conn.map_window(window)
            .map_err(|error| FlickError::Message(error.to_string()))?;
        conn.configure_window(
            window,
            &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        )
        .map_err(|error| FlickError::Message(error.to_string()))?;
        conn.poly_fill_rectangle(
            window,
            dim_gc,
            &[Rectangle {
                x: 0,
                y: 0,
                width: bounds.width as u16,
                height: bounds.height as u16,
            }],
        )
        .map_err(|error| FlickError::Message(error.to_string()))?;
        windows.push(window);
    }

    let border_windows = create_border_windows(conn, root, visuals.border_thickness)?;
    conn.flush()
        .map_err(|error| FlickError::Message(error.to_string()))?;

    Ok(NativeOverlay {
        windows,
        border_windows,
        dim_gc,
        border_gc,
        opacity_atom,
        desktop_origin: (overlay.desktop_bounds.x, overlay.desktop_bounds.y),
    })
}

fn create_border_windows(
    conn: &RustConnection,
    root: Window,
    border_thickness: u32,
) -> Result<[Window; 4], FlickError> {
    let screen = &conn.setup().roots[0];
    let mut ids = Vec::with_capacity(4);

    for _ in 0..4 {
        let window = conn
            .generate_id()
            .map_err(|error| FlickError::Message(error.to_string()))?;
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            border_thickness as u16,
            border_thickness as u16,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new()
                .override_redirect(1)
                .background_pixel(screen.white_pixel),
        )
        .map_err(|error| FlickError::Message(error.to_string()))?;
        conn.unmap_window(window)
            .map_err(|error| FlickError::Message(error.to_string()))?;
        ids.push(window);
    }

    Ok([ids[0], ids[1], ids[2], ids[3]])
}

fn update_overlay_selection(
    conn: &RustConnection,
    native_overlay: &NativeOverlay,
    overlay: &OverlaySetup,
    selection: Option<&SelectionRect>,
) -> Result<(), FlickError> {
    for (window, bounds) in native_overlay.windows.iter().zip(&overlay.geometry) {
        let rectangles = selection
            .and_then(|selection| intersect_rect(selection, bounds))
            .map(|selection| build_dim_rectangles(bounds, &selection))
            .unwrap_or_else(|| full_window_rectangles(bounds));

        conn.shape_rectangles(
            SO::SET,
            SK::BOUNDING,
            ClipOrdering::UNSORTED,
            *window,
            0,
            0,
            &rectangles,
        )
        .map_err(|error| FlickError::Message(error.to_string()))?;
    }

    if let Some(selection) = selection.cloned() {
        for (window, rect) in native_overlay
            .border_windows
            .iter()
            .zip(border_rects(selection, BORDER_THICKNESS))
        {
            conn.configure_window(
                *window,
                &ConfigureWindowAux::new()
                    .x(rect.x)
                    .y(rect.y)
                    .width(rect.width)
                    .height(rect.height)
                    .stack_mode(StackMode::ABOVE),
            )
            .map_err(|error| FlickError::Message(error.to_string()))?;
            conn.map_window(*window)
                .map_err(|error| FlickError::Message(error.to_string()))?;
            conn.poly_fill_rectangle(
                *window,
                native_overlay.border_gc,
                &[Rectangle {
                    x: 0,
                    y: 0,
                    width: rect.width as u16,
                    height: rect.height as u16,
                }],
            )
            .map_err(|error| FlickError::Message(error.to_string()))?;
        }
    } else {
        for window in native_overlay.border_windows {
            conn.unmap_window(window)
                .map_err(|error| FlickError::Message(error.to_string()))?;
        }
    }

    conn.flush()
        .map_err(|error| FlickError::Message(error.to_string()))?;
    let _ = &native_overlay.opacity_atom;
    let _ = &native_overlay.desktop_origin;
    Ok(())
}

fn full_window_rectangles(bounds: &SelectionRect) -> Vec<Rectangle> {
    vec![Rectangle {
        x: 0,
        y: 0,
        width: bounds.width as u16,
        height: bounds.height as u16,
    }]
}

fn build_dim_rectangles(bounds: &SelectionRect, selection: &SelectionRect) -> Vec<Rectangle> {
    let left = selection.x - bounds.x;
    let top = selection.y - bounds.y;
    let right = left + selection.width as i32;
    let bottom = top + selection.height as i32;
    let full_width = bounds.width as i32;
    let full_height = bounds.height as i32;

    let candidates = [
        Rectangle {
            x: 0,
            y: 0,
            width: full_width.max(0) as u16,
            height: top.max(0) as u16,
        },
        Rectangle {
            x: 0,
            y: bottom.max(0) as i16,
            width: full_width.max(0) as u16,
            height: (full_height - bottom).max(0) as u16,
        },
        Rectangle {
            x: 0,
            y: top.max(0) as i16,
            width: left.max(0) as u16,
            height: selection.height as u16,
        },
        Rectangle {
            x: right.max(0) as i16,
            y: top.max(0) as i16,
            width: (full_width - right).max(0) as u16,
            height: selection.height as u16,
        },
    ];

    candidates
        .into_iter()
        .filter(|rect| rect.width > 0 && rect.height > 0)
        .collect()
}

fn selection_from_points(start: &CursorPosition, end: &CursorPosition) -> SelectionRect {
    let x = start.x.min(end.x);
    let y = start.y.min(end.y);
    let width = (start.x - end.x).unsigned_abs();
    let height = (start.y - end.y).unsigned_abs();

    SelectionRect {
        x,
        y,
        width,
        height,
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

fn intersect_rect(a: &SelectionRect, b: &SelectionRect) -> Option<SelectionRect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width as i32).min(b.x + b.width as i32);
    let bottom = (a.y + a.height as i32).min(b.y + b.height as i32);

    (right > left && bottom > top).then_some(SelectionRect {
        x: left,
        y: top,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    })
}

fn clear_active_session() {
    if let Ok(mut runtime) = native_runtime().lock() {
        runtime.active_session_id = None;
    }
}

fn is_active_session(session_id: u64) -> bool {
    native_runtime()
        .lock()
        .map(|runtime| runtime.active_session_id == Some(session_id))
        .unwrap_or(false)
}

fn ensure_x11_session() -> Result<(), FlickError> {
    let has_display = std::env::var_os("DISPLAY").is_some();
    let is_wayland_only = std::env::var_os("WAYLAND_DISPLAY").is_some() && !has_display;
    if is_wayland_only {
        return Err(FlickError::Message(
            "Linux screenshot capture currently requires an X11 session".into(),
        ));
    }
    if !has_display {
        return Err(FlickError::Message(
            "DISPLAY is not available for Linux screenshot capture".into(),
        ));
    }
    Ok(())
}

struct OverlayCleanup<'a> {
    conn: &'a RustConnection,
    root: Window,
    overlay: NativeOverlay,
}

impl<'a> OverlayCleanup<'a> {
    fn new(conn: &'a RustConnection, root: Window, overlay: NativeOverlay) -> Self {
        Self {
            conn,
            root,
            overlay,
        }
    }
}

impl Drop for OverlayCleanup<'_> {
    fn drop(&mut self) {
        let _ = self.conn.ungrab_pointer(x11rb::CURRENT_TIME);
        let _ = self.conn.ungrab_keyboard(x11rb::CURRENT_TIME);

        for window in self.overlay.windows.iter().copied() {
            let _ = self.conn.destroy_window(window);
        }
        for window in self.overlay.border_windows {
            let _ = self.conn.destroy_window(window);
        }
        let _ = self.conn.free_gc(self.overlay.dim_gc);
        let _ = self.conn.free_gc(self.overlay.border_gc);
        let _ = self
            .conn
            .configure_window(self.root, &ConfigureWindowAux::new());
        let _ = self.conn.flush();
    }
}
