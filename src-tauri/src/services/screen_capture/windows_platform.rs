use anyhow::{Context, anyhow};
use image::{ImageBuffer, Rgba, imageops};

use crate::models::SelectionRect;

use super::CachedScreenCapture;

pub fn capture_selection(
    selection: &SelectionRect,
    cached_screens: &[CachedScreenCapture],
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
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
