//! Application-level error type returned to the frontend.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FlickError {
    #[error("{0}")]
    Message(String),
}

impl From<anyhow::Error> for FlickError {
    fn from(value: anyhow::Error) -> Self {
        Self::Message(value.to_string())
    }
}

impl From<tauri::Error> for FlickError {
    fn from(value: tauri::Error) -> Self {
        Self::Message(value.to_string())
    }
}

impl From<tauri_plugin_autostart::Error> for FlickError {
    fn from(value: tauri_plugin_autostart::Error) -> Self {
        Self::Message(value.to_string())
    }
}

impl From<tauri_plugin_global_shortcut::Error> for FlickError {
    fn from(value: tauri_plugin_global_shortcut::Error) -> Self {
        Self::Message(value.to_string())
    }
}

impl serde::Serialize for FlickError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
