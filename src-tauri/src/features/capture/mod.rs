//! Capture feature composition root.
//!
//! Commands call into this module; the internal split keeps session flow, storage concerns,
//! file IO helpers, and platform branches isolated from one another.

mod history;
mod io;
mod long_capture;
pub(crate) mod platform;
mod recording;
mod recording_gif;
mod recording_video;
mod session;

pub use history::{
    clear_all_captures, clear_all_videos, copy_capture_image, current_screenshot_dir,
    delete_capture, delete_video, get_storage_info, list_capture_history, list_video_history,
    prune_capture_history,
};
pub use io::{open_file_in_default_app, pick_screenshot_directory, read_image_as_data_url};
pub use long_capture::{
    cancel_long_capture, confirm_long_capture, get_long_capture_image,
    open_long_capture_edit_window, prepare_long_capture_edit, save_long_capture,
    scroll_long_capture, start_long_capture, stop_long_capture_scroll,
};
pub use recording::{
    cancel_gif_recording, cancel_recording, close_gif_recording_toolbar_window,
    close_recording_controls_window, finish_gif_recording, finish_recording,
    open_gif_recording_toolbar_window, open_recording_controls_window, pause_gif_recording,
    pause_recording, resume_gif_recording, resume_recording, set_gif_recording_window_shape,
    set_recording_window_mode, start_gif_recording, start_recording,
};
pub(crate) use session::capture_editor_log;
pub use session::{
    begin_capture_session, begin_capture_session_with_intent, cancel_capture,
    cancel_capture_edit_command, capture_editor_frontend_log, capture_editor_ready,
    complete_capture, confirm_regular_capture_edit, get_pending_capture_image,
    save_regular_capture_edit,
};
