use std::sync::Mutex;

use anyhow::anyhow;
use screencapturekit::{
    cm::CMSampleBuffer,
    dispatch_queue::{DispatchQoS, DispatchQueue},
    prelude::{
        SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamOutputTrait,
        SCStreamOutputType,
    },
};

use super::{
    LiveSystemAudioStreamHandle, SystemAudioCaptureCapabilities, SystemAudioChunk,
    SystemAudioSampleFormat, SystemAudioSpec,
};

const AUDIO_SAMPLE_RATE: u32 = 48_000;
const AUDIO_CHANNELS: u16 = 2;

struct MacosSystemAudioStreamHandle {
    stream: SCStream,
    handler_id: Option<usize>,
    output_queue: DispatchQueue,
}

impl LiveSystemAudioStreamHandle for MacosSystemAudioStreamHandle {}

impl Drop for MacosSystemAudioStreamHandle {
    fn drop(&mut self) {
        if let Some(handler_id) = self.handler_id.take() {
            let _ = self
                .stream
                .remove_output_handler(handler_id, SCStreamOutputType::Audio);
        }
        drain_dispatch_queue(&self.output_queue);
        let _ = self.stream.stop_capture();
        drain_dispatch_queue(&self.output_queue);
    }
}

struct SystemAudioHandler {
    on_audio: Mutex<Box<dyn FnMut(SystemAudioChunk) + Send>>,
}

impl SCStreamOutputTrait for SystemAudioHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if of_type != SCStreamOutputType::Audio {
            return;
        }

        let Some(audio_buffers) = sample.audio_buffer_list() else {
            eprintln!("[recording][audio][macos] audio sample without buffer list");
            return;
        };

        let data = interleaved_f32_audio_data(&audio_buffers);
        if data.is_empty() {
            return;
        }

        let frames = (data.len() / (usize::from(AUDIO_CHANNELS) * std::mem::size_of::<f32>()))
            .try_into()
            .unwrap_or(0);
        if let Ok(mut on_audio) = self.on_audio.lock() {
            (on_audio)(SystemAudioChunk { data, frames });
        }
    }
}

fn interleaved_f32_audio_data(audio_buffers: &screencapturekit::cm::AudioBufferList) -> Vec<u8> {
    if audio_buffers.num_buffers() == 1 {
        return audio_buffers
            .buffer(0)
            .map(|buffer| buffer.data().to_vec())
            .unwrap_or_default();
    }

    let buffers: Vec<_> = audio_buffers.iter().collect();
    if buffers.is_empty() {
        return Vec::new();
    }
    let bytes_per_sample = std::mem::size_of::<f32>();
    let frame_count = buffers
        .iter()
        .map(|buffer| buffer.data().len() / bytes_per_sample)
        .min()
        .unwrap_or(0);
    let channel_count = buffers.len();
    if frame_count == 0 {
        return Vec::new();
    }

    let mut interleaved = Vec::with_capacity(frame_count * channel_count * bytes_per_sample);
    for frame in 0..frame_count {
        for buffer in &buffers {
            let start = frame * bytes_per_sample;
            let end = start + bytes_per_sample;
            interleaved.extend_from_slice(&buffer.data()[start..end]);
        }
    }
    interleaved
}

pub fn capabilities() -> SystemAudioCaptureCapabilities {
    SystemAudioCaptureCapabilities {
        system_output_supported: true,
        system_output_available: true,
        message: "macOS system audio capture uses ScreenCaptureKit system audio".into(),
    }
}

pub fn open_system_output_stream(
    on_audio: Box<dyn FnMut(SystemAudioChunk) + Send>,
) -> anyhow::Result<(Box<dyn LiveSystemAudioStreamHandle>, SystemAudioSpec)> {
    let content = SCShareableContent::get()
        .map_err(|error| anyhow!("failed to get shareable content: {error}"))?;
    let display = content
        .displays()
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("no display available for system audio capture"))?;
    let excluded_windows = windows_owned_by_current_process(&content);
    let excluded_window_refs: Vec<_> = excluded_windows.iter().collect();
    let filter = SCContentFilter::create()
        .with_display(&display)
        .with_excluding_windows(&excluded_window_refs)
        .build();
    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_captures_audio(true)
        .with_excludes_current_process_audio(true)
        .with_sample_rate(AUDIO_SAMPLE_RATE as i32)
        .with_channel_count(AUDIO_CHANNELS as i32);

    let handler = SystemAudioHandler {
        on_audio: Mutex::new(on_audio),
    };
    let output_queue = DispatchQueue::new(
        "io.github.flick-translater.flick.recording.system-audio",
        DispatchQoS::UserInteractive,
    );
    let mut stream = SCStream::new(&filter, &config);
    let handler_id = stream
        .add_output_handler_with_queue(handler, SCStreamOutputType::Audio, Some(&output_queue))
        .ok_or_else(|| anyhow!("failed to add ScreenCaptureKit audio output handler"))?;
    stream
        .start_capture()
        .map_err(|error| anyhow!("failed to start ScreenCaptureKit audio stream: {error}"))?;

    Ok((
        Box::new(MacosSystemAudioStreamHandle {
            stream,
            handler_id: Some(handler_id),
            output_queue,
        }),
        SystemAudioSpec {
            sample_rate: AUDIO_SAMPLE_RATE,
            channels: AUDIO_CHANNELS,
            sample_format: SystemAudioSampleFormat::F32Le,
        },
    ))
}

fn windows_owned_by_current_process(
    content: &SCShareableContent,
) -> Vec<screencapturekit::shareable_content::SCWindow> {
    let current_pid = std::process::id() as i32;
    content
        .windows()
        .into_iter()
        .filter(|window| {
            window
                .owning_application()
                .is_some_and(|app| app.process_id() == current_pid)
        })
        .collect()
}

extern "C" fn dispatch_noop(_context: *mut std::ffi::c_void) {}

fn drain_dispatch_queue(queue: &DispatchQueue) {
    unsafe extern "C" {
        fn dispatch_sync_f(
            queue: *const std::ffi::c_void,
            context: *mut std::ffi::c_void,
            work: extern "C" fn(*mut std::ffi::c_void),
        );
    }

    unsafe {
        dispatch_sync_f(queue.as_ptr(), std::ptr::null_mut(), dispatch_noop);
    }
}
