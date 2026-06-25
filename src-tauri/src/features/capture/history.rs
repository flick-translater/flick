//! Capture history and screenshot storage management.

use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Command,
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

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn list_capture_history(state: &State<'_, AppState>) -> Result<CaptureHistory, FlickError> {
    let max_screenshots = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .max_screenshots;
    let screenshot_dir = current_screenshot_dir(state)?;
    let items = prune_capture_history(&screenshot_dir, max_screenshots)?;
    let total_count = items.len();

    Ok(CaptureHistory {
        directory: screenshot_dir.display().to_string(),
        total_count,
        items,
    })
}

pub fn list_capture_history_page(
    state: &State<'_, AppState>,
    page: u32,
    page_size: u32,
) -> Result<CaptureHistory, FlickError> {
    let max_screenshots = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .max_screenshots;
    let screenshot_dir = current_screenshot_dir(state)?;
    let records = prune_capture_history_page(&screenshot_dir, max_screenshots, page, page_size)?;

    Ok(CaptureHistory {
        directory: screenshot_dir.display().to_string(),
        total_count: records.total_count,
        items: records.items,
    })
}

pub fn list_video_history(state: &State<'_, AppState>) -> Result<CaptureHistory, FlickError> {
    let video_dir = current_video_dir(state)?;
    let items = load_video_history(&video_dir)?;
    let _ = remove_orphan_video_thumbnail_cache(&video_dir, &items);
    let total_count = items.len();

    Ok(CaptureHistory {
        directory: video_dir.display().to_string(),
        total_count,
        items,
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
    remove_video_thumbnail_cache(&video_dir, video_path)?;

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
    clear_video_thumbnail_cache(&video_dir)?;

    Ok(())
}

pub fn read_video_thumbnail_as_data_url(
    state: &State<'_, AppState>,
    path: &str,
) -> Result<String, FlickError> {
    let video_path = Path::new(path);
    let video_dir = current_video_dir(state)?;

    if !video_path.starts_with(&video_dir) {
        return Err(FlickError::Message(
            "video path is outside video directory".into(),
        ));
    }
    if !video_path.exists() {
        remove_video_thumbnail_cache(&video_dir, video_path)?;
        return Err(FlickError::Message("video file does not exist".into()));
    }

    let cached_thumbnail_path = video_thumbnail_path(&video_dir, video_path)?;
    if cached_thumbnail_path.exists() {
        return crate::features::capture::read_image_as_data_url(
            cached_thumbnail_path
                .to_str()
                .ok_or_else(|| FlickError::Message("thumbnail path is not valid UTF-8".into()))?,
        );
    }

    let ffmpeg_status = state
        .ffmpeg_status
        .lock()
        .map_err(|_| FlickError::Message("ffmpeg status mutex poisoned".into()))?
        .clone();
    if !ffmpeg_status.available {
        return Err(FlickError::Message("ffmpeg is not available".into()));
    }

    let thumbnail = ensure_video_thumbnail(&video_dir, video_path, &ffmpeg_status.path)?;
    crate::features::capture::read_image_as_data_url(
        thumbnail
            .path
            .to_str()
            .ok_or_else(|| FlickError::Message("thumbnail path is not valid UTF-8".into()))?,
    )
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

struct PagedCaptureRecords {
    total_count: usize,
    items: Vec<CaptureRecord>,
}

fn prune_capture_history_page(
    screenshot_dir: &Path,
    max_screenshots: u32,
    page: u32,
    page_size: u32,
) -> Result<PagedCaptureRecords, FlickError> {
    // Read only lightweight file information for all screenshots, then decode dimensions for the
    // visible page. This keeps pagination accurate without opening every image on each page load.
    let records = load_capture_file_entries(screenshot_dir)?;
    let keep_count = max_screenshots.max(1) as usize;

    for record in records.iter().skip(keep_count) {
        fs::remove_file(&record.path).map_err(|error| {
            FlickError::Message(format!("failed to remove old screenshot: {error}"))
        })?;
    }

    let total_count = records.len().min(keep_count);
    let page = page.max(1) as usize;
    let page_size = page_size.clamp(1, 100) as usize;
    let start = page.saturating_sub(1).saturating_mul(page_size);
    let items = records
        .into_iter()
        .take(keep_count)
        .skip(start)
        .take(page_size)
        .map(|entry| entry.into_capture_record())
        .collect::<Result<Vec<_>, _>>()?;

    Ok(PagedCaptureRecords { total_count, items })
}

fn load_capture_history(screenshot_dir: &Path) -> Result<Vec<CaptureRecord>, FlickError> {
    load_capture_file_entries(screenshot_dir)?
        .into_iter()
        .map(|entry| entry.into_capture_record())
        .collect()
}

struct CaptureFileEntry {
    id: String,
    created_at: DateTime<Utc>,
    path: PathBuf,
}

impl CaptureFileEntry {
    fn into_capture_record(self) -> Result<CaptureRecord, FlickError> {
        let (width, height) = capture_dimensions(&self.path)?;
        Ok(CaptureRecord {
            id: self.id,
            created_at: self.created_at,
            width,
            height,
            path: self.path.display().to_string(),
        })
    }
}

fn load_capture_file_entries(screenshot_dir: &Path) -> Result<Vec<CaptureFileEntry>, FlickError> {
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

        let created_at = metadata
            .modified()
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(|_| DateTime::<Utc>::from(SystemTime::UNIX_EPOCH));
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        records.push(CaptureFileEntry {
            id,
            created_at,
            path,
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

struct VideoThumbnail {
    path: PathBuf,
}

fn ensure_video_thumbnail(
    video_dir: &Path,
    video_path: &Path,
    ffmpeg_path: &str,
) -> Result<VideoThumbnail, FlickError> {
    let thumbnail_path = video_thumbnail_path(video_dir, video_path)?;
    if thumbnail_path.exists() {
        return Ok(VideoThumbnail {
            path: thumbnail_path,
        });
    }

    if let Some(parent) = thumbnail_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            FlickError::Message(format!("failed to create video thumbnail cache: {error}"))
        })?;
    }
    remove_video_thumbnail_cache(video_dir, video_path)?;

    let temp_suffix = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_path = thumbnail_path.with_file_name(format!(
        ".{}.{}.tmp.jpg",
        thumbnail_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("thumbnail"),
        temp_suffix
    ));
    let _ = fs::remove_file(&temp_path);
    let mut command = Command::new(ffmpeg_path);
    command
        .arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-ss")
        .arg("00:00:00.2")
        .arg("-i")
        .arg(video_path)
        .arg("-frames:v")
        .arg("1")
        .arg("-vf")
        .arg("scale=320:-1")
        .arg(&temp_path);
    hide_command_window(&mut command);
    let output = command.output().map_err(|error| {
        FlickError::Message(format!("failed to generate video thumbnail: {error}"))
    })?;
    if !output.status.success() {
        let _ = fs::remove_file(&temp_path);
        return Err(FlickError::Message(format!(
            "failed to generate video thumbnail: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    if let Err(error) = fs::rename(&temp_path, &thumbnail_path) {
        let _ = fs::remove_file(&temp_path);
        if thumbnail_path.exists() {
            return Ok(VideoThumbnail {
                path: thumbnail_path,
            });
        }
        return Err(FlickError::Message(format!(
            "failed to save video thumbnail: {error}"
        )));
    }
    Ok(VideoThumbnail {
        path: thumbnail_path,
    })
}

fn video_thumbnail_path(video_dir: &Path, video_path: &Path) -> Result<PathBuf, FlickError> {
    let metadata = video_path
        .metadata()
        .map_err(|error| FlickError::Message(format!("failed to read video metadata: {error}")))?;
    let modified = metadata
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let file_stem = video_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(sanitize_cache_name)
        .unwrap_or_else(|| "video".into());
    Ok(video_thumbnail_dir(video_dir).join(format!(
        "{}-{}-{}-{}.jpg",
        file_stem,
        video_path_hash(video_path),
        modified.as_secs(),
        metadata.len()
    )))
}

fn video_thumbnail_dir(video_dir: &Path) -> PathBuf {
    video_dir.join(".thumbnails")
}

fn remove_video_thumbnail_cache(video_dir: &Path, video_path: &Path) -> Result<(), FlickError> {
    let cache_dir = video_thumbnail_dir(video_dir);
    if !cache_dir.exists() {
        return Ok(());
    }

    let hash = video_path_hash(video_path);
    let entries = fs::read_dir(&cache_dir).map_err(|error| {
        FlickError::Message(format!("failed to read video thumbnail cache: {error}"))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            FlickError::Message(format!(
                "failed to read video thumbnail cache entry: {error}"
            ))
        })?;
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(&hash))
        {
            fs::remove_file(&path).map_err(|error| {
                FlickError::Message(format!("failed to remove video thumbnail cache: {error}"))
            })?;
        }
    }
    Ok(())
}

fn clear_video_thumbnail_cache(video_dir: &Path) -> Result<(), FlickError> {
    let cache_dir = video_thumbnail_dir(video_dir);
    if cache_dir.exists() {
        fs::remove_dir_all(&cache_dir).map_err(|error| {
            FlickError::Message(format!("failed to clear video thumbnail cache: {error}"))
        })?;
    }
    Ok(())
}

fn remove_orphan_video_thumbnail_cache(
    video_dir: &Path,
    records: &[CaptureRecord],
) -> Result<(), FlickError> {
    let cache_dir = video_thumbnail_dir(video_dir);
    if !cache_dir.exists() {
        return Ok(());
    }

    let active_hashes = records
        .iter()
        .map(|record| video_path_hash(Path::new(&record.path)))
        .collect::<Vec<_>>();
    let entries = fs::read_dir(&cache_dir).map_err(|error| {
        FlickError::Message(format!("failed to read video thumbnail cache: {error}"))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            FlickError::Message(format!(
                "failed to read video thumbnail cache entry: {error}"
            ))
        })?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !active_hashes.iter().any(|hash| file_name.contains(hash)) {
            fs::remove_file(&path).map_err(|error| {
                FlickError::Message(format!("failed to remove orphan video thumbnail: {error}"))
            })?;
        }
    }
    Ok(())
}

fn sanitize_cache_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn video_path_hash(path: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn hide_command_window(_command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        _command.creation_flags(CREATE_NO_WINDOW);
    }
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
