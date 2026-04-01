//! macOS frozen-desktop capture helpers.
//!
//! The interactive UI works against a static full-desktop snapshot captured at session start,
//! so menus and transient popovers remain visible after Flick shows its own overlay.

use anyhow::{Context, anyhow};
use core_graphics::{
    display::CGDisplay,
    geometry::{CGPoint, CGRect, CGSize},
    image::CGImage,
    window::{
        create_image, kCGNullWindowID, kCGWindowImageDefault, kCGWindowListOptionOnScreenOnly,
    },
};
use image::{ImageBuffer, Rgba, imageops};

use crate::{models::SelectionRect, services::CachedScreenCapture};

use super::capture_desktop_snapshot_via_backend;

pub fn capture_desktop_snapshot(bounds: &SelectionRect) -> anyhow::Result<CachedScreenCapture> {
    capture_desktop_snapshot_via_backend(bounds)
}

pub fn capture_desktop_snapshot_with_core_graphics(
    bounds: &SelectionRect,
) -> anyhow::Result<CachedScreenCapture> {
    if bounds.width < 2 || bounds.height < 2 {
        return Err(anyhow!("desktop bounds are too small"));
    }

    let cg_image = capture_display_image(bounds)
        .or_else(|| capture_window_list_image(bounds))
        .ok_or_else(|| anyhow!("failed to capture full desktop snapshot"))?;

    Ok(CachedScreenCapture::new(bounds.clone(), cg_image))
}

fn capture_display_image(bounds: &SelectionRect) -> Option<core_graphics::image::CGImage> {
    let rect = CGRect::new(
        &CGPoint::new(bounds.x as f64, bounds.y as f64),
        &CGSize::new(bounds.width as f64, bounds.height as f64),
    );
    let display_count = CGDisplay::display_count_with_rect(rect).ok()?;
    let (display_ids, matched) = CGDisplay::displays_with_rect(rect, display_count.max(1)).ok()?;

    for display_id in display_ids.into_iter().take(matched as usize) {
        let display = CGDisplay::new(display_id);
        let display_bounds = display.bounds();
        let display_rect = SelectionRect {
            x: display_bounds.origin.x.round() as i32,
            y: display_bounds.origin.y.round() as i32,
            width: display_bounds.size.width.round() as u32,
            height: display_bounds.size.height.round() as u32,
        };
        if display_rect.x == bounds.x
            && display_rect.y == bounds.y
            && display_rect.width == bounds.width
            && display_rect.height == bounds.height
        {
            return display.image();
        }
    }
    None
}

fn capture_window_list_image(bounds: &SelectionRect) -> Option<core_graphics::image::CGImage> {
    let rect = CGRect::new(
        &CGPoint::new(bounds.x as f64, bounds.y as f64),
        &CGSize::new(bounds.width as f64, bounds.height as f64),
    );

    create_image(
        rect,
        kCGWindowListOptionOnScreenOnly,
        kCGNullWindowID,
        kCGWindowImageDefault,
    )
}

