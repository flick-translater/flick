//! macOS-specific capture-session behavior.

#[path = "macos_renderer.rs"]
mod renderer;
#[path = "macos_overlay.rs"]
mod overlay;
#[path = "macos_nspanel_backend.rs"]
mod nspanel_backend;
#[path = "macos_core_graphics_backend.rs"]
mod core_graphics_backend;

use std::{
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
use objc2_app_kit::{
    NSApplicationActivationOptions, NSBackingStoreType, NSColor, NSCursor, NSEvent, NSPanel,
    NSRunningApplication, NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{NSInteger, NSPoint, NSRect, NSSize};
use tauri::{AppHandle, Manager, State};

use crate::{
    app::{AppState, windows::emit_capture_status},
    error::FlickError,
    models::SelectionRect,
    services::CachedScreenCapture,
};
use overlay::{CoordinateSpace, OverlayVisuals, collect_overlay_setup};
use renderer::RendererBackend;

const POLL_INTERVAL: Duration = Duration::from_millis(16);
const DRAG_THRESHOLD: f64 = 4.0;
const SESSION_TIMEOUT: Duration = Duration::from_secs(10);
const BORDER_THICKNESS: f64 = 3.0;
const DIM_ALPHA: f64 = 0.1;
const FILL_ALPHA: f64 = 0.6;
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
    blocker_panels: Vec<PanelHandle>,
    dim_panels: Vec<PanelHandle>,
    fill_panels: Vec<PanelHandle>,
    border_panels: Vec<PanelHandle>,
    crosshair_panels: Vec<PanelHandle>,
    coordinate_space: Option<CoordinateSpace>,
    overlay_geometry: Vec<SelectionRect>,
}

#[derive(Debug, Clone, Copy, Default)]
struct PanelHandle {
    ptr: usize,
}

fn native_runtime() -> &'static Mutex<NativeCaptureRuntime> {
    static RUNTIME: OnceLock<Mutex<NativeCaptureRuntime>> = OnceLock::new();
    RUNTIME.get_or_init(|| Mutex::new(NativeCaptureRuntime::default()))
}

pub fn begin_interactive_capture_session(
    app: &AppHandle,
    _state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    let session_id = {
        let mut runtime = native_runtime()
            .lock()
            .map_err(|_| FlickError::Message("native capture runtime mutex poisoned".into()))?;
        runtime.next_session_id += 1;
        runtime.active_session_id = Some(runtime.next_session_id);
        runtime.next_session_id
    };

    show_native_overlay(app)?;

    let app_handle = app.clone();
    thread::spawn(move || run_native_capture_loop(app_handle, session_id));

    Ok(())
}

pub fn cancel_interactive_capture_session(_app: &AppHandle, _state: &State<'_, AppState>) {
    clear_active_session();
}

fn current_global_cursor_position(app: &AppHandle) -> Result<CursorPosition, FlickError> {
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
    clear_active_session();
    hide_native_overlay(app)?;
    thread::sleep(Duration::from_millis(16));
    Ok(Vec::new())
}

pub fn finalize_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
    restore_previous_frontmost: bool,
) {
    let _ = hide_native_overlay(app);
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
    clear_active_session();
    let _ = hide_native_overlay(app);
    restore_main_window_after_capture(app, state);
    restore_previous_frontmost_app(state);
}

