use std::sync::{Arc, Mutex, OnceLock};

use core_graphics::{
    base::kCGRenderingIntentDefault,
    color_space::CGColorSpace,
    data_provider::CGDataProvider,
    image::{CGImage, CGImageAlphaInfo},
};
use foreign_types::ForeignType;
use objc2::{AnyThread, ClassType, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSCompositingOperation, NSCursor, NSGraphicsContext, NSImage,
    NSRectFill, NSRectFillUsingOperation, NSView, NSWindow, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol, NSPoint, NSRect, NSSize};
use tauri::AppHandle;

use crate::{error::FlickError, models::SelectionRect, services::CachedScreenCapture};

use super::{
    CursorPosition, overlay::CoordinateSpace, overlay::OverlayDrawState, overlay::OverlayVisuals,
    overlay::border_rects, shielding_window_level,
};

const ACCENT_RED: f64 = 0.45;
const ACCENT_GREEN: f64 = 0.74;
const ACCENT_BLUE: f64 = 1.0;
const ACCENT_ALPHA: f64 = 1.0;
const PREPARING_BLOCKER_ALPHA: f64 = 0.12;
const INTERACTIVE_BLOCKER_ALPHA: f64 = 0.001;

#[derive(Debug, Default)]
struct FrozenOverlayState {
    overlay_visible: bool,
    coordinate_space: Option<CoordinateSpace>,
    overlay_geometry: Vec<SelectionRect>,
    draw_state: OverlayDrawState,
    visuals: Option<OverlayVisuals>,
    image_windows: Vec<WindowHandle>,
    blocker_windows: Vec<WindowHandle>,
    views: Vec<(usize, usize)>,
    snapshot_images: Vec<usize>,
}

#[derive(Clone, Copy, Debug)]
struct WindowHandle {
    ptr: usize,
}

fn overlay_state() -> &'static Mutex<FrozenOverlayState> {
    static STATE: OnceLock<Mutex<FrozenOverlayState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(FrozenOverlayState::default()))
}

define_class!(
    #[unsafe(super = NSView)]
    #[name = "FlickFrozenOverlayView"]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct FrozenOverlayView;

    unsafe impl NSObjectProtocol for FrozenOverlayView {}

    impl FrozenOverlayView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            draw_overlay_view(self);
        }
    }
);

impl FrozenOverlayView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        let view: Retained<Self> = unsafe { msg_send![super(this), init] };
        view.setFrame(frame);
        view
    }
}

pub(super) fn show_preparing_overlay(
    app: &AppHandle,
    geometry: &[SelectionRect],
) -> Result<(), FlickError> {
    let geometry = geometry.to_vec();
    let started_at = std::time::Instant::now();

    app.run_on_main_thread(move || {
        let mtm = MainThreadMarker::new().expect("main thread marker unavailable");
        let mut state = overlay_state()
            .lock()
            .expect("frozen overlay mutex poisoned");
        ensure_blocker_windows(&mut state, mtm, geometry.len());
        for (window, rect) in state.blocker_windows.iter().zip(geometry.iter()) {
            set_window_background(*window, PREPARING_BLOCKER_ALPHA);
            set_window_frame(*window, rect, coordinate_space_for(rect));
            show_window(*window);
        }
        for window in state.blocker_windows.iter().skip(geometry.len()) {
            hide_window(*window);
        }
    })?;
    eprintln!(
        "capture-perf: preparing_overlay duration_ms={}",
        started_at.elapsed().as_millis()
    );
    Ok(())
}

