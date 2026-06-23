//! macOS capture backend based on ScreenCaptureKit.

use std::{mem, sync::Mutex};

use anyhow::anyhow;
use core_graphics::image::CGImage;
use foreign_types::ForeignType;
use image::{ImageBuffer, Rgba};
use screencapturekit::{
    cg::CGRect,
    cm::{CMSampleBuffer, CMTime, SCFrameStatus},
    cv::CVPixelBufferLockFlags,
    dispatch_queue::{DispatchQoS, DispatchQueue},
    prelude::{
        PixelFormat, SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration,
        SCStreamOutputTrait, SCStreamOutputType,
    },
    screenshot_manager::{CGImage as ScCgImage, SCScreenshotManager},
};

use crate::{
    models::SelectionRect,
    services::{CachedScreenCapture, screen_capture::MacosCaptureBackend},
};

pub struct ScreenCaptureKitBackend;

impl MacosCaptureBackend for ScreenCaptureKitBackend {
    fn name(&self) -> &'static str {
        "ScreenCaptureKit"
    }

    fn capture_selection(
        &self,
        selection: &SelectionRect,
    ) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
        capture_selection(selection)
    }

    fn capture_desktop_snapshot(
        &self,
        bounds: &SelectionRect,
    ) -> anyhow::Result<CachedScreenCapture> {
        capture_desktop_snapshot(bounds)
    }
}

pub fn capture_selection(
    selection: &SelectionRect,
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    if selection.width < 2 || selection.height < 2 {
        return Err(anyhow!("selection is too small"));
    }

    let image = capture_image_in_rect(selection)?;
    rgba_image_from_sckit(image)
}

pub fn capture_desktop_snapshot(bounds: &SelectionRect) -> anyhow::Result<CachedScreenCapture> {
    if bounds.width < 2 || bounds.height < 2 {
        return Err(anyhow!("desktop bounds are too small"));
    }

    let image = capture_image_in_rect(bounds)?;
    let cg_image = transfer_to_core_graphics(image);
    Ok(CachedScreenCapture::new(bounds.clone(), cg_image))
}

fn display_for_selection<'a>(
    selection: &SelectionRect,
    displays: &'a [screencapturekit::shareable_content::SCDisplay],
) -> Option<&'a screencapturekit::shareable_content::SCDisplay> {
    let center_x = selection.x as f64 + selection.width as f64 / 2.0;
    let center_y = selection.y as f64 + selection.height as f64 / 2.0;
    displays
        .iter()
        .find(|display| {
            let frame = display.frame();
            center_x >= frame.x
                && center_x <= frame.x + frame.width
                && center_y >= frame.y
                && center_y <= frame.y + frame.height
        })
        .or_else(|| displays.first())
}

/// How to crop the captured output to the selection. The stream is configured to output exactly the
/// selection rect, so this is the whole output; kept as a struct for the shared decode helper.
#[derive(Clone, Copy)]
struct StreamCrop {
    display_x: f64,
    display_y: f64,
    display_width: f64,
    display_height: f64,
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
}

impl StreamCrop {
    fn entire_output() -> Self {
        Self {
            display_x: 0.0,
            display_y: 0.0,
            display_width: 1.0,
            display_height: 1.0,
            selection_x: 0.0,
            selection_y: 0.0,
            selection_width: 1.0,
            selection_height: 1.0,
        }
    }
}

/// Push-mode handler: decodes each delivered frame to RGBA immediately (releasing the CVPixelBuffer
/// back to the stream's pool right away) and hands the owned image to the callback. Decoding here,
/// rather than holding the pixel buffer down the pipeline, keeps the stream's buffer pool free so it
/// can emit at full rate.
struct PushFrameHandler {
    on_frame: Mutex<Box<dyn FnMut(ImageBuffer<Rgba<u8>, Vec<u8>>) + Send>>,
    crop: StreamCrop,
}