fn run_native_capture_loop(app: AppHandle, session_id: u64) {
    let started_at = Instant::now();
    let mut drag_anchor: Option<CursorPosition> = None;
    let mut dragging = false;
    let mut active_selection: Option<SelectionRect> = None;
    let mut left_was_down = false;
    let mut right_was_down = false;

    loop {
        if !is_active_session(session_id) {
            break;
        }

        if started_at.elapsed() >= SESSION_TIMEOUT || escape_key_is_down() {
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

        let buttons = NSEvent::pressedMouseButtons() as u64;
        let left_down = (buttons & 0b1) != 0;
        let right_down = (buttons & 0b10) != 0;

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
                if let Err(error) = crate::features::capture::complete_capture(&app, &state, selection)
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
    match renderer::active_backend() {
        RendererBackend::NsPanel => nspanel_backend::show_native_overlay(app),
        RendererBackend::CoreGraphics => core_graphics_backend::show_native_overlay(app),
    }
}

fn hide_native_overlay(app: &AppHandle) -> Result<(), FlickError> {
    match renderer::active_backend() {
        RendererBackend::NsPanel => nspanel_backend::hide_native_overlay(app),
        RendererBackend::CoreGraphics => core_graphics_backend::hide_native_overlay(app),
    }
}

fn update_highlight(app: &AppHandle, selection: Option<SelectionRect>) -> Result<(), FlickError> {
    match renderer::active_backend() {
        RendererBackend::NsPanel => nspanel_backend::update_highlight(app, selection),
        RendererBackend::CoreGraphics => core_graphics_backend::update_highlight(app, selection),
    }
}

fn update_crosshair(app: &AppHandle, cursor: &CursorPosition) -> Result<(), FlickError> {
    match renderer::active_backend() {
        RendererBackend::NsPanel => nspanel_backend::update_crosshair(app, cursor),
        RendererBackend::CoreGraphics => core_graphics_backend::update_crosshair(app, cursor),
    }
}

fn hide_crosshair(app: &AppHandle) -> Result<(), FlickError> {
    match renderer::active_backend() {
        RendererBackend::NsPanel => nspanel_backend::hide_crosshair(app),
        RendererBackend::CoreGraphics => core_graphics_backend::hide_crosshair(app),
    }
}

fn panel_show_native_overlay(app: &AppHandle) -> Result<(), FlickError> {
    let overlay = collect_overlay_setup(app)?;
    let overlay_geometry = overlay.geometry;
    let coordinate_space = overlay.coordinate_space;

    app.run_on_main_thread({
        let overlay_geometry = overlay_geometry.clone();
        move || {
            let mtm = MainThreadMarker::new().expect("main thread marker unavailable");
            let mut runtime = native_runtime()
                .lock()
                .expect("native capture runtime mutex poisoned");
            runtime.coordinate_space = Some(coordinate_space);
            runtime.overlay_geometry = overlay_geometry.clone();

            ensure_blocker_panels(&mut runtime, mtm, overlay_geometry.len());
            ensure_dim_panels(&mut runtime, mtm, overlay_geometry.len());
            ensure_fill_panels(&mut runtime, mtm);
            ensure_border_panels(&mut runtime, mtm);
            ensure_crosshair_panels(&mut runtime, mtm);

            for (panel, geometry) in runtime.blocker_panels.iter().zip(overlay_geometry.iter()) {
                set_panel_frame(*panel, geometry, coordinate_space);
                show_panel(*panel);
            }

            for (panel, geometry) in runtime.dim_panels.iter().zip(overlay_geometry.iter()) {
                set_panel_frame(*panel, geometry, coordinate_space);
                show_panel(*panel);
            }

            for panel in runtime.blocker_panels.iter().skip(overlay_geometry.len()) {
                hide_panel(*panel);
            }
            for panel in runtime.dim_panels.iter().skip(overlay_geometry.len()) {
                hide_panel(*panel);
            }
            for panel in &runtime.fill_panels {
                hide_panel(*panel);
            }

            for panel in &runtime.border_panels {
                hide_panel(*panel);
            }
            for panel in &runtime.crosshair_panels {
                hide_panel(*panel);
            }

            let cursor = NSCursor::crosshairCursor();
            cursor.push();
            cursor.set();
        }
    })?;

    Ok(())
}

fn panel_hide_native_overlay(app: &AppHandle) -> Result<(), FlickError> {
    app.run_on_main_thread(move || {
        let runtime = native_runtime()
            .lock()
            .expect("native capture runtime mutex poisoned");
        for panel in &runtime.blocker_panels {
            hide_panel(*panel);
        }
        for panel in &runtime.dim_panels {
            hide_panel(*panel);
        }
        for panel in &runtime.fill_panels {
            hide_panel(*panel);
        }
        for panel in &runtime.border_panels {
            hide_panel(*panel);
        }
        for panel in &runtime.crosshair_panels {
            hide_panel(*panel);
        }

        let cursor = NSCursor::currentCursor();
        cursor.pop();
    })?;
    Ok(())
}

fn panel_update_highlight(
    app: &AppHandle,
    selection: Option<SelectionRect>,
) -> Result<(), FlickError> {
    app.run_on_main_thread(move || {
        let runtime = native_runtime()
            .lock()
            .expect("native capture runtime mutex poisoned");
        let coordinate_space = match runtime.coordinate_space {
            Some(coordinate_space) => coordinate_space,
            None => return,
        };

        if let Some(selection) = selection {
            let border_rects = overlay::border_rects(selection.clone(), overlay_visuals().border_thickness);
            for (panel, rect) in runtime.dim_panels.iter().zip(runtime.overlay_geometry.iter()) {
                set_panel_frame(*panel, rect, coordinate_space);
                show_panel(*panel);
            }
            for panel in runtime.dim_panels.iter().skip(runtime.overlay_geometry.len()) {
                hide_panel(*panel);
            }
            if let Some(panel) = runtime.fill_panels.first() {
                set_panel_frame(*panel, &selection, coordinate_space);
                show_panel(*panel);
            }
            for (panel, rect) in runtime.border_panels.iter().zip(border_rects.iter()) {
                set_panel_frame(*panel, rect, coordinate_space);
                show_panel(*panel);
            }
        } else {
            for (panel, rect) in runtime
                .blocker_panels
                .iter()
                .zip(runtime.overlay_geometry.iter())
            {
                set_panel_frame(*panel, rect, coordinate_space);
                show_panel(*panel);
            }
            for (index, panel) in runtime.dim_panels.iter().enumerate() {
                if let Some(rect) = runtime.overlay_geometry.get(index) {
                    set_panel_frame(*panel, rect, coordinate_space);
                    show_panel(*panel);
                } else {
                    hide_panel(*panel);
                }
            }
            for panel in &runtime.fill_panels {
                hide_panel(*panel);
            }
            for panel in &runtime.border_panels {
                hide_panel(*panel);
            }
        }
    })?;
    Ok(())
}

fn ensure_blocker_panels(runtime: &mut NativeCaptureRuntime, mtm: MainThreadMarker, count: usize) {
    while runtime.blocker_panels.len() < count {
        runtime.blocker_panels.push(create_panel(
            mtm,
            panel_color(0.0, 0.0, 0.0, 0.001),
            false,
        ));
    }
}

fn ensure_dim_panels(runtime: &mut NativeCaptureRuntime, mtm: MainThreadMarker, count: usize) {
    let minimum_panels = count.max(1);
    while runtime.dim_panels.len() < minimum_panels {
        runtime.dim_panels.push(create_panel_with_level_offset(
            mtm,
            panel_color(0.0, 0.0, 0.0, DIM_ALPHA),
            false,
            0,
        ));
    }
}

fn ensure_fill_panels(runtime: &mut NativeCaptureRuntime, mtm: MainThreadMarker) {
    while runtime.fill_panels.len() < 1 {
        runtime.fill_panels.push(create_panel_with_level_offset(
            mtm,
            panel_color(0.2, 0.7, 1.0, FILL_ALPHA),
            true,
            2,
        ));
    }
}

fn ensure_border_panels(runtime: &mut NativeCaptureRuntime, mtm: MainThreadMarker) {
    while runtime.border_panels.len() < 4 {
        runtime.border_panels.push(create_panel_with_level_offset(
            mtm,
            panel_color(0.12, 0.56, 1.0, 0.95),
            true,
            3,
        ));
    }
}

fn ensure_crosshair_panels(runtime: &mut NativeCaptureRuntime, mtm: MainThreadMarker) {
    while runtime.crosshair_panels.len() < 2 {
        runtime.crosshair_panels.push(create_panel_with_level_offset(
            mtm,
            panel_color(1.0, 1.0, 1.0, 0.8),
            true,
            4,
        ));
    }
}

fn create_panel(
    mtm: MainThreadMarker,
    color: Retained<NSColor>,
    ignores_mouse_events: bool,
) -> PanelHandle {
    create_panel_with_level_offset(mtm, color, ignores_mouse_events, 1)
}

fn create_panel_with_level_offset(
    mtm: MainThreadMarker,
    color: Retained<NSColor>,
    ignores_mouse_events: bool,
    level_offset: NSInteger,
) -> PanelHandle {
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        NSBackingStoreType::Buffered,
        false,
    );

    panel.setOpaque(false);
    panel.setHasShadow(false);
    panel.setBackgroundColor(Some(&color));
    panel.setIgnoresMouseEvents(ignores_mouse_events);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::Stationary,
    );
    panel.setLevel(shielding_window_level() + level_offset);
    panel.setFloatingPanel(true);
    panel.setBecomesKeyOnlyIfNeeded(false);
    unsafe {
        panel.setReleasedWhenClosed(false);
    }
    panel.orderOut(None);

    PanelHandle {
        ptr: Retained::into_raw(panel) as usize,
    }
}

