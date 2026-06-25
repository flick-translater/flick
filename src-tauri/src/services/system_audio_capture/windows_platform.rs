use std::{
    ptr::null_mut,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Context, anyhow};
use windows::Win32::{
    Media::Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
        IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX,
        eConsole, eRender,
    },
    System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
        CoUninitialize,
    },
};
use windows::core::HRESULT;

use super::{
    LiveSystemAudioStreamHandle, SystemAudioCaptureCapabilities, SystemAudioChunk,
    SystemAudioSampleFormat, SystemAudioSpec,
};

const RPC_E_CHANGED_MODE: HRESULT = HRESULT(0x80010106u32 as i32);
const WASAPI_BUFFER_DURATION_100NS: i64 = 10_000_000;
const POLL_INTERVAL: Duration = Duration::from_millis(5);
const WAVE_FORMAT_PCM_TAG: u32 = 1;
const WAVE_FORMAT_IEEE_FLOAT_TAG: u32 = 3;
const WAVE_FORMAT_EXTENSIBLE_TAG: u32 = 0xfffe;

struct WindowsSystemAudioStreamHandle {
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl LiveSystemAudioStreamHandle for WindowsSystemAudioStreamHandle {}

impl Drop for WindowsSystemAudioStreamHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
    }
}

pub fn capabilities() -> SystemAudioCaptureCapabilities {
    SystemAudioCaptureCapabilities {
        system_output_supported: true,
        system_output_available: true,
        message: "Windows system audio capture uses WASAPI loopback".into(),
    }
}

pub fn open_system_output_stream(
    on_audio: Box<dyn FnMut(SystemAudioChunk) + Send>,
) -> anyhow::Result<(Box<dyn LiveSystemAudioStreamHandle>, SystemAudioSpec)> {
    eprintln!("[recording][audio][windows] opening WASAPI loopback stream");
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let (spec_sender, spec_receiver) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("flick-windows-system-audio".into())
        .spawn(move || match open_wasapi_loopback() {
            Ok(audio) => {
                let _ = spec_sender.send(Ok(audio.spec));
                if let Err(error) = run_capture_loop(audio, worker_stop, on_audio) {
                    eprintln!("[recording][audio][windows] capture loop failed: {error:#}");
                }
            }
            Err(error) => {
                let message = format!("{error:#}");
                let _ = spec_sender.send(Err(message.clone()));
                eprintln!("[recording][audio][windows] failed to open WASAPI loopback: {message}");
            }
        })
        .context("failed to start WASAPI loopback thread")?;
    let spec = match spec_receiver
        .recv()
        .context("WASAPI loopback thread exited before reporting audio format")?
    {
        Ok(spec) => spec,
        Err(error) => {
            let _ = worker.join();
            return Err(anyhow!(error));
        }
    };

    Ok((
        Box::new(WindowsSystemAudioStreamHandle {
            stop,
            worker: Mutex::new(Some(worker)),
        }),
        spec,
    ))
}

struct WasapiLoopback {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    spec: SystemAudioSpec,
    bytes_per_frame: usize,
    co_uninitialize: bool,
}

fn open_wasapi_loopback() -> anyhow::Result<WasapiLoopback> {
    unsafe {
        let co_result = CoInitializeEx(None, COINIT_MULTITHREADED);
        let co_uninitialize = if co_result.is_ok() {
            true
        } else if co_result == RPC_E_CHANGED_MODE {
            false
        } else {
            return Err(anyhow!(
                "failed to initialize COM for WASAPI: HRESULT 0x{:08x}",
                co_result.0 as u32
            ));
        };

        let result = open_wasapi_loopback_inner(co_uninitialize);
        if result.is_err() && co_uninitialize {
            CoUninitialize();
        }
        result
    }
}

