use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

use crate::{error::FlickError, models::FfmpegStatus};

const FFMPEG_RELEASE_BASE: &str =
    "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1";

pub fn detect_ffmpeg(configured_path: &str) -> FfmpegStatus {
    if !configured_path.trim().is_empty() && ffmpeg_works(Path::new(configured_path)) {
        return FfmpegStatus {
            available: true,
            path: configured_path.to_string(),
            source: "configured".into(),
        };
    }

    if command_works("ffmpeg") {
        return FfmpegStatus {
            available: true,
            path: "ffmpeg".into(),
            source: "system".into(),
        };
    }

    let local_path = local_ffmpeg_path();
    if ffmpeg_works(&local_path) {
        return FfmpegStatus {
            available: true,
            path: local_path.display().to_string(),
            source: "local".into(),
        };
    }

    FfmpegStatus {
        available: false,
        path: String::new(),
        source: "missing".into(),
    }
}

pub async fn download_ffmpeg() -> Result<FfmpegStatus, FlickError> {
    let url = ffmpeg_download_url()?;
    let destination = local_ffmpeg_path();
    let temp_path = destination.with_extension("download");
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| FlickError::Message(format!("failed to create lib dir: {error}")))?;
    }

    let response = reqwest::get(url)
        .await
        .map_err(|error| FlickError::Message(format!("failed to download ffmpeg: {error}")))?;
    if !response.status().is_success() {
        return Err(FlickError::Message(format!(
            "failed to download ffmpeg: HTTP {}",
            response.status()
        )));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|error| FlickError::Message(format!("failed to read ffmpeg download: {error}")))?;
    fs::write(&temp_path, bytes)
        .map_err(|error| FlickError::Message(format!("failed to write ffmpeg file: {error}")))?;

    fs::rename(&temp_path, &destination)
        .map_err(|error| FlickError::Message(format!("failed to install ffmpeg: {error}")))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&destination)
            .map_err(|error| {
                FlickError::Message(format!("failed to read ffmpeg metadata: {error}"))
            })?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&destination, permissions).map_err(|error| {
            FlickError::Message(format!("failed to mark ffmpeg executable: {error}"))
        })?;
    }

    if !ffmpeg_works(&destination) {
        return Err(FlickError::Message(
            "downloaded ffmpeg is not executable on this system".into(),
        ));
    }

    Ok(FfmpegStatus {
        available: true,
        path: destination.display().to_string(),
        source: "local".into(),
    })
}

fn local_ffmpeg_path() -> PathBuf {
    let name = if cfg!(target_os = "windows") {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    program_lib_dir().join(name)
}

fn program_lib_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("lib")
}

fn ffmpeg_download_url() -> Result<String, FlickError> {
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-arm64",
        ("macos", _) => "darwin-x64",
        ("windows", _) => "win32-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("linux", "arm") => "linux-arm",
        ("linux", "x86") => "linux-ia32",
        ("linux", _) => "linux-x64",
        _ => {
            return Err(FlickError::Message(
                "automatic ffmpeg download is not supported on this platform".into(),
            ));
        }
    };
    Ok(format!("{FFMPEG_RELEASE_BASE}/ffmpeg-{platform}"))
}

fn command_works(command: &str) -> bool {
    ffmpeg_version_output(&mut Command::new(command)).is_ok_and(|output| output.status.success())
}

fn ffmpeg_works(path: &Path) -> bool {
    path.is_file()
        && ffmpeg_version_output(&mut Command::new(path))
            .is_ok_and(|output| output.status.success())
}

fn ffmpeg_version_output(command: &mut Command) -> std::io::Result<Output> {
    command.arg("-version");
    hide_command_window(command);
    command.output()
}

fn hide_command_window(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
}
