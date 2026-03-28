use std::sync::{Mutex, OnceLock};

use objc2::{ClassType, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSCompositingOperation, NSGraphicsContext, NSPanel, NSRectFill,
    NSRectFillUsingOperation, NSView,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol, NSPoint, NSRect, NSSize};
use tauri::AppHandle;

use crate::{error::FlickError, models::SelectionRect};

use super::{
    CursorPosition, PanelHandle, hide_panel, overlay::OverlaySetup, panel_color, panel_ref,
    set_panel_frame, shielding_window_level, show_panel,
};
use super::overlay::{OverlayDrawState, OverlayVisuals, border_rects};

const ACCENT_RED: f64 = 0.45;
const ACCENT_GREEN: f64 = 0.74;
const ACCENT_BLUE: f64 = 1.0;
const ACCENT_ALPHA: f64 = 1.0;

#[derive(Debug, Default)]
struct CoreGraphicsBackendState {
    overlay_visible: bool,
    overlay_setup: Option<OverlaySetup>,
    draw_state: OverlayDrawState,
    visuals: Option<OverlayVisuals>,
    panels: Vec<PanelHandle>,
    views: Vec<(usize, usize)>,
}

fn backend_state() -> &'static Mutex<CoreGraphicsBackendState> {
    static STATE: OnceLock<Mutex<CoreGraphicsBackendState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(CoreGraphicsBackendState::default()))
}

define_class!(
    #[unsafe(super = NSView)]
    #[name = "FlickCoreGraphicsOverlayView"]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct CoreGraphicsOverlayView;

    unsafe impl NSObjectProtocol for CoreGraphicsOverlayView {}

    impl CoreGraphicsOverlayView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            draw_overlay_view(self);
        }
    }
);

impl CoreGraphicsOverlayView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        let view: Retained<Self> = unsafe { msg_send![super(this), init] };
        view.setFrame(frame);
        view
    }
}

pub(super) fn show_native_overlay(app: &AppHandle) -> Result<(), FlickError> {
    let overlay_setup = super::overlay::collect_overlay_setup(app)?;
    let visuals = super::overlay_visuals();

    app.run_on_main_thread({
        let overlay_setup = overlay_setup.clone();
        move || {
            let mtm = MainThreadMarker::new().expect("main thread marker unavailable");
            let mut state = backend_state().lock().expect("core graphics backend mutex poisoned");
            state.overlay_visible = true;
            state.overlay_setup = Some(overlay_setup.clone());
            state.draw_state = OverlayDrawState::default();
            state.visuals = Some(visuals);

            while state.panels.len() < overlay_setup.geometry.len() {
                state.panels.push(create_overlay_panel(mtm));
            }
            while state.views.len() < overlay_setup.geometry.len() {
                let screen_index = state.views.len();
                let overlay = &overlay_setup.geometry[screen_index];
                let view = CoreGraphicsOverlayView::new(
                    mtm,
                    NSRect::new(
                        NSPoint::new(0.0, 0.0),
                        NSSize::new(overlay.width as f64, overlay.height as f64),
                    ),
                );
                let panel = unsafe { panel_ref(state.panels[screen_index]) };
                let view_ref: &CoreGraphicsOverlayView = view.as_ref();
                panel.setContentView(Some(view_ref.as_super()));
                state.views.push((screen_index, Retained::into_raw(view) as usize));
            }

            for (panel, geometry) in state.panels.iter().zip(overlay_setup.geometry.iter()) {
                set_panel_frame(*panel, geometry, overlay_setup.coordinate_space);
                show_panel(*panel);
            }
            for panel in state.panels.iter().skip(overlay_setup.geometry.len()) {
                hide_panel(*panel);
            }

            request_redraw_locked(&state);
        }
    })?;

    Ok(())
}

pub(super) fn hide_native_overlay(app: &AppHandle) -> Result<(), FlickError> {
    app.run_on_main_thread(move || {
        let mut state = backend_state().lock().expect("core graphics backend mutex poisoned");
        state.overlay_visible = false;
        state.overlay_setup = None;
        state.draw_state = OverlayDrawState::default();
        state.visuals = None;
        for panel in &state.panels {
            hide_panel(*panel);
        }
    })?;
    Ok(())
}