pub(super) fn show_native_overlay(
    app: &AppHandle,
    snapshots: &[CachedScreenCapture],
    visuals: OverlayVisuals,
) -> Result<(), FlickError> {
    let snapshots = snapshots.to_vec();
    let geometry = snapshots
        .iter()
        .map(|snapshot| snapshot.bounds.clone())
        .collect::<Vec<_>>();
    let coordinate_space = build_coordinate_space(&geometry);
    let started_at = std::time::Instant::now();

    app.run_on_main_thread(move || {
        let mtm = MainThreadMarker::new().expect("main thread marker unavailable");
        let mut state = overlay_state()
            .lock()
            .expect("frozen overlay mutex poisoned");

        for ptr in state.snapshot_images.drain(..) {
            unsafe {
                drop(Retained::from_raw(ptr as *mut NSImage));
            }
        }

        let images = snapshots
            .iter()
            .map(|snapshot| make_ns_image(snapshot, mtm))
            .collect::<Vec<_>>();
        state.overlay_visible = true;
        state.coordinate_space = Some(coordinate_space);
        state.overlay_geometry = geometry.clone();
        state.draw_state = OverlayDrawState::default();
        state.visuals = Some(visuals);
        state.snapshot_images = images
            .into_iter()
            .map(|image| Retained::into_raw(image) as usize)
            .collect();

        ensure_image_windows(&mut state, mtm, geometry.len());
        ensure_blocker_windows(&mut state, mtm, geometry.len());

        while state.views.len() < geometry.len() {
            let screen_index = state.views.len();
            let rect = &geometry[screen_index];
            let view = FrozenOverlayView::new(
                mtm,
                NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(rect.width as f64, rect.height as f64),
                ),
            );
            let window = unsafe { window_ref(state.image_windows[screen_index]) };
            let view_ref: &FrozenOverlayView = view.as_ref();
            window.setContentView(Some(view_ref.as_super()));
            state
                .views
                .push((screen_index, Retained::into_raw(view) as usize));
        }

        for ((window, rect), (_, view_ptr)) in state
            .image_windows
            .iter()
            .zip(geometry.iter())
            .zip(state.views.iter())
        {
            unsafe { overlay_view_ref(*view_ptr) }.setFrame(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(rect.width as f64, rect.height as f64),
            ));
            set_window_frame(*window, rect, coordinate_space);
            show_window(*window);
        }
        for (window, rect) in state.blocker_windows.iter().zip(geometry.iter()) {
            set_window_background(*window, INTERACTIVE_BLOCKER_ALPHA);
            set_window_frame(*window, rect, coordinate_space);
            show_window(*window);
        }
        for window in state.image_windows.iter().skip(geometry.len()) {
            hide_window(*window);
        }
        for window in state.blocker_windows.iter().skip(geometry.len()) {
            hide_window(*window);
        }

        let cursor = NSCursor::crosshairCursor();
        cursor.push();
        cursor.set();

        request_redraw_locked(&state);
    })?;
    eprintln!(
        "capture-perf: frozen_overlay duration_ms={}",
        started_at.elapsed().as_millis()
    );
    Ok(())
}

pub(super) fn hide_native_overlay(app: &AppHandle) -> Result<(), FlickError> {
    app.run_on_main_thread(move || {
        let mut state = overlay_state()
            .lock()
            .expect("frozen overlay mutex poisoned");
        state.overlay_visible = false;
        state.coordinate_space = None;
        state.overlay_geometry.clear();
        state.draw_state = OverlayDrawState::default();
        state.visuals = None;
        for window in &state.image_windows {
            hide_window(*window);
        }
        for window in &state.blocker_windows {
            hide_window(*window);
        }
        for ptr in state.snapshot_images.drain(..) {
            unsafe {
                drop(Retained::from_raw(ptr as *mut NSImage));
            }
        }

        let cursor = NSCursor::currentCursor();
        cursor.pop();
    })?;
    Ok(())
}

pub(super) fn update_highlight(
    app: &AppHandle,
    selection: Option<SelectionRect>,
) -> Result<(), FlickError> {
    app.run_on_main_thread(move || {
        let mut state = overlay_state()
            .lock()
            .expect("frozen overlay mutex poisoned");
        state.draw_state.selection = selection;
        request_redraw_locked(&state);
    })?;
    Ok(())
}

pub(super) fn update_crosshair(app: &AppHandle, cursor: &CursorPosition) -> Result<(), FlickError> {
    let cursor = (cursor.x, cursor.y);
    app.run_on_main_thread(move || {
        let mut state = overlay_state()
            .lock()
            .expect("frozen overlay mutex poisoned");
        state.draw_state.cursor = Some(cursor);
        request_redraw_locked(&state);
    })?;
    Ok(())
}