impl SCStreamOutputTrait for PushFrameHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if of_type != SCStreamOutputType::Screen {
            return;
        }
        if sample
            .frame_status()
            .is_some_and(|status| status != SCFrameStatus::Complete)
        {
            return;
        }
        let Some(pixel_buffer) = sample.image_buffer() else {
            return;
        };
        // Decode now, then drop `pixel_buffer` (end of scope) so the pool slot is freed immediately.
        let image = match rgba_image_from_pixel_buffer_crop(&pixel_buffer, self.crop) {
            Ok(image) => image,
            Err(error) => {
                eprintln!("[long-capture] push handler: decode failed {error}");
                return;
            }
        };
        drop(pixel_buffer);
        if let Ok(mut cb) = self.on_frame.lock() {
            (cb)(image);
        }
    }
}

/// Owns a running push-mode stream; dropping it stops capture.
pub struct PushStreamHandle {
    stream: SCStream,
    handler_id: Option<usize>,
    output_queue: DispatchQueue,
}

impl crate::services::screen_capture::LiveFrameStreamHandle for PushStreamHandle {}

impl Drop for PushStreamHandle {
    fn drop(&mut self) {
        if let Some(handler_id) = self.handler_id.take() {
            let _ = self
                .stream
                .remove_output_handler(handler_id, SCStreamOutputType::Screen);
        }
        drain_dispatch_queue(&self.output_queue);
        let _ = self.stream.stop_capture();
        drain_dispatch_queue(&self.output_queue);
    }
}

/// Open a push-mode live stream: `on_frame` is called with each delivered frame already decoded to
/// RGBA (the decode happens on the delivery thread so the pixel-buffer pool is freed immediately).
pub fn open_live_frame_stream(
    selection: &SelectionRect,
    on_frame: Box<dyn FnMut(ImageBuffer<Rgba<u8>, Vec<u8>>) + Send>,
) -> anyhow::Result<Box<dyn crate::services::screen_capture::LiveFrameStreamHandle>> {
    if selection.width < 2 || selection.height < 2 {
        return Err(anyhow!("selection is too small"));
    }

    let content = SCShareableContent::get()
        .map_err(|error| anyhow!("failed to get shareable content: {error}"))?;
    let displays = content.displays();
    let display = display_for_selection(selection, &displays)
        .ok_or_else(|| anyhow!("no display available for live capture"))?;
    let display_frame = display.frame();
    let filter = SCContentFilter::create()
        .with_display(display)
        .with_excluding_windows(&[])
        .build();
    let point_pixel_scale = f64::from(filter.point_pixel_scale()).max(1.0);
    let source_rect = CGRect::new(
        selection.x as f64 - display_frame.x,
        selection.y as f64 - display_frame.y,
        selection.width as f64,
        selection.height as f64,
    );
    let output_width = (selection.width as f64 * point_pixel_scale)
        .round()
        .max(2.0) as u32;
    let output_height = (selection.height as f64 * point_pixel_scale)
        .round()
        .max(2.0) as u32;
    let frame_interval = CMTime::new(1, 60);
    let config = SCStreamConfiguration::new()
        .with_width(output_width)
        .with_height(output_height)
        .with_source_rect(source_rect)
        .with_pixel_format(PixelFormat::BGRA)
        .with_shows_cursor(false)
        // Generous buffer pool. The callback now decodes each frame to RGBA immediately and releases
        // the CVPixelBuffer, so the pool is not held by the pipeline; a deep pool just guarantees the
        // stream never starves for a free slot and keeps emitting at full rate.
        .with_queue_depth(30)
        .with_minimum_frame_interval(&frame_interval);

    let handler = PushFrameHandler {
        on_frame: Mutex::new(on_frame),
        crop: StreamCrop::entire_output(),
    };
    let output_queue = DispatchQueue::new(
        "io.github.flick-translater.flick.long-capture.stream-push",
        DispatchQoS::UserInteractive,
    );
    let mut stream = SCStream::new(&filter, &config);
    let handler_id = stream
        .add_output_handler_with_queue(handler, SCStreamOutputType::Screen, Some(&output_queue))
        .ok_or_else(|| anyhow!("failed to add ScreenCaptureKit stream output handler"))?;
    stream
        .start_capture()
        .map_err(|error| anyhow!("failed to start ScreenCaptureKit stream: {error}"))?;

    Ok(Box::new(PushStreamHandle {
        stream,
        handler_id: Some(handler_id),
        output_queue,
    }))
}

