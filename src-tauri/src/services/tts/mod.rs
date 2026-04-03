mod edge;
mod playback;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::anyhow;
use async_trait::async_trait;
pub use edge::EdgeTtsEngine;
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

pub(super) struct TtsRuntime {
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
                .and_then(|audio| {
                    playback::play_audio_with_lifecycle(&inner, session_id, target, audio)
                });

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

fn mark_inactive(inner: &Arc<Mutex<TtsRuntime>>, session_id: u64) {
    if let Ok(mut guard) = inner.lock() {
        if guard.session_id == session_id {
            guard.status = TtsStatus::Idle;
            guard.target = None;
        }
    }
}
