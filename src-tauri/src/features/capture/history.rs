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

pub fn get_storage_info(state: &State<'_, AppState>) -> Result<StorageInfo, FlickError> {
    let screenshot_dir = current_screenshot_dir(state)?;
    Ok(StorageInfo {
        data_dir: state.data_dir.display().to_string(),
        screenshot_dir: screenshot_dir.display().to_string(),
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

pub fn copy_capture_image(path: &str) -> Result<(), FlickError> {
    let image = image::open(path)
        .map_err(|error| FlickError::Message(format!("failed to read screenshot: {error}")))?
        .into_rgba8();
    ScreenCaptureService
        .copy_to_clipboard(&image)
        .map_err(|error| FlickError::Message(format!("failed to copy screenshot image: {error}")))?;

    Ok(())
}

pub fn current_screenshot_dir(state: &State<'_, AppState>) -> Result<PathBuf, FlickError> {
    state
        .screenshot_dir
        .lock()
        .map_err(|_| FlickError::Message("screenshot dir mutex poisoned".into()))
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

        if !matches!(path.extension().and_then(|ext| ext.to_str()), Some("png")) {
            continue;
        }

        let metadata = entry.metadata().map_err(|error| {
            FlickError::Message(format!("failed to read screenshot metadata: {error}"))
        })?;
        if !metadata.is_file() {
            continue;
        }

        let (width, height) = image::image_dimensions(&path).map_err(|error| {
            FlickError::Message(format!("failed to read screenshot dimensions: {error}"))
        })?;
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
