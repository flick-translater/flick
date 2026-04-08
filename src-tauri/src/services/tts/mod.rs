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

use crate::models::TtsEngineInfo;

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
    engine: Arc<Mutex<Arc<dyn TextToSpeechEngine>>>,
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
            engine: Arc::new(Mutex::new(create_tts_engine("edge"))),
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
        let engine = self
            .engine
            .lock()
            .map_err(|_| anyhow!("tts engine mutex poisoned"))?
            .clone();
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

    pub fn set_engine(&self, engine_id: &str) -> anyhow::Result<String> {
        let normalized = normalize_tts_engine_id(engine_id);
        let mut engine = self
            .engine
            .lock()
            .map_err(|_| anyhow!("tts engine mutex poisoned"))?;
        *engine = create_tts_engine(&normalized);
        Ok(normalized)
    }

    pub fn snapshot(&self) -> TtsSnapshot {
        let engine_id = self
            .engine
            .lock()
            .map(|engine| engine.id().to_string())
            .unwrap_or_else(|_| "edge".to_string());
        self.inner
            .lock()
            .map(|inner| TtsSnapshot {
                status: inner.status,
                target: inner.target,
                engine: engine_id.clone(),
            })
            .unwrap_or_else(|_| TtsSnapshot {
                status: TtsStatus::Idle,
                target: None,
                engine: engine_id,
            })
    }
}

pub fn create_tts_engine(engine_id: &str) -> Arc<dyn TextToSpeechEngine> {
    match normalize_tts_engine_id(engine_id).as_str() {
        "edge" => Arc::new(EdgeTtsEngine),
        _ => Arc::new(EdgeTtsEngine),
    }
}

pub fn available_tts_engines() -> Vec<TtsEngineInfo> {
    vec![TtsEngineInfo { id: "edge".into() }]
}

pub fn normalize_tts_engine_id(engine_id: &str) -> String {
    match engine_id.trim().to_lowercase().as_str() {
        "edge" => "edge".into(),
        _ => "edge".into(),
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
