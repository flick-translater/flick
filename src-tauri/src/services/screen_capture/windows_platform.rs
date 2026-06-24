use std::{
    mem::{size_of, zeroed},
    ptr::null_mut,
};

use anyhow::{Context, anyhow};
use image::{ImageBuffer, Rgba, imageops};
use windows_capture::{
    capture::{CaptureControl, Context as CaptureContext, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
};
use windows_sys::Win32::{
    Foundation::POINT,
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CAPTUREBLT, CreateCompatibleBitmap,
        CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits,
        GetMonitorInfoW, HGDIOBJ, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO,
        MonitorFromPoint, ReleaseDC, SRCCOPY, SelectObject,
    },
};

use crate::models::SelectionRect;

use super::{CachedScreenCapture, LiveFrameStreamHandle};

type StreamError = Box<dyn std::error::Error + Send + Sync>;

pub struct WindowsLiveFrameStreamHandle {
    control: Option<CaptureControl<WindowsStreamHandler, StreamError>>,
}

impl LiveFrameStreamHandle for WindowsLiveFrameStreamHandle {}

impl Drop for WindowsLiveFrameStreamHandle {
    fn drop(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.stop();
        }
    }
}

struct WindowsStreamConfig {
    crop: StreamCrop,
    on_frame: Box<dyn FnMut(ImageBuffer<Rgba<u8>, Vec<u8>>) + Send>,
}

#[derive(Clone, Copy)]
struct StreamCrop {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

struct MonitorBounds {
    hmonitor: HMONITOR,
    x: i32,
    y: i32,
}

struct WindowsStreamHandler {
    crop: StreamCrop,
    on_frame: Box<dyn FnMut(ImageBuffer<Rgba<u8>, Vec<u8>>) + Send>,
    scratch: Vec<u8>,
}

impl GraphicsCaptureApiHandler for WindowsStreamHandler {
    type Flags = WindowsStreamConfig;
    type Error = StreamError;