pub fn capture_from_snapshot(
    selection: &SelectionRect,
    cached_screens: &[CachedScreenCapture],
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    if selection.width < 2 || selection.height < 2 {
        return Err(anyhow!("selection is too small"));
    }

    if cached_screens.is_empty() {
        return Err(anyhow!("missing frozen desktop snapshots"));
    }

    let mut pieces = Vec::new();
    let mut output_width = 0_u32;
    let mut output_height = 0_u32;
    let mut had_overlap = false;
    for snapshot in cached_screens {
        let Some(intersection) = intersect_rect(selection, &snapshot.bounds) else {
            continue;
        };
        had_overlap = true;

        let snapshot_bounds = &snapshot.bounds;
        let relative_left = (intersection.x - snapshot_bounds.x) as f64;
        let relative_top = (intersection.y - snapshot_bounds.y) as f64;
        let relative_right = relative_left + intersection.width as f64;
        let relative_bottom = relative_top + intersection.height as f64;

        let scale_x = snapshot.image.0.width() as f64 / snapshot_bounds.width as f64;
        let scale_y = snapshot.image.0.height() as f64 / snapshot_bounds.height as f64;

        let left = (relative_left * scale_x).floor().max(0.0) as u32;
        let top = (relative_top * scale_y).floor().max(0.0) as u32;
        let right = (relative_right * scale_x)
            .ceil()
            .min(snapshot.image.0.width() as f64) as u32;
        let bottom = (relative_bottom * scale_y)
            .ceil()
            .min(snapshot.image.0.height() as f64) as u32;

        let width = right.saturating_sub(left);
        let height = bottom.saturating_sub(top);
        let dest_left = (((intersection.x - selection.x) as f64) * scale_x)
            .round()
            .max(0.0) as u32;
        let dest_top = (((intersection.y - selection.y) as f64) * scale_y)
            .round()
            .max(0.0) as u32;
        if width == 0 || height == 0 {
            continue;
        }

        output_width = output_width.max(dest_left.saturating_add(width));
        output_height = output_height.max(dest_top.saturating_add(height));
        pieces.push((snapshot, left, top, width, height, dest_left, dest_top));
    }

    if !had_overlap {
        return Err(anyhow!("selection is outside frozen desktop snapshots"));
    }

    let mut output = ImageBuffer::from_pixel(output_width, output_height, Rgba([0, 0, 0, 0]));
    for (snapshot, left, top, width, height, dest_left, dest_top) in pieces {
        let cropped = crop_snapshot_to_rgba(&snapshot.image.0, left, top, width, height)?;
        imageops::replace(
            &mut output,
            &cropped,
            i64::from(dest_left),
            i64::from(dest_top),
        );
    }

    Ok(output)
}
fn crop_snapshot_to_rgba(
    image: &CGImage,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let bytes_per_row = image.bytes_per_row();
    let image_width = image.width();
    let image_height = image.height();
    let row_bytes = usize::try_from(width)
        .context("invalid crop width")?
        .saturating_mul(4);
    let left_px = usize::try_from(left).context("invalid crop left")?;
    let top_px = usize::try_from(top).context("invalid crop top")?;
    let width_px = usize::try_from(width).context("invalid crop width")?;
    let height_px = usize::try_from(height).context("invalid crop height")?;

    if left_px.saturating_add(width_px) > image_width
        || top_px.saturating_add(height_px) > image_height
    {
        return Err(anyhow!("cropped region is outside frozen snapshot bounds"));
    }

    let data = image.data();
    let bytes = data.bytes();
    let required_len = bytes_per_row.saturating_mul(image_height);
    if bytes.len() < required_len {
        return Err(anyhow!(
            "frozen snapshot buffer too small: len={} required={}",
            bytes.len(),
            required_len
        ));
    }

    let mut cropped_buf = Vec::with_capacity(row_bytes.saturating_mul(height_px));
    for row in 0..height_px {
        let src_row = top_px + row;
        let start = src_row
            .saturating_mul(bytes_per_row)
            .saturating_add(left_px.saturating_mul(4));
        let end = start.saturating_add(row_bytes);
        if end > bytes.len() {
            return Err(anyhow!(
                "cropped row is outside frozen snapshot buffer: start={} end={} len={}",
                start,
                end,
                bytes.len()
            ));
        }
        cropped_buf.extend_from_slice(&bytes[start..end]);
    }

    bgra_to_rgba_image(width, height, cropped_buf)
        .context("failed to convert cropped snapshot to RGBA")
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

fn bgra_to_rgba_image(
    width: u32,
    height: u32,
    buf: Vec<u8>,
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let mut rgba_buf = buf.clone();

    for (src, dst) in buf.chunks_exact(4).zip(rgba_buf.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }

    ImageBuffer::from_vec(width, height, rgba_buf).ok_or_else(|| anyhow!("buffer not big enough"))
}
