use anyhow::anyhow;

use super::{
    LiveSystemAudioStreamHandle, SystemAudioCaptureCapabilities, SystemAudioChunk, SystemAudioSpec,
};

struct WindowsSystemAudioStreamHandle;

impl LiveSystemAudioStreamHandle for WindowsSystemAudioStreamHandle {}

pub fn capabilities() -> SystemAudioCaptureCapabilities {
    SystemAudioCaptureCapabilities {
        system_output_supported: true,
        system_output_available: false,
        message: "Windows system audio capture will use WASAPI loopback; implementation pending"
            .into(),
    }
}

pub fn open_system_output_stream(
    on_audio: Box<dyn FnMut(SystemAudioChunk) + Send>,
) -> anyhow::Result<(Box<dyn LiveSystemAudioStreamHandle>, SystemAudioSpec)> {
    let _ = on_audio;
    let _ = WindowsSystemAudioStreamHandle;
    Err(anyhow!(
        "Windows system audio capture is not implemented yet"
    ))
}
