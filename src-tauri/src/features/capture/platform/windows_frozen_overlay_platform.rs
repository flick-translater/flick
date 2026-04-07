use std::{
    mem::{size_of, zeroed},
    ptr::null_mut,
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, anyhow};
use image::ImageBuffer;
use tauri::AppHandle;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM},
    Graphics::Gdi::{
        AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
        CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, GetDC, GetDIBits, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
    },
    UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
        GetWindowLongPtrW, IDC_CROSS, LoadCursorW, RegisterClassW, SW_HIDE, SW_SHOWNA,
        SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, ShowWindow, ULW_ALPHA,
        UpdateLayeredWindow, WM_DESTROY, WM_ERASEBKGND, WM_NCCREATE, WNDCLASSW, WS_EX_LAYERED,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    },
};

use crate::{error::FlickError, models::SelectionRect, services::CachedScreenCapture};

use super::overlay::{OverlayDrawState, OverlayVisuals, border_rects};

#[derive(Default)]
struct FrozenOverlayState {
    overlay_visible: bool,
    draw_state: OverlayDrawState,
    visuals: Option<OverlayVisuals>,
    windows: Vec<WindowHandle>,
}

#[derive(Clone, Copy, Debug)]
struct WindowHandle {
    hwnd: usize,
}

struct OverlayWindowData {
    bounds: SelectionRect,
    background_bgra: Vec<u8>,
    dimmed_bgra: Vec<u8>,
    frame_bgra: Vec<u8>,
    memory_dc: usize,
    bitmap: usize,
    bitmap_bits: usize,
}

fn overlay_state() -> &'static Mutex<FrozenOverlayState> {
    static STATE: OnceLock<Mutex<FrozenOverlayState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(FrozenOverlayState::default()))
}

pub(super) fn capture_desktop_snapshot(
    bounds: &SelectionRect,
) -> anyhow::Result<CachedScreenCapture> {
    let width_i32 = i32::try_from(bounds.width).context("invalid monitor width")?;
    let height_i32 = i32::try_from(bounds.height).context("invalid monitor height")?;
    let pixels_len = usize::try_from(bounds.width)
        .ok()
        .and_then(|width| usize::try_from(bounds.height).ok().map(|height| width * height * 4))
        .context("invalid monitor pixel size")?;

    let screen_dc = unsafe { GetDC(null_mut()) };
    if screen_dc.is_null() {
        return Err(anyhow!("failed to acquire screen device context"));
    }

    let memory_dc = unsafe { CreateCompatibleDC(screen_dc) };
    if memory_dc.is_null() {
        unsafe { ReleaseDC(null_mut(), screen_dc) };
        return Err(anyhow!("failed to create memory device context"));
    }

    let bitmap = unsafe { CreateCompatibleBitmap(screen_dc, width_i32, height_i32) };
    if bitmap.is_null() {
        unsafe {
            DeleteDC(memory_dc);
            ReleaseDC(null_mut(), screen_dc);
        }
        return Err(anyhow!("failed to create compatible bitmap"));
    }

    let previous = unsafe { SelectObject(memory_dc, bitmap as HGDIOBJ) };
    if previous.is_null() {
        unsafe {
            DeleteObject(bitmap as _);
            DeleteDC(memory_dc);
            ReleaseDC(null_mut(), screen_dc);
        }
        return Err(anyhow!("failed to select bitmap into memory context"));
    }

    let blit_ok = unsafe {
        windows_sys::Win32::Graphics::Gdi::BitBlt(
            memory_dc,
            0,
            0,
            width_i32,
            height_i32,
            screen_dc,
            bounds.x,
            bounds.y,
            SRCCOPY | windows_sys::Win32::Graphics::Gdi::CAPTUREBLT,
        )
    };

    if blit_ok == 0 {
        unsafe {
            SelectObject(memory_dc, previous);
            DeleteObject(bitmap as _);
            DeleteDC(memory_dc);
            ReleaseDC(null_mut(), screen_dc);
        }
        return Err(anyhow!("failed to copy screen pixels"));
    }

    let mut pixels = vec![0u8; pixels_len];
    let mut bitmap_info = bitmap_info_for(bounds.width, bounds.height);
    let scan_lines = unsafe {
        GetDIBits(
            memory_dc,
            bitmap,
            0,
            bounds.height,
            pixels.as_mut_ptr().cast(),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        )
    };

    unsafe {
        SelectObject(memory_dc, previous);
        DeleteObject(bitmap as _);
        DeleteDC(memory_dc);
        ReleaseDC(null_mut(), screen_dc);
    }

    if scan_lines == 0 {
        return Err(anyhow!("failed to read bitmap pixels"));
    }

    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2);
        chunk[3] = 0xff;
    }

    let image = ImageBuffer::from_vec(bounds.width, bounds.height, pixels)
        .context("failed to build image from screen pixels")?;

    Ok(CachedScreenCapture::new(bounds.clone(), image))
}

