//! File-system and shell helpers related to captured images.

use std::{fs, path::Path};

#[cfg(not(target_os = "windows"))]
use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;

#[cfg(target_os = "windows")]
use windows_sys::{
    Win32::{
        Foundation::HWND,
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    },
    core::PCWSTR,
};

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
    return open_file_with_shell_execute(path);

    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(target_os = "linux")]
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
}

#[cfg(target_os = "windows")]
fn open_file_with_shell_execute(path: &str) -> Result<(), FlickError> {
    let wide_path = wide_null(Path::new(path).as_os_str());
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut::<std::ffi::c_void>() as HWND,
            std::ptr::null::<u16>() as PCWSTR,
            wide_path.as_ptr(),
            std::ptr::null::<u16>() as PCWSTR,
            std::ptr::null::<u16>() as PCWSTR,
            SW_SHOWNORMAL,
        )
    } as isize;

    if result <= 32 {
        return Err(FlickError::Message(format!(
            "failed to open file with default app: ShellExecuteW returned {result}"
        )));
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn wide_null(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn read_image_as_data_url(path: &str) -> Result<String, FlickError> {
    let bytes = fs::read(path)
        .map_err(|error| FlickError::Message(format!("failed to read image: {error}")))?;
    let mime = match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("gif") => "image/gif",
        Some(ext) if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") => {
            "image/jpeg"
        }
        _ => "image/png",
    };

    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}
