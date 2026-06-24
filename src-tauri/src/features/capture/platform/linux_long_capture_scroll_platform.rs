//! Linux long-capture scroll input controller.

use std::sync::{Arc, atomic::AtomicBool};

use crate::{error::FlickError, features::capture::long_capture::long_log, models::SelectionRect};

use super::{ScrollControllerOptions, ScrollTarget};

pub(super) fn start_scroll_controller(options: ScrollControllerOptions) {
    let _ = options;
    long_log(
        "scroll_controller/linux: not implemented yet; X11 can use XI2/XTest, Wayland is unsupported",
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
    long_log("scroll_controller/linux: button scroll loop is not implemented yet");
    Ok(())
}
