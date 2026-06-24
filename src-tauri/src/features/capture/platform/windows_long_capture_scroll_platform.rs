//! Windows long-capture scroll input controller.

use crate::features::capture::long_capture::long_log;

use super::ScrollControllerOptions;

pub(super) fn start_scroll_controller(options: ScrollControllerOptions) {
    let _ = options;
    long_log(
        "scroll_controller/windows: not implemented yet; intended backend is WH_MOUSE_LL + SendInput",
    );
}
