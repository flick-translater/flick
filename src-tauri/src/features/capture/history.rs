//! Capture history and screenshot storage management.

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use chrono::{DateTime, Utc};
use tauri::State;
use uuid::Uuid;

use crate::{
    app::AppState,
    error::FlickError,
    models::{CaptureHistory, CaptureRecord, StorageInfo},
    services::ScreenCaptureService,
};

pub fn list_capture_history(state: &State<'_, AppState>) -> Result<CaptureHistory, FlickError> {
    let max_screenshots = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .max_screenshots;
    let screenshot_dir = current_screenshot_dir(state)?;

    Ok(CaptureHistory {
        directory: screenshot_dir.display().to_string(),
        items: prune_capture_history(&screenshot_dir, max_screenshots)?,
    })
}

pub fn list_video_history(state: &State<'_, AppState>) -> Result<CaptureHistory, FlickError> {
    let video_dir = current_video_dir(state)?;

    Ok(CaptureHistory {
        directory: video_dir.display().to_string(),
        items: load_video_history(&video_dir)?,
    })
}

pub fn get_storage_info(state: &State<'_, AppState>) -> Result<StorageInfo, FlickError> {
    let screenshot_dir = current_screenshot_dir(state)?;
    let video_dir = current_video_dir(state)?;
    Ok(StorageInfo {
        data_dir: state.data_dir.display().to_string(),
        screenshot_dir: screenshot_dir.display().to_string(),
        video_dir: video_dir.display().to_string(),
    })
}

pub fn delete_capture(state: &State<'_, AppState>, path: &str) -> Result<(), FlickError> {
    let capture_path = Path::new(path);
    let screenshot_dir = current_screenshot_dir(state)?;

    if !capture_path.starts_with(&screenshot_dir) {
        return Err(FlickError::Message(
            "capture path is outside screenshot directory".into(),
        ));
    }

    if !capture_path.exists() {
        return Ok(());
    }

    fs::remove_file(capture_path)
        .map_err(|error| FlickError::Message(format!("failed to delete capture: {error}")))?;

    if let Ok(mut history) = state.history.lock() {
        history.retain(|record| record.path != path);
    }

    Ok(())
}

pub fn clear_all_captures(state: &State<'_, AppState>) -> Result<(), FlickError> {
    let screenshot_dir = current_screenshot_dir(state)?;
    let records = load_capture_history(&screenshot_dir)?;

    for record in records {
        let capture_path = Path::new(&record.path);
        if capture_path.starts_with(&screenshot_dir) && capture_path.exists() {
            fs::remove_file(capture_path).map_err(|error| {
                FlickError::Message(format!("failed to delete capture: {error}"))
            })?;
        }
    }

    if let Ok(mut history) = state.history.lock() {
        history.clear();
    }

    Ok(())
}

pub fn delete_video(state: &State<'_, AppState>, path: &str) -> Result<(), FlickError> {
    let video_path = Path::new(path);
    let video_dir = current_video_dir(state)?;

    if !video_path.starts_with(&video_dir) {
        return Err(FlickError::Message(
            "video path is outside video directory".into(),
        ));
    }

    if video_path.exists() {
        fs::remove_file(video_path)
            .map_err(|error| FlickError::Message(format!("failed to delete video: {error}")))?;
    }

    Ok(())
}

pub fn clear_all_videos(state: &State<'_, AppState>) -> Result<(), FlickError> {
    let video_dir = current_video_dir(state)?;
    let records = load_video_history(&video_dir)?;

    for record in records {
        let video_path = Path::new(&record.path);
        if video_path.starts_with(&video_dir) && video_path.exists() {
            fs::remove_file(video_path)
                .map_err(|error| FlickError::Message(format!("failed to delete video: {error}")))?;
        }
    }

    Ok(())
}

pub fn copy_capture_image(path: &str) -> Result<(), FlickError> {
    if path
        .rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("gif"))
    {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|error| FlickError::Message(format!("failed to access clipboard: {error}")))?;
        if let Err(file_error) = clipboard.set().file_list(&[Path::new(path)]) {
            clipboard.set_text(path.to_string()).map_err(|error| {
                FlickError::Message(format!(
                    "failed to copy gif file to clipboard: {file_error}; text fallback failed: {error}"
                ))
            })?;
        }
        return Ok(());
    }

    let image = image::open(path)
        .map_err(|error| FlickError::Message(format!("failed to read screenshot: {error}")))?
        .into_rgba8();
    ScreenCaptureService
        .copy_to_clipboard(&image)
        .map_err(|error| {
            FlickError::Message(format!("failed to copy screenshot image: {error}"))
        })?;

    Ok(())
}

