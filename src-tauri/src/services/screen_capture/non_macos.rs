use anyhow::{Context, anyhow};
use image::{ImageBuffer, Rgba, imageops};

use crate::{models::SelectionRect, services::CachedScreenCapture};

pub fn capture_selection(
    selection: &SelectionRect,
    cached_screens: &[CachedScreenCapture],
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    if selection.width < 2 || selection.height < 2 {
        return Err(anyhow!("selection is too small"));
    }

    let screen = cached_screens
        .iter()
        .find(|screen| {
            let max_x = screen.display_x + screen.display_width as i32;
            let max_y = screen.display_y + screen.display_height as i32;
            selection.x >= screen.display_x
                && selection.y >= screen.display_y
                && selection.x < max_x
                && selection.y < max_y
        })
        .context("failed to find the cached screen for the current selection")?;

    let local_x = selection.x.saturating_sub(screen.display_x);
    let local_y = selection.y.saturating_sub(screen.display_y);

    let available_width = (screen.display_width as i32 - local_x).max(0) as u32;
    let available_height = (screen.display_height as i32 - local_y).max(0) as u32;

    let logical_width = selection.width.min(available_width);
    let logical_height = selection.height.min(available_height);

    if logical_width == 0 || logical_height == 0 {
        return Err(anyhow!("selection is outside of the active display"));
    }

    let scale_x = screen.image.width() as f64 / screen.display_width.max(1) as f64;
    let scale_y = screen.image.height() as f64 / screen.display_height.max(1) as f64;

    let pixel_x = ((local_x as f64) * scale_x).round().max(0.0) as u32;
    let pixel_y = ((local_y as f64) * scale_y).round().max(0.0) as u32;
    let pixel_width = ((logical_width as f64) * scale_x).round().max(1.0) as u32;
    let pixel_height = ((logical_height as f64) * scale_y).round().max(1.0) as u32;

    let bounded_width = pixel_width.min(screen.image.width().saturating_sub(pixel_x));
    let bounded_height = pixel_height.min(screen.image.height().saturating_sub(pixel_y));

    if bounded_width == 0 || bounded_height == 0 {
        return Err(anyhow!("selection is outside of cached screen bounds"));
    }

    Ok(imageops::crop_imm(
        &screen.image,
        pixel_x,
        pixel_y,
        bounded_width,
        bounded_height,
    )
    .to_image())
}