fn panel_color(red: f64, green: f64, blue: f64, alpha: f64) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, alpha)
}

fn set_panel_frame(panel: PanelHandle, selection: &SelectionRect, coordinate_space: CoordinateSpace) {
    let rect = cocoa_rect_for_selection(selection, coordinate_space);
    unsafe {
        panel_ref(panel).setFrame_display(rect, false);
    }
}

fn show_panel(panel: PanelHandle) {
    unsafe {
        panel_ref(panel).orderFrontRegardless();
    }
}

fn hide_panel(panel: PanelHandle) {
    unsafe {
        panel_ref(panel).orderOut(None);
    }
}

unsafe fn panel_ref(panel: PanelHandle) -> &'static NSPanel {
    unsafe { &*(panel.ptr as *const NSPanel) }
}

fn cocoa_rect_for_selection(selection: &SelectionRect, coordinate_space: CoordinateSpace) -> NSRect {
    let width = selection.width as f64;
    let height = selection.height as f64;
    let y = coordinate_space.max_y - selection.y as f64 - height + coordinate_space.min_y;

    NSRect::new(
        NSPoint::new(selection.x as f64, y),
        NSSize::new(width, height),
    )
}

fn panel_update_crosshair(app: &AppHandle, cursor: &CursorPosition) -> Result<(), FlickError> {
    let cursor = cursor.clone();
    app.run_on_main_thread(move || {
        let runtime = native_runtime()
            .lock()
            .expect("native capture runtime mutex poisoned");
        let coordinate_space = match runtime.coordinate_space {
            Some(coordinate_space) => coordinate_space,
            None => return,
        };
        if runtime.crosshair_panels.len() < 2 {
            return;
        }

        let horizontal = SelectionRect {
            x: coordinate_space.min_x.floor() as i32,
            y: cursor.y.floor() as i32,
            width: (coordinate_space.max_x - coordinate_space.min_x).ceil().max(1.0) as u32,
            height: 1,
        };
        let vertical = SelectionRect {
            x: cursor.x.floor() as i32,
            y: coordinate_space.min_y.floor() as i32,
            width: 1,
            height: (coordinate_space.max_y - coordinate_space.min_y).ceil().max(1.0) as u32,
        };

        set_panel_frame(runtime.crosshair_panels[0], &horizontal, coordinate_space);
        set_panel_frame(runtime.crosshair_panels[1], &vertical, coordinate_space);
        show_panel(runtime.crosshair_panels[0]);
        show_panel(runtime.crosshair_panels[1]);
    })?;
    Ok(())
}

fn panel_hide_crosshair(app: &AppHandle) -> Result<(), FlickError> {
    app.run_on_main_thread(move || {
        let runtime = native_runtime()
            .lock()
            .expect("native capture runtime mutex poisoned");
        for panel in &runtime.crosshair_panels {
            hide_panel(*panel);
        }
    })?;
    Ok(())
}

fn clear_active_session() {
    if let Ok(mut runtime) = native_runtime().lock() {
        runtime.active_session_id = None;
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

fn remember_previous_frontmost_app(state: &State<'_, AppState>) {
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

unsafe extern "C" {
    fn CGShieldingWindowLevel() -> NSInteger;
    fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
}

fn shielding_window_level() -> NSInteger {
    unsafe { CGShieldingWindowLevel() }
}
