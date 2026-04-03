use anyhow::Context;
use async_trait::async_trait;
use edge_tts_rust::{Boundary, EdgeTtsClient, SpeakOptions};

use super::TextToSpeechEngine;

pub struct EdgeTtsEngine;

#[async_trait]
impl TextToSpeechEngine for EdgeTtsEngine {
    fn id(&self) -> &'static str {
        "edge"
    }

    async fn synthesize(&self, text: &str, language: Option<&str>) -> anyhow::Result<Vec<u8>> {
        let voice = select_voice(language);
        let client = EdgeTtsClient::new().context("failed to initialize edge tts client")?;
        let result = client
            .synthesize(
                text,
                SpeakOptions {
                    voice: voice.into(),
                    boundary: Boundary::Sentence,
                    ..SpeakOptions::default()
                },
            )
            .await
            .with_context(|| format!("edge tts synthesis failed for voice `{voice}`"))?;
        Ok(result.audio)
    }
}

fn select_voice(language: Option<&str>) -> &'static str {
    match normalize_language(language).as_deref() {
        Some("zh") | Some("zh-cn") | Some("zh-hans") => "zh-CN-XiaoxiaoNeural",
        Some("zh-tw") | Some("zh-hant") => "zh-TW-HsiaoChenNeural",
        Some("en") | Some("en-us") => "en-US-JennyNeural",
        Some("en-gb") => "en-GB-SoniaNeural",
        Some("ja") => "ja-JP-NanamiNeural",
        Some("ko") => "ko-KR-SunHiNeural",
        Some("fr") => "fr-FR-DeniseNeural",
        Some("de") => "de-DE-KatjaNeural",
        Some("it") => "it-IT-ElsaNeural",
        Some("es") => "es-ES-ElviraNeural",
        Some("pt") | Some("pt-br") => "pt-BR-FranciscaNeural",
        Some("ru") => "ru-RU-SvetlanaNeural",
        Some("ar") => "ar-SA-ZariyahNeural",
        Some("th") => "th-TH-PremwadeeNeural",
        Some("vi") => "vi-VN-HoaiMyNeural",
        Some("id") => "id-ID-GadisNeural",
        Some("tr") => "tr-TR-EmelNeural",
        Some("pl") => "pl-PL-ZofiaNeural",
        Some("nl") => "nl-NL-ColetteNeural",
        Some("hi") => "hi-IN-SwaraNeural",
        Some("he") => "he-IL-HilaNeural",
        Some("el") => "el-GR-AthinaNeural",
        _ => "en-US-JennyNeural",
    }
}

fn normalize_language(language: Option<&str>) -> Option<String> {
    language
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_lowercase().replace('_', "-"))
}