pub(super) fn hide_crosshair(app: &AppHandle) -> Result<(), FlickError> {
    app.run_on_main_thread(move || {
        let mut state = overlay_state()
            .lock()
            .expect("frozen overlay mutex poisoned");
        state.draw_state.cursor = None;
        request_redraw_locked(&state);
    })?;
    Ok(())
}

fn make_ns_image(snapshot: &CachedScreenCapture, mtm: MainThreadMarker) -> Retained<NSImage> {
    let bytes = Arc::new(snapshot.image.as_raw().clone());
    let provider = CGDataProvider::from_buffer(bytes);
    let color_space = CGColorSpace::create_device_rgb();
    let cg_image = CGImage::new(
        snapshot.image.width() as usize,
        snapshot.image.height() as usize,
        8,
        32,
        snapshot.image.width() as usize * 4,
        &color_space,
        CGImageAlphaInfo::CGImageAlphaLast as u32,
        &provider,
        false,
        kCGRenderingIntentDefault,
    );

    let _ = mtm;
    unsafe {
        msg_send![
            NSImage::alloc(),
            initWithCGImage: cg_image.as_ptr().cast::<std::ffi::c_void>(),
            size: NSSize::new(snapshot.bounds.width as f64, snapshot.bounds.height as f64)
        ]
    }
}

fn panel_color(red: f64, green: f64, blue: f64, alpha: f64) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, alpha)
}

fn create_overlay_window(mtm: MainThreadMarker) -> WindowHandle {
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };

    window.setOpaque(false);
    window.setHasShadow(false);
    window.setBackgroundColor(Some(&panel_color(0.0, 0.0, 0.0, 0.0)));
    window.setIgnoresMouseEvents(true);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::Stationary,
    );
    window.setLevel(shielding_window_level());
    unsafe { window.setReleasedWhenClosed(false) };
    window.orderOut(None);

    WindowHandle {
        ptr: Retained::into_raw(window) as usize,
    }
}

fn create_blocker_window(mtm: MainThreadMarker) -> WindowHandle {
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };

    window.setOpaque(false);
    window.setHasShadow(false);
    window.setBackgroundColor(Some(&panel_color(0.0, 0.0, 0.0, 0.001)));
    window.setIgnoresMouseEvents(false);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::Stationary,
    );
    window.setLevel(shielding_window_level() + 1);
    unsafe { window.setReleasedWhenClosed(false) };
    window.orderOut(None);

    WindowHandle {
        ptr: Retained::into_raw(window) as usize,
    }
}

fn ensure_image_windows(state: &mut FrozenOverlayState, mtm: MainThreadMarker, count: usize) {
    while state.image_windows.len() < count {
        state.image_windows.push(create_overlay_window(mtm));
    }
}

fn ensure_blocker_windows(state: &mut FrozenOverlayState, mtm: MainThreadMarker, count: usize) {
    while state.blocker_windows.len() < count {
        state.blocker_windows.push(create_blocker_window(mtm));
    }
}

fn set_window_frame(
    window: WindowHandle,
    selection: &SelectionRect,
    coordinate_space: CoordinateSpace,
) {
    let width = selection.width as f64;
    let height = selection.height as f64;
    let y = coordinate_space.max_y - selection.y as f64 - height + coordinate_space.min_y;
    let rect = NSRect::new(
        NSPoint::new(selection.x as f64, y),
        NSSize::new(width, height),
    );
    unsafe {
        window_ref(window).setFrame_display(rect, false);
    }
}

fn show_window(window: WindowHandle) {
    unsafe {
        window_ref(window).orderFrontRegardless();
    }
}

fn hide_window(window: WindowHandle) {
    unsafe {
        window_ref(window).orderOut(None);
    }
}

fn set_window_background(window: WindowHandle, alpha: f64) {
    unsafe {
        window_ref(window).setBackgroundColor(Some(&panel_color(0.0, 0.0, 0.0, alpha)));
    }
}

unsafe fn window_ref(window: WindowHandle) -> &'static NSWindow {
    unsafe { &*(window.ptr as *const NSWindow) }
}

fn request_redraw_locked(state: &FrozenOverlayState) {
    for (_, view) in &state.views {
        unsafe { overlay_view_ref(*view) }.setNeedsDisplay(true);
    }
}

