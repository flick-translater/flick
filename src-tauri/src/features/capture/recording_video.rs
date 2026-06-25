use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

use anyhow::Context;
use image::imageops::FilterType;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use crate::services::system_audio_capture::{
    LiveSystemAudioStream, SystemAudioCaptureService, SystemAudioSampleFormat, SystemAudioSpec,
};

use super::recording::{
    RecordingAudioMode, RecordingEncodeResult, RecordingEncoderOptions, RecordingMessage,
};

struct AudioCaptureContext {
    stream: Option<LiveSystemAudioStream>,
    writer: Arc<Mutex<BufWriter<fs::File>>>,
    path: PathBuf,
    spec: SystemAudioSpec,
    bytes: Arc<AtomicU64>,
    chunks: Arc<AtomicU64>,
}

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub(super) fn encode_video(
    path: PathBuf,
    options: RecordingEncoderOptions,
    receiver: mpsc::Receiver<RecordingMessage>,
) -> anyhow::Result<RecordingEncodeResult> {
    let ffmpeg_path = options
        .ffmpeg_path
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("ffmpeg path is missing"))?
        .to_string();
    let silent_path = if options.audio == RecordingAudioMode::System {
        path.with_extension("silent.mp4")
    } else {
        path.clone()
    };
    let mut audio = if options.audio == RecordingAudioMode::System {
        match start_system_audio_capture(&path) {
            Ok(audio) => Some(audio),
            Err(error) => {
                eprintln!(
                    "[recording][audio] failed to start system audio capture; continuing without audio: {error}"
                );
                None
            }
        }
    } else {
        eprintln!("[recording][audio] audio disabled mode={:?}", options.audio);
        None
    };
    let mut child = None;
    let mut input_width = 0;
    let mut input_height = 0;
    let mut first_frame_elapsed: Option<Duration> = None;
    let mut last_pixels: Option<Vec<u8>> = None;
    let mut frames_received = 0u32;
    let mut frames_written = 0u32;

    while let Ok(message) = receiver.recv() {
        match message {
            RecordingMessage::Frame {
                image: frame,
                elapsed,
            } => {
                frames_received += 1;
                if child.is_none() {
                    input_width = frame.width();
                    input_height = frame.height();
                    let (scaled_width, scaled_height) = scaled_video_size(
                        input_width,
                        input_height,
                        options.max_width,
                        options.max_height,
                    );
                    eprintln!(
                        "[recording][video] starting ffmpeg rawvideo encoder path={} size={}x{} fps={} audio={:?}",
                        silent_path.display(),
                        scaled_width,
                        scaled_height,
                        options.fps,
                        options.audio
                    );
                    child = Some(spawn_ffmpeg_encoder(
                        &silent_path,
                        &ffmpeg_path,
                        input_width,
                        input_height,
                        scaled_width,
                        scaled_height,
                        options.fps,
                    )?);
                    first_frame_elapsed = Some(elapsed);
                }

                let pixels = if frame.width() == input_width && frame.height() == input_height {
                    frame.into_raw()
                } else {
                    image::imageops::resize(&frame, input_width, input_height, FilterType::Triangle)
                        .into_raw()
                };

                let relative_elapsed =
                    elapsed.saturating_sub(first_frame_elapsed.unwrap_or(elapsed));
                let target_frame_count =
                    target_video_frame_count(relative_elapsed, options.fps).max(1);
                if let Some(last_pixels) = last_pixels.as_deref() {
                    while frames_written < target_frame_count.saturating_sub(1) {
                        write_video_pixels(&mut child, last_pixels)?;
                        frames_written += 1;
                    }
                }
                write_video_pixels(&mut child, &pixels)?;
                frames_written += 1;
                if frames_written <= 3 || frames_written % 100 == 0 {
                    eprintln!(
                        "[recording][video] wrote frames={} received={} elapsed_ms={} target={}",
                        frames_written,
                        frames_received,
                        elapsed.as_millis(),
                        target_frame_count
                    );
                }
                last_pixels = Some(pixels);
            }
            RecordingMessage::End { elapsed } => {
                if let (Some(started), Some(last_pixels)) =
                    (first_frame_elapsed, last_pixels.as_deref())
                {
                    let relative_elapsed = elapsed.saturating_sub(started);
                    let target_frame_count =
                        target_video_frame_count(relative_elapsed, options.fps).max(1);
                    while frames_written < target_frame_count {
                        write_video_pixels(&mut child, last_pixels)?;
                        frames_written += 1;
                    }
                    eprintln!(
                        "[recording][video] padded to end frames={} received={} elapsed_ms={} target={}",
                        frames_written,
                        frames_received,
                        elapsed.as_millis(),
                        target_frame_count
                    );
                }
            }
        }
    }

    let (audio_chunks, audio_bytes) = if let Some(audio) = audio.as_mut() {
        eprintln!("[recording][audio] stopping system audio before video encoder flush");
        stop_system_audio_capture(audio)?;
        (
            audio.chunks.load(Ordering::SeqCst),
            audio.bytes.load(Ordering::SeqCst),
        )
    } else {
        (0, 0)
    };

    if let Some(mut process) = child {
        eprintln!("[recording][video] closing ffmpeg stdin and waiting for encoder flush");
        drop(process.stdin.take());
        let output = process
            .wait_with_output()
            .context("failed to wait for ffmpeg")?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "ffmpeg failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    eprintln!(
        "[recording][video] silent video encoder finished path={} frames={} received={}",
        silent_path.display(),
        frames_written,
        frames_received
    );

    if let Some(audio) = audio {
        if audio_bytes > 0 {
            mux_video_with_pcm_audio(&ffmpeg_path, &silent_path, &audio.path, audio.spec, &path)?;
        } else {
            eprintln!("[recording][audio] no audio bytes captured; using silent video");
            if silent_path != path {
                fs::rename(&silent_path, &path).with_context(|| {
                    format!(
                        "failed to move silent video from {} to {}",
                        silent_path.display(),
                        path.display()
                    )
                })?;
            }
        }
        let _ = fs::remove_file(&audio.path);
        if silent_path != path {
            let _ = fs::remove_file(&silent_path);
        }
    }

    Ok(RecordingEncodeResult {
        frame_count: frames_written,
        audio_bytes,
        audio_chunks,
    })
}