pub(super) fn show_native_overlay(
    snapshots: &[CachedScreenCapture],
    visuals: OverlayVisuals,
) -> Result<(), FlickError> {
    register_overlay_window_class()?;

    let mut state = overlay_state()
        .lock()
        .map_err(|_| FlickError::Message("windows overlay state mutex poisoned".into()))?;

    state.overlay_visible = true;
    state.draw_state = OverlayDrawState::default();
    state.visuals = Some(visuals);

    while state.windows.len() < snapshots.len() {
        state.windows.push(WindowHandle {
            hwnd: create_overlay_window()? as usize,
        });
    }

    for (window, snapshot) in state.windows.iter().zip(snapshots.iter()) {
        let mut data = create_window_data(snapshot, visuals);
        initialize_layered_surface(&mut data)?;
        paint_overlay_frame(&mut data, &state.draw_state, visuals);
        attach_window_data(window.hwnd as HWND, data);
        show_overlay_window(window.hwnd as HWND, &snapshot.bounds)?;
        let data_ptr =
            unsafe { GetWindowLongPtrW(window.hwnd as HWND, GWLP_USERDATA) as *mut OverlayWindowData };
        if !data_ptr.is_null() {
            let data = unsafe { &mut *data_ptr };
            render_overlay_window(window.hwnd as HWND, data)?;
        }
    }

    for window in state.windows.iter().skip(snapshots.len()) {
        hide_overlay_window(window.hwnd as HWND);
    }

    Ok(())
}

pub(super) fn hide_native_overlay(_app: &AppHandle) -> Result<(), FlickError> {
    let mut state = overlay_state()
        .lock()
        .map_err(|_| FlickError::Message("windows overlay state mutex poisoned".into()))?;
    state.overlay_visible = false;
    state.draw_state = OverlayDrawState::default();
    state.visuals = None;

    for window in state.windows.drain(..) {
        destroy_overlay_window(window.hwnd as HWND);
    }

    Ok(())
}

pub(super) fn update_highlight(
    _app: &AppHandle,
    selection: Option<SelectionRect>,
) -> Result<(), FlickError> {
    let mut state = overlay_state()
        .lock()
        .map_err(|_| FlickError::Message("windows overlay state mutex poisoned".into()))?;
    if !state.overlay_visible {
        return Ok(());
    }

    let new_draw_state = OverlayDrawState {
        selection,
        cursor: state.draw_state.cursor,
    };
    if selections_equal(state.draw_state.selection.as_ref(), new_draw_state.selection.as_ref()) {
        return Ok(());
    }
    state.draw_state = new_draw_state.clone();
    let visuals = state
        .visuals
        .ok_or_else(|| FlickError::Message("missing windows overlay visuals".into()))?;

    for window in &state.windows {
        let data_ptr =
            unsafe { GetWindowLongPtrW(window.hwnd as HWND, GWLP_USERDATA) as *mut OverlayWindowData };
        if data_ptr.is_null() {
            continue;
        }
        let data = unsafe { &mut *data_ptr };
        paint_overlay_frame(data, &new_draw_state, visuals);
        render_overlay_window(window.hwnd as HWND, data)?;
    }

    Ok(())
}

pub(super) fn update_crosshair(
    _app: &AppHandle,
    cursor: Option<(f64, f64)>,
) -> Result<(), FlickError> {
    let mut state = overlay_state()
        .lock()
        .map_err(|_| FlickError::Message("windows overlay state mutex poisoned".into()))?;
    if !state.overlay_visible {
        return Ok(());
    }

    if cursor_equal(state.draw_state.cursor, cursor) {
        return Ok(());
    }

    state.draw_state.cursor = cursor;
    let visuals = state
        .visuals
        .ok_or_else(|| FlickError::Message("missing windows overlay visuals".into()))?;

    for window in &state.windows {
        let data_ptr =
            unsafe { GetWindowLongPtrW(window.hwnd as HWND, GWLP_USERDATA) as *mut OverlayWindowData };
        if data_ptr.is_null() {
            continue;
        }
        let data = unsafe { &mut *data_ptr };
        paint_overlay_frame(data, &state.draw_state, visuals);
        render_overlay_window(window.hwnd as HWND, data)?;
    }

    Ok(())
}