unsafe fn open_wasapi_loopback_inner(co_uninitialize: bool) -> anyhow::Result<WasapiLoopback> {
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .context("failed to create MMDeviceEnumerator")?;
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
        .context("failed to get default render endpoint")?;
    let client: IAudioClient =
        unsafe { device.Activate(CLSCTX_ALL, None) }.context("failed to activate IAudioClient")?;
    let format_ptr = unsafe { client.GetMixFormat() }.context("failed to get WASAPI mix format")?;
    if format_ptr.is_null() {
        return Err(anyhow!("WASAPI mix format is null"));
    }

    let format = unsafe { *format_ptr };
    let spec = system_audio_spec(&format)?;
    let bytes_per_frame = usize::from(format.nBlockAlign);
    unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            WASAPI_BUFFER_DURATION_100NS,
            0,
            format_ptr,
            None,
        )
    }
    .context("failed to initialize WASAPI loopback client")?;
    unsafe { CoTaskMemFree(Some(format_ptr.cast())) };
    let capture: IAudioCaptureClient =
        unsafe { client.GetService() }.context("failed to get IAudioCaptureClient")?;
    unsafe { client.Start() }.context("failed to start WASAPI loopback client")?;
    eprintln!(
        "[recording][audio][windows] audio stream started sample_rate={} channels={} format={:?}",
        spec.sample_rate, spec.channels, spec.sample_format
    );

    Ok(WasapiLoopback {
        client,
        capture,
        spec,
        bytes_per_frame,
        co_uninitialize,
    })
}

fn system_audio_spec(format: &WAVEFORMATEX) -> anyhow::Result<SystemAudioSpec> {
    let sample_format = match (format.wFormatTag as u32, format.wBitsPerSample) {
        (WAVE_FORMAT_IEEE_FLOAT_TAG, 32) => SystemAudioSampleFormat::F32Le,
        (WAVE_FORMAT_PCM_TAG, 16) => SystemAudioSampleFormat::S16Le,
        (WAVE_FORMAT_EXTENSIBLE_TAG, 32) => SystemAudioSampleFormat::F32Le,
        (WAVE_FORMAT_EXTENSIBLE_TAG, 16) => SystemAudioSampleFormat::S16Le,
        (tag, bits) => {
            return Err(anyhow!(
                "unsupported WASAPI mix format tag={tag} bits={bits}"
            ));
        }
    };
    if format.nChannels == 0 || format.nSamplesPerSec == 0 || format.nBlockAlign == 0 {
        return Err(anyhow!("invalid WASAPI mix format"));
    }

    Ok(SystemAudioSpec {
        sample_rate: format.nSamplesPerSec,
        channels: format.nChannels,
        sample_format,
    })
}

fn run_capture_loop(
    audio: WasapiLoopback,
    stop: Arc<AtomicBool>,
    mut on_audio: Box<dyn FnMut(SystemAudioChunk) + Send>,
) -> anyhow::Result<()> {
    let mut callback_count = 0u64;
    while !stop.load(Ordering::SeqCst) {
        let mut packet_size = unsafe { audio.capture.GetNextPacketSize() }
            .context("failed to get WASAPI packet size")?;
        if packet_size == 0 {
            thread::sleep(POLL_INTERVAL);
            continue;
        }

        while packet_size > 0 {
            let mut data_ptr = null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            unsafe {
                audio
                    .capture
                    .GetBuffer(&mut data_ptr, &mut frames, &mut flags, None, None)
            }
            .context("failed to read WASAPI packet")?;

            let byte_len = frames as usize * audio.bytes_per_frame;
            let data = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                vec![0; byte_len]
            } else if data_ptr.is_null() || byte_len == 0 {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(data_ptr, byte_len) }.to_vec()
            };
            unsafe { audio.capture.ReleaseBuffer(frames) }
                .context("failed to release WASAPI packet")?;

            if !data.is_empty() {
                callback_count += 1;
                if callback_count <= 3 || callback_count % 100 == 0 {
                    eprintln!(
                        "[recording][audio][windows] callback={} bytes={} frames={}",
                        callback_count,
                        data.len(),
                        frames
                    );
                }
                on_audio(SystemAudioChunk { data, frames });
            }

            packet_size = unsafe { audio.capture.GetNextPacketSize() }
                .context("failed to get WASAPI packet size")?;
        }
    }

    unsafe { audio.client.Stop() }.context("failed to stop WASAPI loopback client")?;
    if audio.co_uninitialize {
        unsafe { CoUninitialize() };
    }
    Ok(())
}