fn write_video_pixels(
    child: &mut Option<std::process::Child>,
    pixels: &[u8],
) -> anyhow::Result<()> {
    child
        .as_mut()
        .and_then(|process| process.stdin.as_mut())
        .ok_or_else(|| anyhow::anyhow!("ffmpeg stdin is missing"))?
        .write_all(pixels)
        .context("failed to write frame to ffmpeg")
}

fn target_video_frame_count(elapsed: Duration, fps: u32) -> u32 {
    ((elapsed.as_secs_f64() * f64::from(fps.max(1))).floor() as u32).saturating_add(1)
}

fn start_system_audio_capture(base_path: &Path) -> anyhow::Result<AudioCaptureContext> {
    let audio_path = base_path.with_extension("audio.f32le");
    let file = fs::File::create(&audio_path)
        .with_context(|| format!("failed to create audio temp file {}", audio_path.display()))?;
    let writer = Arc::new(Mutex::new(BufWriter::new(file)));
    let bytes = Arc::new(AtomicU64::new(0));
    let chunks = Arc::new(AtomicU64::new(0));
    let callback_writer = Arc::clone(&writer);
    let callback_bytes = Arc::clone(&bytes);
    let callback_chunks = Arc::clone(&chunks);
    let stream = SystemAudioCaptureService.open_system_output_stream(Box::new(move |chunk| {
        let next_chunk = callback_chunks.fetch_add(1, Ordering::SeqCst) + 1;
        callback_bytes.fetch_add(chunk.data.len() as u64, Ordering::SeqCst);
        if next_chunk <= 3 || next_chunk % 100 == 0 {
            eprintln!(
                "[recording][audio] pipeline chunk={} bytes={} frames={}",
                next_chunk,
                chunk.data.len(),
                chunk.frames
            );
        }
        if let Ok(mut writer) = callback_writer.lock() {
            if let Err(error) = writer.write_all(&chunk.data) {
                eprintln!("[recording][audio] failed to write pcm chunk: {error}");
            }
        }
    }))?;
    let spec = stream.spec();
    eprintln!(
        "[recording][audio] system audio capture started path={} sample_rate={} channels={} format={:?}",
        audio_path.display(),
        spec.sample_rate,
        spec.channels,
        spec.sample_format
    );
    Ok(AudioCaptureContext {
        stream: Some(stream),
        writer,
        path: audio_path,
        spec,
        bytes,
        chunks,
    })
}