pub(super) fn pump_native_overlay_messages() {
    let mut message = windows_sys::Win32::UI::WindowsAndMessaging::MSG {
        hwnd: null_mut(),
        message: 0,
        wParam: 0,
        lParam: 0,
        time: 0,
        pt: windows_sys::Win32::Foundation::POINT { x: 0, y: 0 },
    };

    while unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::PeekMessageW(
            &mut message,
            null_mut(),
            0,
            0,
            windows_sys::Win32::UI::WindowsAndMessaging::PM_REMOVE,
        )
    } != 0
    {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::TranslateMessage(&message);
            windows_sys::Win32::UI::WindowsAndMessaging::DispatchMessageW(&message);
        }
    }
}

fn create_window_data(snapshot: &CachedScreenCapture, visuals: OverlayVisuals) -> Box<OverlayWindowData> {
    let background_bgra = rgba_to_bgra(snapshot.image.as_raw());
    let dimmed_bgra = build_dimmed_background(&background_bgra, visuals.dim_alpha);
    let frame_bgra = dimmed_bgra.clone();

    Box::new(OverlayWindowData {
        bounds: snapshot.bounds.clone(),
        background_bgra,
        dimmed_bgra,
        frame_bgra,
        memory_dc: 0,
        bitmap: 0,
        bitmap_bits: 0,
    })
}

fn selections_equal(left: Option<&SelectionRect>, right: Option<&SelectionRect>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.x == right.x
                && left.y == right.y
                && left.width == right.width
                && left.height == right.height
        }
        _ => false,
    }
}

fn cursor_equal(left: Option<(f64, f64)>, right: Option<(f64, f64)>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some((lx, ly)), Some((rx, ry))) => lx == rx && ly == ry,
        _ => false,
    }
}

fn rgba_to_bgra(bytes: &[u8]) -> Vec<u8> {
    let mut bgra = bytes.to_vec();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 0xff;
    }
    bgra
}

fn build_dimmed_background(background_bgra: &[u8], dim_alpha: f32) -> Vec<u8> {
    let mut dimmed = background_bgra.to_vec();
    let factor = (1.0 - dim_alpha).clamp(0.0, 1.0);
    for pixel in dimmed.chunks_exact_mut(4) {
        pixel[0] = (pixel[0] as f32 * factor) as u8;
        pixel[1] = (pixel[1] as f32 * factor) as u8;
        pixel[2] = (pixel[2] as f32 * factor) as u8;
        pixel[3] = 0xff;
    }
    dimmed
}

fn paint_overlay_frame(
    data: &mut OverlayWindowData,
    draw_state: &OverlayDrawState,
    visuals: OverlayVisuals,
) {
    data.frame_bgra.copy_from_slice(&data.dimmed_bgra);

    if let Some(selection) = draw_state.selection.as_ref() {
        let local = intersect_local_rect(selection, &data.bounds);
        if let Some(local) = local {
            restore_selection(&mut data.frame_bgra, &data.background_bgra, &data.bounds, &local);
            for border in border_rects(local, visuals.border_thickness) {
                draw_filled_rect(
                    &mut data.frame_bgra,
                    &data.bounds,
                    &border,
                    visuals.border_color,
                );
            }
        }
    }

    if let Some((cursor_x, cursor_y)) = draw_state.cursor {
        draw_crosshair(&mut data.frame_bgra, &data.bounds, cursor_x, cursor_y, visuals);
    }
}

fn intersect_local_rect(selection: &SelectionRect, bounds: &SelectionRect) -> Option<SelectionRect> {
    let left = selection.x.max(bounds.x);
    let top = selection.y.max(bounds.y);
    let right = (selection.x + selection.width as i32).min(bounds.x + bounds.width as i32);
    let bottom = (selection.y + selection.height as i32).min(bounds.y + bounds.height as i32);

    (right > left && bottom > top).then_some(SelectionRect {
        x: left - bounds.x,
        y: top - bounds.y,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    })
}

fn restore_selection(
    frame_bgra: &mut [u8],
    background_bgra: &[u8],
    bounds: &SelectionRect,
    local: &SelectionRect,
) {
    let width = bounds.width as usize;
    let left = local.x.max(0) as usize;
    let top = local.y.max(0) as usize;
    let right = left
        .saturating_add(local.width as usize)
        .min(bounds.width as usize);
    let bottom = top
        .saturating_add(local.height as usize)
        .min(bounds.height as usize);

    for y in top..bottom {
        let start = (y * width + left) * 4;
        let end = (y * width + right) * 4;
        frame_bgra[start..end].copy_from_slice(&background_bgra[start..end]);
    }
}

