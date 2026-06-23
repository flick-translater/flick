//! Capture feature composition root.
//!
//! Commands call into this module; the internal split keeps session flow, storage concerns,
//! file IO helpers, and platform branches isolated from one another.

mod history;
mod io;
mod long_capture;
pub(crate) mod platform;
mod session;

pub use history::{
    clear_all_captures, copy_capture_image, current_screenshot_dir, delete_capture,
    get_storage_info, list_capture_history, prune_capture_history,
};
pub use io::{open_file_in_default_app, pick_screenshot_directory, read_image_as_data_url};
pub use long_capture::{
    cancel_long_capture, confirm_long_capture, get_long_capture_image,
    open_long_capture_edit_window, prepare_long_capture_edit, save_long_capture,
    start_long_capture,
};
pub(crate) use session::capture_editor_log;
pub use session::{
    begin_capture_session, begin_capture_session_with_intent, cancel_capture,
    cancel_capture_edit_command, capture_editor_frontend_log, capture_editor_ready,
    complete_capture, confirm_regular_capture_edit, get_pending_capture_image,
    save_regular_capture_edit,
};
