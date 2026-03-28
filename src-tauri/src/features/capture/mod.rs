mod history;
mod io;
mod platform;
mod session;

pub use history::{
    clear_all_captures, current_screenshot_dir, delete_capture, get_storage_info,
    list_capture_history, prune_capture_history,
};
pub use io::{open_file_in_default_app, pick_screenshot_directory, read_image_as_data_url};
pub use session::{
    begin_capture_session, begin_capture_session_with_intent, cancel_capture, complete_capture,
    focus_capture_window, get_capture_context, get_global_cursor_position, refresh_capture_context,
};
