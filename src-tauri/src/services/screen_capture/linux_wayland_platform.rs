use anyhow::{Context, anyhow};
use image::{ImageBuffer, Rgba};

use crate::models::SelectionRect;

use super::CachedScreenCapture;

pub fn capture_selection(
    selection: &SelectionRect,
    cached_screens: &[CachedScreenCapture],
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let snapshot = cached_screens
        .first()
        .ok_or_else(|| anyhow!("missing Wayland portal snapshot"))?;
    let bounds = &snapshot.bounds;

    let relative_x = selection
        .x
        .checked_sub(bounds.x)
        .ok_or_else(|| anyhow!("selection starts outside portal snapshot"))?;
    let relative_y = selection
        .y
        .checked_sub(bounds.y)
        .ok_or_else(|| anyhow!("selection starts outside portal snapshot"))?;
    let right = relative_x
        .checked_add(selection.width as i32)
        .ok_or_else(|| anyhow!("invalid portal selection width"))?;
    let bottom = relative_y
        .checked_add(selection.height as i32)
        .ok_or_else(|| anyhow!("invalid portal selection height"))?;

    if right > bounds.width as i32 || bottom > bounds.height as i32 {
        anyhow::bail!("selection exceeds portal snapshot bounds");
    }

    let cropped = image::imageops::crop_imm(
        snapshot.image.as_ref(),
        relative_x as u32,
        relative_y as u32,
        selection.width,
        selection.height,
    )
    .to_image();

    ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
        cropped.width(),
        cropped.height(),
        cropped.into_raw(),
    )
    .context("failed to build cropped Wayland image")
}
