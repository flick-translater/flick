//! File-system and shell helpers related to captured images.

use std::{fs, path::Path, process::Command};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::error::FlickError;

pub fn pick_screenshot_directory() -> Result<Option<String>, FlickError> {
    Ok(rfd::FileDialog::new()
        .set_title("Select Screenshot Directory")
        .pick_folder()
        .map(|path| path.display().to_string()))
}

pub fn open_file_in_default_app(path: &str) -> Result<(), FlickError> {
    if !Path::new(path).exists() {
        return Err(FlickError::Message("file does not exist".into()));
    }

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(path);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", path]);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(path);
        command
    };

    command
        .spawn()
        .map_err(|error| FlickError::Message(format!("failed to open file: {error}")))?;

    Ok(())
}

pub fn read_image_as_data_url(path: &str) -> Result<String, FlickError> {
    let bytes = fs::read(path)
        .map_err(|error| FlickError::Message(format!("failed to read image: {error}")))?;

    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}
