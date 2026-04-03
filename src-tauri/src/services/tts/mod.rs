mod edge;

use std::{
    io::Cursor,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::{Context, anyhow};
use async_trait::async_trait;
pub use edge::EdgeTtsEngine;
use rodio::{Decoder, DeviceSinkBuilder, Player};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TtsStatus {
    Idle,
    Generating,
    Playing,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TtsTarget {
    Source,
    Translation,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TtsSnapshot {
    pub status: TtsStatus,
    pub target: Option<TtsTarget>,
    pub engine: String,
}

#[async_trait]
pub trait TextToSpeechEngine: Send + Sync {
    fn id(&self) -> &'static str;
    async fn synthesize(&self, text: &str, language: Option<&str>) -> anyhow::Result<Vec<u8>>;
}

#[derive(Clone)]
pub struct TtsService {
    inner: Arc<Mutex<TtsRuntime>>,
    engine: Arc<dyn TextToSpeechEngine>,
}

struct TtsRuntime {
    session_id: u64,
    status: TtsStatus,
    target: Option<TtsTarget>,
}

impl TtsService {
    pub fn new(_data_dir: PathBuf) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TtsRuntime {
                session_id: 0,
                status: TtsStatus::Idle,
                target: None,
            })),
            engine: create_tts_engine("edge"),
        }
    }

    pub fn speak(
        &self,
        text: &str,
        language: Option<&str>,
        target: TtsTarget,
    ) -> anyhow::Result<()> {
        let text = text.trim();
        if text.is_empty() {
            return Err(anyhow!("tts text is empty"));
        }

        let text = text.to_string();
        let language = language.map(str::to_owned);

        let mut inner = self
            .inner
            .lock()
            .map_err(|_| anyhow!("tts runtime mutex poisoned"))?;
        inner.session_id = inner.session_id.wrapping_add(1);
        inner.status = TtsStatus::Generating;
        inner.target = Some(target);
        let session_id = inner.session_id;
        drop(inner);

        let inner = Arc::clone(&self.inner);
        let engine = Arc::clone(&self.engine);
        tauri::async_runtime::spawn(async move {
            let result = engine
                .synthesize(&text, language.as_deref())
                .await
                .and_then(|audio| play_audio_with_lifecycle(&inner, session_id, target, audio));

            if let Err(error) = result {
                eprintln!("tts session failed: {error}");
            }

            mark_inactive(&inner, session_id);
        });

        Ok(())
    }

    pub fn stop(&self) -> anyhow::Result<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| anyhow!("tts runtime mutex poisoned"))?;
        inner.session_id = inner.session_id.wrapping_add(1);
        inner.status = TtsStatus::Idle;
        inner.target = None;
        Ok(())
    }

    pub fn snapshot(&self) -> TtsSnapshot {
        self.inner
            .lock()
            .map(|inner| TtsSnapshot {
                status: inner.status,
                target: inner.target,
                engine: self.engine.id().to_string(),
            })
            .unwrap_or_else(|_| TtsSnapshot {
                status: TtsStatus::Idle,
                target: None,
                engine: self.engine.id().to_string(),
            })
    }
}

pub fn create_tts_engine(engine_id: &str) -> Arc<dyn TextToSpeechEngine> {
    match engine_id {
        "edge" => Arc::new(EdgeTtsEngine),
        _ => Arc::new(EdgeTtsEngine),
    }
}

fn play_audio_with_lifecycle(
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

    let sink =
        DeviceSinkBuilder::open_default_sink().context("failed to open default audio output")?;
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

fn mark_inactive(inner: &Arc<Mutex<TtsRuntime>>, session_id: u64) {
    if let Ok(mut guard) = inner.lock() {
        if guard.session_id == session_id {
            guard.status = TtsStatus::Idle;
            guard.target = None;
        }
    }
}
