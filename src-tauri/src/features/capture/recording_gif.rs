use std::{fs, io::BufWriter, path::PathBuf, sync::mpsc};

use anyhow::Context;
use image::imageops::FilterType;

use super::recording::{RecordingEncodeResult, RecordingEncoderOptions, RecordingMessage};

pub(super) fn encode_gif(
    path: PathBuf,
    options: RecordingEncoderOptions,
    receiver: mpsc::Receiver<RecordingMessage>,
) -> anyhow::Result<RecordingEncodeResult> {
    let file = fs::File::create(&path)
        .with_context(|| format!("failed to create gif recording file at {}", path.display()))?;
    let mut writer = Some(BufWriter::new(file));
    let mut encoder: Option<gif::Encoder<BufWriter<fs::File>>> = None;
    let mut output_width = 0;
    let mut output_height = 0;

    let mut frame_count = 0;
    while let Ok(message) = receiver.recv() {
        match message {
            RecordingMessage::Frame { image: frame, .. } => {
                if encoder.is_none() {
                    let (scaled_width, scaled_height) = scaled_gif_size(
                        frame.width(),
                        frame.height(),
                        options.max_width,
                        options.max_height,
                    );
                    output_width = scaled_width;
                    output_height = scaled_height;
                    let width_u16 = u16::try_from(output_width).context("invalid GIF width")?;
                    let height_u16 = u16::try_from(output_height).context("invalid GIF height")?;
                    let mut next_encoder = gif::Encoder::new(
                        writer
                            .take()
                            .ok_or_else(|| anyhow::anyhow!("GIF writer is missing"))?,
                        width_u16,
                        height_u16,
                        &[],
                    )
                    .context("failed to create GIF encoder")?;
                    next_encoder
                        .set_repeat(gif::Repeat::Infinite)
                        .context("failed to configure GIF loop")?;
                    encoder = Some(next_encoder);
                }
                let width_u16 = u16::try_from(output_width).context("invalid GIF width")?;
                let height_u16 = u16::try_from(output_height).context("invalid GIF height")?;
                let mut pixels = if frame.width() == output_width && frame.height() == output_height
                {
                    frame.into_raw()
                } else {
                    image::imageops::resize(
                        &frame,
                        output_width,
                        output_height,
                        FilterType::Triangle,
                    )
                    .into_raw()
                };
                let mut gif_frame =
                    gif::Frame::from_rgba_speed(width_u16, height_u16, &mut pixels, 10);
                gif_frame.delay = gif_delay_cs(options.fps);
                encoder
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("GIF encoder is missing"))?
                    .write_frame(&gif_frame)
                    .context("failed to write GIF frame")?;
                frame_count += 1;
            }
            RecordingMessage::End { .. } => {}
        }
    }

    Ok(RecordingEncodeResult { frame_count })
}

fn scaled_gif_size(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
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

fn gif_delay_cs(fps: u32) -> u16 {
    (100 / fps.max(1)) as u16
}
