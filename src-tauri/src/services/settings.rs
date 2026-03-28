//! Small JSON-backed persistence for app settings.

use std::{fs, path::PathBuf};

use serde::{Serialize, de::DeserializeOwned};

use crate::models::AppSettings;

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load_settings(&self) -> anyhow::Result<AppSettings> {
        self.load_json().or_else(|error| {
            if self.path.exists() {
                Err(error)
            } else {
                Ok(AppSettings::default())
            }
        })
    }

    pub fn save_settings(&self, settings: &AppSettings) -> anyhow::Result<()> {
        self.save_json(settings)
    }

    fn load_json<T: DeserializeOwned>(&self) -> anyhow::Result<T> {
        let content = fs::read_to_string(&self.path)?;
        Ok(serde_json::from_str(&content)?)
    }

    fn save_json<T: Serialize>(&self, value: &T) -> anyhow::Result<()> {
        // Ensure the parent directory exists before we write the pretty-printed JSON file.
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, serde_json::to_vec_pretty(value)?)?;
        Ok(())
    }
}