fn stop_system_audio_capture(audio: &mut AudioCaptureContext) -> anyhow::Result<()> {
    if let Some(stream) = audio.stream.take() {
        stream.stop();
    }
    if let Ok(mut writer) = audio.writer.lock() {
        writer.flush().context("failed to flush audio pcm file")?;
    }
    eprintln!(
        "[recording][audio] system audio capture stopped chunks={} bytes={}",
        audio.chunks.load(Ordering::SeqCst),
        audio.bytes.load(Ordering::SeqCst)
    );
    Ok(())
}

fn mux_video_with_pcm_audio(
    ffmpeg_path: &str,
    video_path: &Path,
    audio_path: &Path,
    audio_spec: SystemAudioSpec,
    output_path: &Path,
) -> anyhow::Result<()> {
    let sample_format = match audio_spec.sample_format {
        SystemAudioSampleFormat::F32Le => "f32le",
        SystemAudioSampleFormat::S16Le => "s16le",
    };
    eprintln!(
        "[recording][mux] mux start video={} audio={} output={} format={} sample_rate={} channels={}",
        video_path.display(),
        audio_path.display(),
        output_path.display(),
        sample_format,
        audio_spec.sample_rate,
        audio_spec.channels
    );
    let mut command = Command::new(ffmpeg_path);
    hide_command_window(&mut command);
    let output = command
        .arg("-y")
        .arg("-i")
        .arg(video_path)
        .arg("-f")
        .arg(sample_format)
        .arg("-ar")
        .arg(audio_spec.sample_rate.to_string())
        .arg("-ac")
        .arg(audio_spec.channels.to_string())
        .arg("-i")
        .arg(audio_path)
        .arg("-c:v")
        .arg("copy")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg("128k")
        .arg("-shortest")
        .arg("-movflags")
        .arg("+faststart")
        .arg(output_path)
        .output()
        .context("failed to start ffmpeg mux")?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "ffmpeg mux failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    eprintln!(
        "[recording][mux] mux complete output={}",
        output_path.display()
    );
    Ok(())
}

fn spawn_ffmpeg_encoder(
    path: &Path,
    ffmpeg_path: &str,
    input_width: u32,
    input_height: u32,
    width: u32,
    height: u32,
    fps: u32,
) -> anyhow::Result<std::process::Child> {
    let mut command = Command::new(ffmpeg_path);
    hide_command_window(&mut command);
    command
        .arg("-y")
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("rgba")
        .arg("-s")
        .arg(format!("{input_width}x{input_height}"))
        .arg("-r")
        .arg(fps.to_string())
        .arg("-i")
        .arg("pipe:0")
        .arg("-an");

    if input_width != width || input_height != height {
        command
            .arg("-vf")
            .arg(format!("scale={width}:{height}:flags=fast_bilinear"));
    }

    command
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("veryfast")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-movflags")
        .arg("+faststart")
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to start ffmpeg")
}

fn hide_command_window(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

fn scaled_video_size(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    let (scaled_width, scaled_height) = scaled_recording_size(width, height, max_width, max_height);
    (even_dimension(scaled_width), even_dimension(scaled_height))
}

fn scaled_recording_size(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    if max_width == 0 || max_height == 0 || (width <= max_width && height <= max_height) {
        return (width.max(1), height.max(1));
    }
    let scale = (max_width as f64 / width.max(1) as f64)
        .min(max_height as f64 / height.max(1) as f64)
        .min(1.0);
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
}

fn even_dimension(value: u32) -> u32 {
    value.max(2) & !1
}
