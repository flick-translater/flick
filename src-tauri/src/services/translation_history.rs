use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};

use crate::models::TranslationRecord;

#[derive(Debug, Clone)]
pub struct TranslationHistoryStore {
    db_path: PathBuf,
}

impl TranslationHistoryStore {
    pub fn new(db_path: PathBuf) -> anyhow::Result<Self> {
        let store = Self { db_path };
        store.initialize()?;
        Ok(store)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn insert_record(&self, record: NewTranslationRecord<'_>) -> anyhow::Result<()> {
        let connection = self.open()?;
        let normalized_source_text = normalize_source_text(record.source_text);
        connection
            .execute(
                "INSERT INTO translation_history (
                    created_at,
                    source_text,
                    translated_text,
                    source_language,
                    target_language,
                    provider,
                    image_path
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    Utc::now(),
                    normalized_source_text,
                    record.translated_text,
                    record.source_language,
                    record.target_language,
                    record.provider,
                    record.image_path,
                ],
            )
            .context("failed to insert translation record")?;

        Ok(())
    }

    pub fn list_records(&self) -> anyhow::Result<Vec<TranslationRecord>> {
        let connection = self.open()?;
        let mut statement = connection
            .prepare(
                "SELECT
                    id,
                    created_at,
                    source_text,
                    translated_text,
                    source_language,
                    target_language,
                    provider,
                    image_path
                 FROM translation_history
                 ORDER BY created_at DESC, id DESC",
            )
            .context("failed to prepare translation history query")?;

        let rows = statement
            .query_map([], |row| {
                Ok(TranslationRecord {
                    id: row.get(0)?,
                    created_at: row.get::<_, DateTime<Utc>>(1)?,
                    source_text: normalize_source_text(&row.get::<_, String>(2)?),
                    translated_text: row.get(3)?,
                    source_language: row.get(4)?,
                    target_language: row.get(5)?,
                    provider: row.get(6)?,
                    image_path: row.get(7)?,
                })
            })
            .context("failed to query translation history")?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.context("failed to decode translation history row")?);
        }

        Ok(records)
    }

    pub fn clear(&self) -> anyhow::Result<()> {
        let connection = self.open()?;
        connection
            .execute("DELETE FROM translation_history", [])
            .context("failed to clear translation history")?;
        Ok(())
    }

    pub fn delete_record(&self, id: i64) -> anyhow::Result<()> {
        let connection = self.open()?;
        connection
            .execute("DELETE FROM translation_history WHERE id = ?1", params![id])
            .context("failed to delete translation history record")?;
        Ok(())
    }

    fn initialize(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent).context("failed to create translation db directory")?;
        }

        let connection = self.open()?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS translation_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    created_at TEXT NOT NULL,
                    source_text TEXT NOT NULL,
                    translated_text TEXT NOT NULL,
                    source_language TEXT,
                    target_language TEXT NOT NULL,
                    provider TEXT NOT NULL,
                    image_path TEXT
                );",
            )
            .context("failed to initialize translation history database")?;

        Ok(())
    }

    fn open(&self) -> anyhow::Result<Connection> {
        Connection::open(&self.db_path).context("failed to open translation history database")
    }
}

pub struct NewTranslationRecord<'a> {
    pub source_text: &'a str,
    pub translated_text: &'a str,
    pub source_language: Option<&'a str>,
    pub target_language: &'a str,
    pub provider: &'a str,
    pub image_path: Option<&'a str>,
}

fn normalize_source_text(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
