//! Platform-specific audio playback for synthesized speech.

use std::{
    io::Cursor,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::{Context, anyhow};
use rodio::{Decoder, DeviceSinkBuilder, Player};

use crate::services::tts::TtsStatus;

use super::{TtsRuntime, TtsTarget};

pub fn play_audio_with_lifecycle(
    inner: &Arc<Mutex<TtsRuntime>>,
    session_id: u64,
    target: TtsTarget,
    audio: Vec<u8>,
) -> anyhow::Result<()> {
    if !is_current_session(inner, session_id)? {
        return Ok(());
    }

    {
        let mut guard = inner
            .lock()
            .map_err(|_| anyhow!("tts runtime mutex poisoned"))?;
        if guard.session_id != session_id {
            return Ok(());
        }
        guard.status = TtsStatus::Playing;
        guard.target = Some(target);
    }

    let sink = DeviceSinkBuilder::open_default_sink()
        .context("failed to open default audio output")?;
    let player = Player::connect_new(&sink.mixer());
    let decoder = Decoder::try_from(Cursor::new(audio)).context("failed to decode tts audio")?;
    player.append(decoder);
    player.play();

    while !player.empty() {
        if !is_current_session(inner, session_id)? {
            player.stop();
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }

    Ok(())
}

fn is_current_session(inner: &Arc<Mutex<TtsRuntime>>, session_id: u64) -> anyhow::Result<bool> {
    let guard = inner
        .lock()
        .map_err(|_| anyhow!("tts runtime mutex poisoned"))?;
    Ok(guard.status != TtsStatus::Idle && guard.session_id == session_id)
}
