use anyhow::anyhow;

use super::{
    LiveSystemAudioStreamHandle, SystemAudioCaptureCapabilities, SystemAudioChunk, SystemAudioSpec,
};

struct LinuxSystemAudioStreamHandle;

impl LiveSystemAudioStreamHandle for LinuxSystemAudioStreamHandle {}

pub fn capabilities() -> SystemAudioCaptureCapabilities {
    SystemAudioCaptureCapabilities {
        system_output_supported: true,
        system_output_available: false,
        message:
            "Linux system audio capture will use PipeWire or PulseAudio; implementation pending"
                .into(),
    }
}

pub fn open_system_output_stream(
    on_audio: Box<dyn FnMut(SystemAudioChunk) + Send>,
) -> anyhow::Result<(Box<dyn LiveSystemAudioStreamHandle>, SystemAudioSpec)> {
    let _ = on_audio;
    let _ = LinuxSystemAudioStreamHandle;
    Err(anyhow!("Linux system audio capture is not implemented yet"))
}
