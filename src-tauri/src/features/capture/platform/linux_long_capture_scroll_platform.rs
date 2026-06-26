//! Linux long-capture scroll input controller.

use std::sync::{Arc, atomic::AtomicBool};

use crate::{error::FlickError, models::SelectionRect};

use super::{ScrollControllerOptions, ScrollTarget};

pub(super) fn start_scroll_controller(options: ScrollControllerOptions) {
    let _ = options;
}

pub(super) fn start_button_scroll(
    _app: tauri::AppHandle,
    _session_id: String,
    _selection: SelectionRect,
    _target: ScrollTarget,
    _direction: i32,
    _stop: Arc<AtomicBool>,
    _running: Arc<AtomicBool>,
) -> Result<(), FlickError> {
    Ok(())
}