fn draw_overlay_view(_view: &FrozenOverlayView) {
    let state = match overlay_state().lock() {
        Ok(state) => state,
        Err(_) => return,
    };
    if !state.overlay_visible {
        return;
    }

    let view_ptr = _view as *const FrozenOverlayView as usize;
    let Some((screen_index, _)) = state.views.iter().find(|(_, ptr)| *ptr == view_ptr) else {
        return;
    };
    let Some(visuals) = state.visuals else {
        return;
    };
    let Some(bounds) = state.overlay_geometry.get(*screen_index) else {
        return;
    };

    let overlay_rect = local_rect(bounds, bounds);
    if let Some(snapshot_ptr) = state.snapshot_images.get(*screen_index).copied() {
        unsafe { snapshot_image_ref(snapshot_ptr) }.drawInRect(overlay_rect);
    }

    NSColor::colorWithSRGBRed_green_blue_alpha(0.0, 0.0, 0.0, visuals.dim_alpha).setFill();
    NSRectFill(overlay_rect);

    if let Some(selection) = state.draw_state.selection.clone() {
        if let Some(intersection) = intersect_rect(&selection, bounds) {
            let fill_rect = local_rect(&intersection, bounds);
            NSGraphicsContext::saveGraphicsState_class();
            NSColor::clearColor().setFill();
            NSRectFillUsingOperation(fill_rect, NSCompositingOperation::Clear);
            NSGraphicsContext::restoreGraphicsState_class();

            NSColor::colorWithSRGBRed_green_blue_alpha(
                ACCENT_RED,
                ACCENT_GREEN,
                ACCENT_BLUE,
                ACCENT_ALPHA,
            )
            .setFill();
            for border in border_rects(intersection, visuals.border_thickness) {
                NSRectFill(local_rect(&border, bounds));
            }
        }
    }

    if let Some((cursor_x, cursor_y)) = state.draw_state.cursor {
        if point_in_rect(cursor_x, cursor_y, bounds) {
            NSColor::colorWithSRGBRed_green_blue_alpha(
                ACCENT_RED,
                ACCENT_GREEN,
                ACCENT_BLUE,
                ACCENT_ALPHA,
            )
            .setFill();
            NSRectFill(local_rect(
                &SelectionRect {
                    x: bounds.x,
                    y: cursor_y.floor() as i32,
                    width: bounds.width,
                    height: 1,
                },
                bounds,
            ));
            NSRectFill(local_rect(
                &SelectionRect {
                    x: cursor_x.floor() as i32,
                    y: bounds.y,
                    width: 1,
                    height: bounds.height,
                },
                bounds,
            ));
        }
    }
}

fn local_rect(rect: &SelectionRect, overlay: &SelectionRect) -> NSRect {
    NSRect::new(
        NSPoint::new(
            (rect.x - overlay.x) as f64,
            overlay.height as f64 - (rect.y - overlay.y) as f64 - rect.height as f64,
        ),
        NSSize::new(rect.width as f64, rect.height as f64),
    )
}

fn point_in_rect(x: f64, y: f64, rect: &SelectionRect) -> bool {
    x >= rect.x as f64
        && x <= rect.x as f64 + rect.width as f64
        && y >= rect.y as f64
        && y <= rect.y as f64 + rect.height as f64
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

unsafe fn overlay_view_ref(ptr: usize) -> &'static FrozenOverlayView {
    unsafe { &*(ptr as *const FrozenOverlayView) }
}

unsafe fn snapshot_image_ref(ptr: usize) -> &'static NSImage {
    unsafe { &*(ptr as *const NSImage) }
}

fn build_coordinate_space(geometry: &[SelectionRect]) -> CoordinateSpace {
    let min_y = geometry
        .iter()
        .map(|rect| rect.y as f64)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let max_y = geometry
        .iter()
        .map(|rect| rect.y as f64 + rect.height as f64)
        .reduce(f64::max)
        .unwrap_or(0.0);

    CoordinateSpace { min_y, max_y }
}

fn coordinate_space_for(rect: &SelectionRect) -> CoordinateSpace {
    CoordinateSpace {
        min_y: rect.y as f64,
        max_y: rect.y as f64 + rect.height as f64,
    }
}
