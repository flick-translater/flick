//! Linux long-capture scroll input controller.

use crate::features::capture::long_capture::long_log;

use super::ScrollControllerOptions;

pub(super) fn start_scroll_controller(options: ScrollControllerOptions) {
    let _ = options;
    long_log(
        "scroll_controller/linux: not implemented yet; X11 can use XI2/XTest, Wayland is unsupported",
    );
}
