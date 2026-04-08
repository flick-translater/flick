use std::{fs, path::Path};

use anyhow::{Context, anyhow};
use image::{GenericImageView, ImageFormat, imageops::FilterType};
use uuid::Uuid;
use windows::{
    Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat},
    Media::Ocr::OcrEngine,
    Storage::StorageFile,
    core::{HSTRING, RuntimeType},
};
use windows_future::{AsyncStatus, IAsyncOperation};
use windows_sys::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

use super::OcrService;
use crate::models::{OcrResponse, OcrTextBlock};

pub struct WindowsBuiltinOcrService;

impl OcrService for WindowsBuiltinOcrService {
    fn run_with_data(&self, image_data: &[u8]) -> anyhow::Result<OcrResponse> {
        let mut payload = run_windows_builtin_ocr_from_bytes(image_data)?;
        if payload.text.trim().is_empty() && payload.blocks.is_empty() {
            if let Some(enhanced_bytes) = build_retry_image_bytes(image_data) {
                payload = run_windows_builtin_ocr_from_bytes(&enhanced_bytes)?;
            }
        }
        let mut blocks = payload.blocks;

        if blocks.is_empty() && !payload.text.trim().is_empty() {
            blocks.push(OcrTextBlock {
                text: payload.text.trim().to_string(),
                confidence: 1.0,
            });
        }

        let text = if payload.text.trim().is_empty() {
            blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            payload.text
        };

        Ok(OcrResponse {
            provider: "windows".into(),
            text,
            blocks,
        })
    }
}

#[derive(Debug)]
struct WindowsOcrPayload {
    text: String,
    blocks: Vec<OcrTextBlock>,
}

fn run_windows_builtin_ocr_from_bytes(image_data: &[u8]) -> anyhow::Result<WindowsOcrPayload> {
    let temp_image_path = write_temp_image(image_data)?;
    let ocr_result = run_windows_builtin_ocr(&temp_image_path);
    let _ = fs::remove_file(&temp_image_path);
    ocr_result
}

fn write_temp_image(image_data: &[u8]) -> anyhow::Result<std::path::PathBuf> {
    let path = std::env::temp_dir().join(format!("flick-windows-ocr-{}.png", Uuid::new_v4()));
    fs::write(&path, image_data)
        .with_context(|| format!("failed to write temporary OCR image to {}", path.display()))?;
    Ok(path)
}

fn build_retry_image_bytes(image_data: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(image_data).ok()?;
    let (width, height) = image.dimensions();

    let scale = if height < 48 {
        4
    } else if height < 72 {
        3
    } else if height < 96 {
        2
    } else {
        1
    };

    if scale <= 1 {
        return None;
    }

    let resized = image.resize_exact(width * scale, height * scale, FilterType::CatmullRom);
    let mut encoded = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut encoded);
    if resized.write_to(&mut cursor, ImageFormat::Png).is_err() {
        return None;
    }
    Some(encoded)
}

fn run_windows_builtin_ocr(image_path: &Path) -> anyhow::Result<WindowsOcrPayload> {
    let apartment = initialize_com_apartment()?;
    let result = recognize_image(image_path);
    drop(apartment);
    result
}

fn recognize_image(image_path: &Path) -> anyhow::Result<WindowsOcrPayload> {
    let image_path = HSTRING::from(image_path.to_string_lossy().as_ref());
    let file = wait_async(
        StorageFile::GetFileFromPathAsync(&image_path)
            .context("failed to resolve OCR image path")?,
        "failed to open OCR image file",
    )?;
    let stream = file
        .OpenReadAsync()
        .context("failed to open OCR image stream")?;
    let stream = wait_async(stream, "failed to read OCR image stream")?;
    let decoder = wait_async(
        BitmapDecoder::CreateWithIdAsync(
            BitmapDecoder::PngDecoderId().context("failed to get PNG decoder id")?,
            &stream,
        )
        .context("failed to create bitmap decoder")?,
        "failed to decode OCR image",
    )?;

    let bitmap = wait_async(
        decoder
            .GetSoftwareBitmapConvertedAsync(
                BitmapPixelFormat::Bgra8,
                BitmapAlphaMode::Premultiplied,
            )
            .context("failed to convert OCR bitmap")?,
        "failed to load OCR bitmap",
    )?;

    let max_dimension = OcrEngine::MaxImageDimension().context("failed to query OCR limits")?;
    let width = bitmap.PixelWidth().context("failed to get bitmap width")?;
    let height = bitmap
        .PixelHeight()
        .context("failed to get bitmap height")?;
    if width > max_dimension as i32 || height > max_dimension as i32 {
        return Err(anyhow!(
            "image exceeds Windows OCR limit of {} px (got {}x{})",
            max_dimension,
            width,
            height
        ));
    }

    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .context("Windows built-in OCR engine is not available on this system")?;
    let result = wait_async(
        engine
            .RecognizeAsync(&bitmap)
            .context("failed to start Windows OCR recognition")?,
        "Windows OCR recognition failed",
    )?;

    let lines = result.Lines().context("failed to read Windows OCR lines")?;
    let line_count = lines
        .Size()
        .context("failed to read Windows OCR line count")?;
    let mut blocks = Vec::new();
    for index in 0..line_count {
        let line = lines
            .GetAt(index)
            .context("failed to read Windows OCR line")?;
        let text = line
            .Text()
            .context("failed to read Windows OCR line text")?;
        let text = text.to_string();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        blocks.push(OcrTextBlock {
            text: trimmed.to_string(),
            confidence: 1.0,
        });
    }

    let full_text = result
        .Text()
        .context("failed to read Windows OCR text")?
        .to_string();

    Ok(WindowsOcrPayload {
        text: full_text,
        blocks,
    })
}

fn wait_async<T>(operation: IAsyncOperation<T>, context: &'static str) -> anyhow::Result<T>
where
    T: RuntimeType + 'static,
{
    loop {
        match operation.Status().context(context)? {
            AsyncStatus::Started => std::thread::sleep(std::time::Duration::from_millis(5)),
            AsyncStatus::Completed => return operation.GetResults().context(context),
            AsyncStatus::Canceled => return Err(anyhow!("{context}: operation canceled")),
            AsyncStatus::Error => {
                let hr = operation.ErrorCode().context(context)?;
                return Err(anyhow!("{context}: HRESULT {:#x}", hr.0));
            }
            other => return Err(anyhow!("{context}: unexpected async status {:?}", other)),
        }
    }
}

fn initialize_com_apartment() -> anyhow::Result<ComApartmentGuard> {
    let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
    if hr < 0 && hr != -2147417850 {
        return Err(anyhow!(
            "failed to initialize COM apartment: HRESULT {hr:#x}"
        ));
    }
    Ok(ComApartmentGuard {
        should_uninitialize: hr >= 0,
    })
}

struct ComApartmentGuard {
    should_uninitialize: bool,
}

impl Drop for ComApartmentGuard {
    fn drop(&mut self) {
        if self.should_uninitialize {
            unsafe {
                CoUninitialize();
            }
        }
    }
}
