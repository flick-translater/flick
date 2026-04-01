//! Capture feature composition root.
//!
//! Commands call into this module; the internal split keeps session flow, storage concerns,
//! file IO helpers, and platform branches isolated from one another.

mod history;
mod io;
mod platform;
mod session;

pub use history::{
    clear_all_captures, copy_capture_image, current_screenshot_dir, delete_capture, get_storage_info,
    list_capture_history, prune_capture_history,
};
pub use io::{open_file_in_default_app, pick_screenshot_directory, read_image_as_data_url};
pub use session::{
    begin_capture_session, begin_capture_session_with_intent, cancel_capture, complete_capture,
};
