//! Cross-platform system audio capture facade.
//!
//! The recording feature should depend on this facade instead of branching on OS details. Platform
//! modules own the actual capture APIs, while callers receive normalized interleaved PCM chunks.
#![allow(dead_code)]

#[cfg(target_os = "linux")]
mod linux_platform;
#[cfg(target_os = "macos")]
mod macos_platform;
#[cfg(target_os = "windows")]
mod windows_platform;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemAudioCaptureCapabilities {
    pub system_output_supported: bool,
    pub system_output_available: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SystemAudioSpec {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: SystemAudioSampleFormat,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SystemAudioSampleFormat {
    S16Le,
    F32Le,
}

#[derive(Debug, Clone)]
pub struct SystemAudioChunk {
    pub data: Vec<u8>,
    pub frames: u32,
}

pub struct LiveSystemAudioStream {
    #[allow(dead_code)]
    inner: Box<dyn LiveSystemAudioStreamHandle>,
    spec: SystemAudioSpec,
}

impl LiveSystemAudioStream {
    pub fn spec(&self) -> SystemAudioSpec {
        self.spec
    }

    pub fn stop(self) {}
}

pub(crate) trait LiveSystemAudioStreamHandle: Send {}

#[derive(Default)]
pub struct SystemAudioCaptureService;

impl SystemAudioCaptureService {
    pub fn capabilities(&self) -> SystemAudioCaptureCapabilities {
        #[cfg(target_os = "windows")]
        {
            return windows_platform::capabilities();
        }

        #[cfg(target_os = "macos")]
        {
            return macos_platform::capabilities();
        }

        #[cfg(target_os = "linux")]
        {
            return linux_platform::capabilities();
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            SystemAudioCaptureCapabilities {
                system_output_supported: false,
                system_output_available: false,
                message: "system audio capture is not supported on this platform".into(),
            }
        }
    }

    pub fn open_system_output_stream(
        &self,
        on_audio: Box<dyn FnMut(SystemAudioChunk) + Send>,
    ) -> anyhow::Result<LiveSystemAudioStream> {
        #[cfg(target_os = "windows")]
        {
            let (inner, spec) = windows_platform::open_system_output_stream(on_audio)?;
            return Ok(LiveSystemAudioStream { inner, spec });
        }

        #[cfg(target_os = "macos")]
        {
            let (inner, spec) = macos_platform::open_system_output_stream(on_audio)?;
            return Ok(LiveSystemAudioStream { inner, spec });
        }

        #[cfg(target_os = "linux")]
        {
            let (inner, spec) = linux_platform::open_system_output_stream(on_audio)?;
            return Ok(LiveSystemAudioStream { inner, spec });
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            let _ = on_audio;
            Err(anyhow::anyhow!(
                "system audio capture is not supported on this platform"
            ))
        }
    }
}