    fn new(ctx: CaptureContext<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            crop: ctx.flags.crop,
            on_frame: ctx.flags.on_frame,
            scratch: Vec::new(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let buffer = frame.buffer()?;
        let width = buffer.width();
        let height = buffer.height();
        if self.crop.x.saturating_add(self.crop.width) > width
            || self.crop.y.saturating_add(self.crop.height) > height
        {
            return Ok(());
        }

        let pixels = buffer.as_nopadding_buffer(&mut self.scratch);
        let image = crop_rgba_buffer(
            pixels,
            width,
            self.crop.x,
            self.crop.y,
            self.crop.width,
            self.crop.height,
        )?;
        (self.on_frame)(image);
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

pub fn open_live_frame_stream(
    selection: &SelectionRect,
    on_frame: Box<dyn FnMut(ImageBuffer<Rgba<u8>, Vec<u8>>) + Send>,
) -> anyhow::Result<Box<dyn LiveFrameStreamHandle>> {
    if selection.width < 2 || selection.height < 2 {
        return Err(anyhow!("selection is too small"));
    }

    let monitor_bounds = monitor_bounds_for_selection(selection)?;
    let monitor = Monitor::from_raw_hmonitor(monitor_bounds.hmonitor as _);
    let crop = StreamCrop {
        x: u32::try_from(selection.x - monitor_bounds.x)
            .context("selection extends beyond stream monitor on the left edge")?,
        y: u32::try_from(selection.y - monitor_bounds.y)
            .context("selection extends beyond stream monitor on the top edge")?,
        width: selection.width,
        height: selection.height,
    };

    let settings = Settings::new(
        monitor,
        CursorCaptureSettings::WithoutCursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Exclude,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        WindowsStreamConfig { crop, on_frame },
    );

    let control = WindowsStreamHandler::start_free_threaded(settings)
        .map_err(|error| anyhow!("failed to start Windows Graphics Capture stream: {error}"))?;
    Ok(Box::new(WindowsLiveFrameStreamHandle {
        control: Some(control),
    }))
}

fn monitor_bounds_for_selection(selection: &SelectionRect) -> anyhow::Result<MonitorBounds> {
    let center = POINT {
        x: selection.x + (selection.width / 2) as i32,
        y: selection.y + (selection.height / 2) as i32,
    };
    let hmonitor = unsafe { MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST) };
    if hmonitor.is_null() {
        return Err(anyhow!("failed to find monitor for selection"));
    }

    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        rcMonitor: Default::default(),
        rcWork: Default::default(),
        dwFlags: 0,
    };
    let ok = unsafe { GetMonitorInfoW(hmonitor, &mut info) };
    if ok == 0 {
        return Err(anyhow!("failed to read monitor bounds for stream capture"));
    }

    let left = info.rcMonitor.left;
    let top = info.rcMonitor.top;
    Ok(MonitorBounds {
        hmonitor,
        x: left,
        y: top,
    })
}

fn crop_rgba_buffer(
    pixels: &[u8],
    source_width: u32,
    crop_x: u32,
    crop_y: u32,
    crop_width: u32,
    crop_height: u32,
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let source_width = usize::try_from(source_width).context("invalid stream source width")?;
    let crop_x = usize::try_from(crop_x).context("invalid stream crop x")?;
    let crop_y = usize::try_from(crop_y).context("invalid stream crop y")?;
    let crop_width = usize::try_from(crop_width).context("invalid stream crop width")?;
    let crop_height = usize::try_from(crop_height).context("invalid stream crop height")?;
    let row_len = crop_width
        .checked_mul(4)
        .ok_or_else(|| anyhow!("stream crop row size overflow"))?;
    let mut out = Vec::with_capacity(
        crop_height
            .checked_mul(row_len)
            .ok_or_else(|| anyhow!("stream crop image size overflow"))?,
    );

    for row in 0..crop_height {
        let source_start = ((crop_y + row) * source_width + crop_x)
            .checked_mul(4)
            .ok_or_else(|| anyhow!("stream crop offset overflow"))?;
        let source_end = source_start
            .checked_add(row_len)
            .ok_or_else(|| anyhow!("stream crop offset overflow"))?;
        if source_end > pixels.len() {
            return Err(anyhow!("stream frame buffer is smaller than expected"));
        }
        out.extend_from_slice(&pixels[source_start..source_end]);
    }

    ImageBuffer::from_vec(crop_width as u32, crop_height as u32, out)
        .context("failed to build image from Windows stream frame")
}

pub fn capture_selection(
    selection: &SelectionRect,
    cached_screens: &[CachedScreenCapture],
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    if cached_screens.is_empty() {
        return capture_live_selection(selection);
    }

    let snapshot = cached_screens
        .iter()
        .find(|snapshot| selection_fits_within(selection, &snapshot.bounds))
        .ok_or_else(|| anyhow!("missing cached screen capture for selection"))?;

    let relative_x = u32::try_from(selection.x - snapshot.bounds.x)
        .context("selection extends beyond cached capture on the left edge")?;
    let relative_y = u32::try_from(selection.y - snapshot.bounds.y)
        .context("selection extends beyond cached capture on the top edge")?;

    if relative_x.saturating_add(selection.width) > snapshot.image.width()
        || relative_y.saturating_add(selection.height) > snapshot.image.height()
    {
        return Err(anyhow!("selection extends beyond cached capture bounds"));
    }

    Ok(imageops::crop_imm(
        snapshot.image.as_ref(),
        relative_x,
        relative_y,
        selection.width,
        selection.height,
    )
    .to_image())
}

fn capture_live_selection(
    selection: &SelectionRect,
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let width_i32 = i32::try_from(selection.width).context("invalid selection width")?;
    let height_i32 = i32::try_from(selection.height).context("invalid selection height")?;
    let pixels_len = usize::try_from(selection.width)
        .ok()
        .and_then(|width| {
            usize::try_from(selection.height)
                .ok()
                .map(|height| width * height * 4)
        })
        .context("invalid selection pixel size")?;

    let screen_dc = unsafe { GetDC(null_mut()) };
    if screen_dc.is_null() {
        return Err(anyhow!("failed to acquire screen device context"));
    }

    let memory_dc = unsafe { CreateCompatibleDC(screen_dc) };
    if memory_dc.is_null() {
        unsafe {
            ReleaseDC(null_mut(), screen_dc);
        }
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
            selection.x,
            selection.y,
            SRCCOPY | CAPTUREBLT,
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
    let mut bitmap_info = bitmap_info_for(selection.width, selection.height);
    let scan_lines = unsafe {
        GetDIBits(
            memory_dc,
            bitmap,
            0,
            selection.height,
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

    ImageBuffer::from_vec(selection.width, selection.height, pixels)
        .context("failed to build image from screen pixels")
}

fn bitmap_info_for(width: u32, height: u32) -> BITMAPINFO {
    let mut info = unsafe { zeroed::<BITMAPINFO>() };
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        // Negative height requests a top-down DIB, matching screen coordinate order.
        biHeight: -(height as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..unsafe { zeroed() }
    };
    info
}

fn selection_fits_within(selection: &SelectionRect, bounds: &SelectionRect) -> bool {
    let selection_right = selection.x + selection.width as i32;
    let selection_bottom = selection.y + selection.height as i32;
    let bounds_right = bounds.x + bounds.width as i32;
    let bounds_bottom = bounds.y + bounds.height as i32;

    selection.x >= bounds.x
        && selection.y >= bounds.y
        && selection_right <= bounds_right
        && selection_bottom <= bounds_bottom
}
