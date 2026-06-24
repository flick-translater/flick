//! Windows long-capture scroll input controller.

use std::sync::{Arc, atomic::AtomicBool};

use crate::{error::FlickError, features::capture::long_capture::long_log, models::SelectionRect};

use super::{ScrollControllerOptions, ScrollTarget};

pub(super) fn start_scroll_controller(options: ScrollControllerOptions) {
    let _ = options;
    long_log(
        "scroll_controller/windows: not implemented yet; intended backend is WH_MOUSE_LL + SendInput",
    );
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
    long_log("scroll_controller/windows: button scroll loop is not implemented yet");
    Ok(())
}