fn draw_filled_rect(
    frame_bgra: &mut [u8],
    bounds: &SelectionRect,
    rect: &SelectionRect,
    color_rgba: [u8; 4],
) {
    let width = bounds.width as usize;
    let left = rect.x.max(0) as usize;
    let top = rect.y.max(0) as usize;
    let right = left
        .saturating_add(rect.width as usize)
        .min(bounds.width as usize);
    let bottom = top
        .saturating_add(rect.height as usize)
        .min(bounds.height as usize);

    for y in top..bottom {
        for x in left..right {
            let offset = (y * width + x) * 4;
            frame_bgra[offset] = color_rgba[2];
            frame_bgra[offset + 1] = color_rgba[1];
            frame_bgra[offset + 2] = color_rgba[0];
            frame_bgra[offset + 3] = color_rgba[3];
        }
    }
}

fn draw_crosshair(
    frame_bgra: &mut [u8],
    bounds: &SelectionRect,
    cursor_x: f64,
    cursor_y: f64,
    visuals: OverlayVisuals,
) {
    if cursor_x < bounds.x as f64
        || cursor_x > (bounds.x + bounds.width as i32) as f64
        || cursor_y < bounds.y as f64
        || cursor_y > (bounds.y + bounds.height as i32) as f64
    {
        return;
    }

    let local_x = (cursor_x.floor() as i32 - bounds.x).clamp(0, bounds.width as i32 - 1);
    let local_y = (cursor_y.floor() as i32 - bounds.y).clamp(0, bounds.height as i32 - 1);
    let dash = visuals.crosshair_dash_length.max(1) as i32;
    let gap = visuals.crosshair_gap_length.max(1) as i32;

    let mut x = 0;
    while x < bounds.width as i32 {
        let segment_end = (x + dash).min(bounds.width as i32);
        draw_filled_rect(
            frame_bgra,
            bounds,
            &SelectionRect {
                x,
                y: local_y,
                width: (segment_end - x) as u32,
                height: 1,
            },
            visuals.crosshair_color,
        );
        x += dash + gap;
    }

    let mut y = 0;
    while y < bounds.height as i32 {
        let segment_end = (y + dash).min(bounds.height as i32);
        draw_filled_rect(
            frame_bgra,
            bounds,
            &SelectionRect {
                x: local_x,
                y,
                width: 1,
                height: (segment_end - y) as u32,
            },
            visuals.crosshair_color,
        );
        y += dash + gap;
    }
}

fn create_overlay_window() -> Result<HWND, FlickError> {
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            overlay_window_class_name().as_ptr(),
            overlay_window_class_name().as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
        )
    };

    if hwnd.is_null() {
        return Err(FlickError::Message("failed to create overlay window".into()));
    }

    Ok(hwnd)
}

fn attach_window_data(hwnd: HWND, data: Box<OverlayWindowData>) {
    clear_window_data(hwnd);
    let ptr = Box::into_raw(data);
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
    }
}

fn clear_window_data(hwnd: HWND) {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayWindowData };
    if !ptr.is_null() {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            let mut data = Box::from_raw(ptr);
            release_layered_surface(&mut data);
        }
    }
}

fn show_overlay_window(hwnd: HWND, bounds: &SelectionRect) -> Result<(), FlickError> {
    let width = i32::try_from(bounds.width)
        .map_err(|_| FlickError::Message("invalid overlay width".into()))?;
    let height = i32::try_from(bounds.height)
        .map_err(|_| FlickError::Message("invalid overlay height".into()))?;
    unsafe {
        SetWindowPos(
            hwnd,
            -1isize as HWND,
            bounds.x,
            bounds.y,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        ShowWindow(hwnd, SW_SHOWNA);
    }
    Ok(())
}

fn hide_overlay_window(hwnd: HWND) {
    unsafe {
        ShowWindow(hwnd, SW_HIDE);
    }
}

fn destroy_overlay_window(hwnd: HWND) {
    clear_window_data(hwnd);
    unsafe {
        DestroyWindow(hwnd);
    }
}

fn register_overlay_window_class() -> Result<(), FlickError> {
    static REGISTERED: OnceLock<Result<(), FlickError>> = OnceLock::new();
    match REGISTERED.get_or_init(|| {
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(overlay_window_proc),
            hCursor: unsafe { LoadCursorW(null_mut(), IDC_CROSS) },
            lpszClassName: overlay_window_class_name().as_ptr(),
            ..unsafe { zeroed() }
        };

        let atom = unsafe { RegisterClassW(&class) };
        if atom == 0 {
            return Err(FlickError::Message(
                "failed to register windows capture overlay class".into(),
            ));
        }
        Ok(())
    }) {
        Ok(()) => Ok(()),
        Err(error) => Err(FlickError::Message(error.to_string())),
    }
}