extern "C" fn dispatch_noop(_context: *mut std::ffi::c_void) {}

fn drain_dispatch_queue(queue: &DispatchQueue) {
    unsafe extern "C" {
        fn dispatch_sync_f(
            queue: *const std::ffi::c_void,
            context: *mut std::ffi::c_void,
            work: extern "C" fn(*mut std::ffi::c_void),
        );
    }

    unsafe {
        dispatch_sync_f(queue.as_ptr(), std::ptr::null_mut(), dispatch_noop);
    }
}

fn capture_image_in_rect(bounds: &SelectionRect) -> anyhow::Result<ScCgImage> {
    let rect = CGRect::new(
        bounds.x as f64,
        bounds.y as f64,
        bounds.width as f64,
        bounds.height as f64,
    );
    SCScreenshotManager::capture_image_in_rect(rect)
        .map_err(|error| anyhow!("ScreenCaptureKit capture failed: {error}"))
}

fn rgba_image_from_sckit(image: ScCgImage) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let width = image.width() as u32;
    let height = image.height() as u32;
    let rgba = image
        .rgba_data()
        .map_err(|error| anyhow!("failed to extract ScreenCaptureKit RGBA data: {error}"))?;
    ImageBuffer::from_vec(width, height, rgba)
        .ok_or_else(|| anyhow!("ScreenCaptureKit RGBA buffer size mismatch"))
}

fn rgba_image_from_pixel_buffer_crop(
    pixel_buffer: &screencapturekit::cv::CVPixelBuffer,
    crop: StreamCrop,
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let width = pixel_buffer.width();
    let height = pixel_buffer.height();
    if width == 0 || height == 0 {
        return Err(anyhow!("empty ScreenCaptureKit pixel buffer"));
    }
    let scale_x = width as f64 / crop.display_width;
    let scale_y = height as f64 / crop.display_height;
    let crop_x = ((crop.selection_x - crop.display_x) * scale_x)
        .round()
        .clamp(0.0, width.saturating_sub(1) as f64) as usize;
    let crop_y = ((crop.selection_y - crop.display_y) * scale_y)
        .round()
        .clamp(0.0, height.saturating_sub(1) as f64) as usize;
    let crop_width = (crop.selection_width * scale_x)
        .round()
        .max(1.0)
        .min((width - crop_x) as f64) as usize;
    let crop_height = (crop.selection_height * scale_y)
        .round()
        .max(1.0)
        .min((height - crop_y) as f64) as usize;

    let guard = pixel_buffer
        .lock(CVPixelBufferLockFlags::READ_ONLY)
        .map_err(|status| anyhow!("failed to lock ScreenCaptureKit pixel buffer: {status}"))?;
    let bytes_per_row = guard.bytes_per_row();
    let raw = guard.as_slice();
    let mut rgba = vec![0_u8; crop_width * crop_height * 4];
    for y in 0..crop_height {
        let src_start = (crop_y + y) * bytes_per_row + crop_x * 4;
        let src_row = &raw[src_start..src_start + crop_width * 4];
        let dst_row = &mut rgba[y * crop_width * 4..(y + 1) * crop_width * 4];
        for (src, dst) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }
    }
    ImageBuffer::from_vec(crop_width as u32, crop_height as u32, rgba)
        .ok_or_else(|| anyhow!("ScreenCaptureKit pixel buffer size mismatch"))
}

fn transfer_to_core_graphics(image: ScCgImage) -> CGImage {
    let ptr = image.as_ptr() as *mut core_graphics::sys::CGImage;
    mem::forget(image);
    unsafe { CGImage::from_ptr(ptr) }
}