pub fn current_screenshot_dir(state: &State<'_, AppState>) -> Result<PathBuf, FlickError> {
    state
        .screenshot_dir
        .lock()
        .map_err(|_| FlickError::Message("screenshot dir mutex poisoned".into()))
        .map(|path| path.clone())
}

pub fn current_video_dir(state: &State<'_, AppState>) -> Result<PathBuf, FlickError> {
    state
        .video_dir
        .lock()
        .map_err(|_| FlickError::Message("video dir mutex poisoned".into()))
        .map(|path| path.clone())
}

pub fn prune_capture_history(
    screenshot_dir: &Path,
    max_screenshots: u32,
) -> Result<Vec<CaptureRecord>, FlickError> {
    // Storage is bounded eagerly so the screenshot directory cannot grow without limit.
    let records = load_capture_history(screenshot_dir)?;
    let keep_count = max_screenshots.max(1) as usize;

    for record in records.iter().skip(keep_count) {
        fs::remove_file(&record.path).map_err(|error| {
            FlickError::Message(format!("failed to remove old screenshot: {error}"))
        })?;
    }

    Ok(records.into_iter().take(keep_count).collect())
}

fn load_capture_history(screenshot_dir: &Path) -> Result<Vec<CaptureRecord>, FlickError> {
    let mut records = Vec::new();
    let entries = fs::read_dir(screenshot_dir)
        .map_err(|error| FlickError::Message(format!("failed to read screenshot dir: {error}")))?;

    for entry in entries {
        let entry = entry.map_err(|error| {
            FlickError::Message(format!("failed to read screenshot entry: {error}"))
        })?;
        let path = entry.path();

        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if !ext.eq_ignore_ascii_case("png") && !ext.eq_ignore_ascii_case("gif") {
            continue;
        }

        let metadata = entry.metadata().map_err(|error| {
            FlickError::Message(format!("failed to read screenshot metadata: {error}"))
        })?;
        if !metadata.is_file() {
            continue;
        }

        let (width, height) = capture_dimensions(&path)?;
        let created_at = metadata
            .modified()
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(|_| DateTime::<Utc>::from(SystemTime::UNIX_EPOCH));
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        records.push(CaptureRecord {
            id,
            created_at,
            width,
            height,
            path: path.display().to_string(),
        });
    }

    records.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(records)
}

fn load_video_history(video_dir: &Path) -> Result<Vec<CaptureRecord>, FlickError> {
    if !video_dir.exists() {
        return Ok(Vec::new());
    }

    let mut records = Vec::new();
    let entries = fs::read_dir(video_dir)
        .map_err(|error| FlickError::Message(format!("failed to read video dir: {error}")))?;

    for entry in entries {
        let entry = entry
            .map_err(|error| FlickError::Message(format!("failed to read video entry: {error}")))?;
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if !ext.eq_ignore_ascii_case("mp4") {
            continue;
        }

        let metadata = entry.metadata().map_err(|error| {
            FlickError::Message(format!("failed to read video metadata: {error}"))
        })?;
        if !metadata.is_file() {
            continue;
        }

        let created_at = metadata
            .modified()
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(|_| DateTime::<Utc>::from(SystemTime::UNIX_EPOCH));
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        records.push(CaptureRecord {
            id,
            created_at,
            width: 0,
            height: 0,
            path: path.display().to_string(),
        });
    }

    records.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(records)
}

fn capture_dimensions(path: &Path) -> Result<(u32, u32), FlickError> {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gif"))
    {
        let file = fs::File::open(path)
            .map_err(|error| FlickError::Message(format!("failed to open gif: {error}")))?;
        let decoder = gif::DecodeOptions::new().read_info(file).map_err(|error| {
            FlickError::Message(format!("failed to read gif dimensions: {error}"))
        })?;
        return Ok((u32::from(decoder.width()), u32::from(decoder.height())));
    }

    image::image_dimensions(path).map_err(|error| {
        FlickError::Message(format!("failed to read screenshot dimensions: {error}"))
    })
}
