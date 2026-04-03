//! Platform-specific audio playback for synthesized speech.

use std::sync::{Arc, Mutex};

use super::{TtsRuntime, TtsTarget};

pub fn play_audio_with_lifecycle(
    inner: &Arc<Mutex<TtsRuntime>>,
    session_id: u64,
    target: TtsTarget,
    audio: Vec<u8>,
) -> anyhow::Result<()> {
    imp::play_audio_with_lifecycle(inner, session_id, target, audio)
}

#[cfg(target_os = "macos")]
mod imp {
    use std::{
        io::Cursor,
        sync::{Arc, Mutex},
        thread,
        time::Duration,
    };

    use anyhow::{Context, anyhow};
    use rodio::{Decoder, DeviceSinkBuilder, Player};

    use super::{TtsRuntime, TtsStatus, TtsTarget};

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
        let decoder =
            Decoder::try_from(Cursor::new(audio)).context("failed to decode tts audio")?;
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
}

#[cfg(target_os = "windows")]
mod imp {
    use std::{
        io::Cursor,
        sync::{Arc, Mutex},
        thread,
        time::Duration,
    };

    use anyhow::{Context, anyhow};
    use rodio::{Decoder, DeviceSinkBuilder, Player};

    use super::{TtsRuntime, TtsStatus, TtsTarget};

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
        let decoder =
            Decoder::try_from(Cursor::new(audio)).context("failed to decode tts audio")?;
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
}

#[cfg(target_os = "linux")]
mod imp {
    use std::sync::{Arc, Mutex};

    use anyhow::anyhow;

    use super::{TtsRuntime, TtsTarget};

    pub fn play_audio_with_lifecycle(
        _inner: &Arc<Mutex<TtsRuntime>>,
        _session_id: u64,
        _target: TtsTarget,
        _audio: Vec<u8>,
    ) -> anyhow::Result<()> {
        Err(anyhow!("tts playback is not implemented on this platform"))
    }
}
