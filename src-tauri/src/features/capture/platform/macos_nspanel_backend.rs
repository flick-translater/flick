use tauri::AppHandle;

use crate::{error::FlickError, models::SelectionRect};

use super::CursorPosition;

pub(super) fn show_native_overlay(app: &AppHandle) -> Result<(), FlickError> {
    super::panel_show_native_overlay(app)
}

pub(super) fn hide_native_overlay(app: &AppHandle) -> Result<(), FlickError> {
    super::panel_hide_native_overlay(app)
}

pub(super) fn update_highlight(
    app: &AppHandle,
    selection: Option<SelectionRect>,
) -> Result<(), FlickError> {
    super::panel_update_highlight(app, selection)
}

pub(super) fn update_crosshair(
    app: &AppHandle,
    cursor: &CursorPosition,
) -> Result<(), FlickError> {
    super::panel_update_crosshair(app, cursor)
}

pub(super) fn hide_crosshair(app: &AppHandle) -> Result<(), FlickError> {
    super::panel_hide_crosshair(app)
}