pub(super) fn update_highlight(
    app: &AppHandle,
    selection: Option<SelectionRect>,
) -> Result<(), FlickError> {
    app.run_on_main_thread(move || {
        let mut state = backend_state().lock().expect("core graphics backend mutex poisoned");
        state.draw_state.selection = selection;
        request_redraw_locked(&state);
    })?;
    Ok(())
}

pub(super) fn update_crosshair(
    app: &AppHandle,
    cursor: &CursorPosition,
) -> Result<(), FlickError> {
    let cursor = (cursor.x, cursor.y);
    app.run_on_main_thread(move || {
        let mut state = backend_state().lock().expect("core graphics backend mutex poisoned");
        state.draw_state.cursor = Some(cursor);
        request_redraw_locked(&state);
    })?;
    Ok(())
}

pub(super) fn hide_crosshair(app: &AppHandle) -> Result<(), FlickError> {
    app.run_on_main_thread(move || {
        let mut state = backend_state().lock().expect("core graphics backend mutex poisoned");
        state.draw_state.cursor = None;
        request_redraw_locked(&state);
    })?;
    Ok(())
}

fn create_overlay_panel(mtm: MainThreadMarker) -> PanelHandle {
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        NSBackingStoreType::Buffered,
        false,
    );

    panel.setOpaque(false);
    panel.setHasShadow(false);
    panel.setBackgroundColor(Some(&panel_color(0.0, 0.0, 0.0, 0.0)));
    panel.setIgnoresMouseEvents(false);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::Stationary,
    );
    panel.setLevel(shielding_window_level());
    panel.setFloatingPanel(true);
    panel.setBecomesKeyOnlyIfNeeded(false);
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.orderOut(None);

    PanelHandle {
        ptr: Retained::into_raw(panel) as usize,
    }
}

fn request_redraw_locked(state: &CoreGraphicsBackendState) {
    for (_, view) in &state.views {
        unsafe { overlay_view_ref(*view) }.setNeedsDisplay(true);
    }
}

fn draw_overlay_view(view: &CoreGraphicsOverlayView) {
    let state = match backend_state().lock() {
        Ok(state) => state,
        Err(_) => return,
    };
    let view_ptr = view as *const CoreGraphicsOverlayView as usize;
    let Some((screen_index, _)) = state.views.iter().find(|(_, ptr)| *ptr == view_ptr) else {
        return;
    };
    if !state.overlay_visible {
        return;
    }

    let Some(setup) = state.overlay_setup.as_ref() else {
        return;
    };
    let Some(overlay) = setup.geometry.get(*screen_index).cloned() else {
        return;
    };
    let Some(visuals) = state.visuals else {
        return;
    };

    let overlay_rect = local_rect(&overlay, &overlay);
    NSColor::colorWithSRGBRed_green_blue_alpha(0.0, 0.0, 0.0, visuals.dim_alpha).setFill();
    NSRectFill(overlay_rect);

    if let Some(selection) = state.draw_state.selection.clone() {
        if let Some(intersection) = intersect_rect(&selection, &overlay) {
            let fill_rect = local_rect(&intersection, &overlay);
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
                NSRectFill(local_rect(&border, &overlay));
            }
        }
    }

    if let Some((cursor_x, cursor_y)) = state.draw_state.cursor {
        if point_in_rect(cursor_x, cursor_y, &overlay) {
            NSColor::colorWithSRGBRed_green_blue_alpha(
                ACCENT_RED,
                ACCENT_GREEN,
                ACCENT_BLUE,
                ACCENT_ALPHA,
            )
            .setFill();
            NSRectFill(local_rect(
                &SelectionRect {
                    x: overlay.x,
                    y: cursor_y.floor() as i32,
                    width: overlay.width,
                    height: 1,
                },
                &overlay,
            ));
            NSRectFill(local_rect(
                &SelectionRect {
                    x: cursor_x.floor() as i32,
                    y: overlay.y,
                    width: 1,
                    height: overlay.height,
                },
                &overlay,
            ));
        }
    }
}

fn local_rect(rect: &SelectionRect, overlay: &SelectionRect) -> NSRect {
    NSRect::new(
        NSPoint::new((rect.x - overlay.x) as f64, overlay.height as f64 - (rect.y - overlay.y) as f64 - rect.height as f64),
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

unsafe fn overlay_view_ref(ptr: usize) -> &'static CoreGraphicsOverlayView {
    unsafe { &*(ptr as *const CoreGraphicsOverlayView) }
}
