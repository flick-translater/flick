//! Provider registry and provider-specific configuration lookup.

use anyhow::{Result, anyhow};

use crate::models::{AISettings, ProviderSettings};

use super::protocol::ProtocolKind;

pub struct ResolvedProvider<'a> {
    pub key: &'static str,
    pub protocol: ProtocolKind,
    pub settings: &'a ProviderSettings,
}

pub fn resolve_provider<'a>(
    provider: &str,
    ai_settings: &'a AISettings,
) -> Result<ResolvedProvider<'a>> {
    match provider {
        "openai" => Ok(ResolvedProvider {
            key: "openai",
            protocol: ProtocolKind::OpenAi,
            settings: &ai_settings.openai,
        }),
        "openai_compatible" => Ok(ResolvedProvider {
            key: "openai_compatible",
            protocol: ProtocolKind::OpenAi,
            settings: &ai_settings.openai_compatible,
        }),
        "ollama" => Ok(ResolvedProvider {
            key: "ollama",
            protocol: ProtocolKind::OpenAi,
            settings: &ai_settings.ollama,
        }),
        "lmstudio" => Ok(ResolvedProvider {
            key: "lmstudio",
            protocol: ProtocolKind::OpenAi,
            settings: &ai_settings.lmstudio,
        }),
        "anthropic" => Ok(ResolvedProvider {
            key: "anthropic",
            protocol: ProtocolKind::Anthropic,
            settings: &ai_settings.anthropic,
        }),
        "anthropic_compatible" => Ok(ResolvedProvider {
            key: "anthropic_compatible",
            protocol: ProtocolKind::Anthropic,
            settings: &ai_settings.anthropic_compatible,
        }),
        other => Err(anyhow!("Unsupported AI provider: {other}")),
    }
}