fn overlay_window_class_name() -> &'static [u16] {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    NAME.get_or_init(|| "FlickWindowsFrozenOverlay\0".encode_utf16().collect())
        .as_slice()
}

unsafe extern "system" fn overlay_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => 1,
        WM_ERASEBKGND => 1,
        WM_DESTROY => {
            clear_window_data(hwnd);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn bitmap_info_for(width: u32, height: u32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: width.saturating_mul(height).saturating_mul(4),
            ..unsafe { zeroed() }
        },
        ..unsafe { zeroed() }
    }
}

fn initialize_layered_surface(data: &mut OverlayWindowData) -> Result<(), FlickError> {
    let screen_dc = unsafe { GetDC(null_mut()) };
    if screen_dc.is_null() {
        return Err(FlickError::Message(
            "failed to acquire screen device context for layered overlay".into(),
        ));
    }

    let memory_dc = unsafe { CreateCompatibleDC(screen_dc) };
    if memory_dc.is_null() {
        unsafe { ReleaseDC(null_mut(), screen_dc) };
        return Err(FlickError::Message(
            "failed to create layered overlay memory dc".into(),
        ));
    }

    let bitmap_info = bitmap_info_for(data.bounds.width, data.bounds.height);
    let mut bitmap_bits = null_mut();
    let bitmap = unsafe {
        CreateDIBSection(
            screen_dc,
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bitmap_bits,
            null_mut(),
            0,
        )
    };
    unsafe { ReleaseDC(null_mut(), screen_dc) };

    if bitmap.is_null() || bitmap_bits.is_null() {
        unsafe {
            if !bitmap.is_null() {
                DeleteObject(bitmap as _);
            }
            DeleteDC(memory_dc);
        }
        return Err(FlickError::Message(
            "failed to create layered overlay bitmap".into(),
        ));
    }

    let previous = unsafe { SelectObject(memory_dc, bitmap as HGDIOBJ) };
    if previous.is_null() {
        unsafe {
            DeleteObject(bitmap as _);
            DeleteDC(memory_dc);
        }
        return Err(FlickError::Message(
            "failed to select layered overlay bitmap".into(),
        ));
    }

    data.memory_dc = memory_dc as usize;
    data.bitmap = bitmap as usize;
    data.bitmap_bits = bitmap_bits as usize;
    Ok(())
}

fn release_layered_surface(data: &mut OverlayWindowData) {
    if data.bitmap != 0 {
        unsafe {
            DeleteObject(data.bitmap as _);
        }
        data.bitmap = 0;
    }
    if data.memory_dc != 0 {
        unsafe {
            DeleteDC(data.memory_dc as _);
        }
        data.memory_dc = 0;
    }
    data.bitmap_bits = 0;
}

fn render_overlay_window(hwnd: HWND, data: &mut OverlayWindowData) -> Result<(), FlickError> {
    if data.memory_dc == 0 || data.bitmap == 0 || data.bitmap_bits == 0 {
        return Err(FlickError::Message(
            "layered overlay surface is not initialized".into(),
        ));
    }

    unsafe {
        std::ptr::copy_nonoverlapping(
            data.frame_bgra.as_ptr(),
            data.bitmap_bits as *mut u8,
            data.frame_bgra.len(),
        );
    }

    let screen_dc = unsafe { GetDC(null_mut()) };
    if screen_dc.is_null() {
        return Err(FlickError::Message(
            "failed to acquire screen dc for layered window update".into(),
        ));
    }

    let dst_point = POINT {
        x: data.bounds.x,
        y: data.bounds.y,
    };
    let src_point = POINT { x: 0, y: 0 };
    let size = SIZE {
        cx: i32::try_from(data.bounds.width)
            .map_err(|_| FlickError::Message("invalid overlay width".into()))?,
        cy: i32::try_from(data.bounds.height)
            .map_err(|_| FlickError::Message("invalid overlay height".into()))?,
    };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };

    let updated = unsafe {
        UpdateLayeredWindow(
            hwnd,
            screen_dc,
            &dst_point,
            &size,
            data.memory_dc as _,
            &src_point,
            0,
            &blend,
            ULW_ALPHA,
        )
    };
    unsafe { ReleaseDC(null_mut(), screen_dc) };

    if updated == 0 {
        return Err(FlickError::Message(
            "UpdateLayeredWindow failed for capture overlay".into(),
        ));
    }

    Ok(())
}
