use std::thread;

use tauri::{AppHandle, Emitter, Manager};
use tokio::runtime::Runtime;

use crate::{
    app::{AppState, windows},
    error::FlickError,
    models::{
        AISettings, TranslateRequest, TranslateResponse, TranslateWindowState, TranslationHistory,
    },
    services::{NewTranslationRecord, TranslationGateway, read_selected_text},
};

#[derive(Debug, Clone)]
pub struct TranslationPipeline {
    pub request: TranslateRequest,
    pub image_path: Option<String>,
}

impl TranslationPipeline {
    pub fn new(request: TranslateRequest) -> Self {
        Self {
            request,
            image_path: None,
        }
    }

    pub fn with_image_path(mut self, image_path: impl Into<String>) -> Self {
        self.image_path = Some(image_path.into());
        self
    }

    pub fn prepare(mut self) -> Self {
        self.request = prepare_request(self.request);
        self
    }
}

pub async fn run(
    state: &AppState,
    request: TranslateRequest,
) -> Result<TranslateResponse, FlickError> {
    let pipeline = TranslationPipeline::new(request).prepare();
    let ai_settings = state
        .settings
        .lock()
        .map_err(|_| FlickError::LockError("settings".into()))?
        .ai
        .clone();
    run_pipeline_with_ai_settings(&ai_settings, &pipeline).await
}

pub async fn run_pipeline_with_ai_settings(
    ai_settings: &AISettings,
    pipeline: &TranslationPipeline,
) -> Result<TranslateResponse, FlickError> {
    let response = TranslationGateway::new(ai_settings.clone())
        .translate(pipeline.request.clone())
        .await
        .map_err(FlickError::from)?;
    Ok(finalize_response(&pipeline.request, response))
}

pub fn save_pipeline_history(
    state: &AppState,
    pipeline: &TranslationPipeline,
    response: &TranslateResponse,
) -> Result<(), FlickError> {
    save_history(
        state,
        &pipeline.request,
        response,
        pipeline.image_path.as_deref(),
    )
}

pub fn save_history(
    state: &AppState,
    request: &TranslateRequest,
    response: &TranslateResponse,
    image_path: Option<&str>,
) -> Result<(), FlickError> {
    state
        .translation_history_store
        .insert_record(NewTranslationRecord {
            source_text: &request.text,
            translated_text: &response.translated_text,
            source_language: response
                .detected_source_language
                .as_deref()
                .or(request.source_language.as_deref()),
            target_language: &request.target_language,
            provider: &response.provider,
            image_path,
        })
        .map_err(Into::into)
}

pub fn list_history(state: &AppState) -> Result<TranslationHistory, FlickError> {
    Ok(TranslationHistory {
        database_path: state
            .translation_history_store
            .db_path()
            .display()
            .to_string(),
        items: state.translation_history_store.list_records()?,
    })
}

pub fn clear_history(state: &AppState) -> Result<(), FlickError> {
    state.translation_history_store.clear().map_err(Into::into)
}

pub fn delete_history_record(state: &AppState, id: i64) -> Result<(), FlickError> {
    state
        .translation_history_store
        .delete_record(id)
        .map_err(Into::into)
}

pub fn prepare_request(mut request: TranslateRequest) -> TranslateRequest {
    if request.source_language.is_none() {
        request.source_language = detect_text_language(&request.text);
    }
    request
}

pub fn finalize_response(
    request: &TranslateRequest,
    mut response: TranslateResponse,
) -> TranslateResponse {
    if response.detected_source_language.is_none() {
        response.detected_source_language = request
            .source_language
            .clone()
            .or_else(|| detect_text_language(&request.text));
    }
    response
}

pub fn detect_text_language(text: &str) -> Option<String> {
    let mut han_count = 0;
    let mut kana_count = 0;
    let mut hangul_count = 0;
    let mut latin_count = 0;
    let mut japanese_marker_count = 0;
    let mut arabic_count = 0;
    let mut cyrillic_count = 0;
    let mut thai_count = 0;
    let mut hebrew_count = 0;
    let mut greek_count = 0;
    let mut devanagari_count = 0;
    let mut german_char_count = 0;
    let mut french_char_count = 0;
    let mut italian_char_count = 0;
    let mut dutch_char_count = 0;

    for ch in text.chars() {
        match ch {
            '\u{3040}'..='\u{30ff}' => kana_count += 1,
            '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' => {
                han_count += 1;
            }
            '\u{ac00}'..='\u{d7af}' => hangul_count += 1,
            '\u{0600}'..='\u{06ff}' | '\u{0750}'..='\u{077f}' | '\u{08a0}'..='\u{08ff}' => {
                arabic_count += 1;
            }
            '\u{0400}'..='\u{04ff}' | '\u{0500}'..='\u{052f}' => cyrillic_count += 1,
            '\u{0e00}'..='\u{0e7f}' => thai_count += 1,
            '\u{0590}'..='\u{05ff}' => hebrew_count += 1,
            '\u{0370}'..='\u{03ff}' => greek_count += 1,
            '\u{0900}'..='\u{097f}' => devanagari_count += 1,
            'A'..='Z' | 'a'..='z' => latin_count += 1,
            '。' | '、' | '「' | '」' | '『' | '』' | '〜' | '々' => {
                japanese_marker_count += 1;
            }
            _ => {}
        }

        if matches!(ch, 'ä' | 'ö' | 'ü' | 'Ä' | 'Ö' | 'Ü' | 'ß') {
            german_char_count += 1;
        }

        if matches!(
            ch,
            'à' | 'â'
                | 'æ'
                | 'ç'
                | 'è'
                | 'é'
                | 'ê'
                | 'ë'
                | 'î'
                | 'ï'
                | 'ô'
                | 'œ'
                | 'ù'
                | 'û'
                | 'ü'
                | 'ÿ'
                | 'À'
                | 'Â'
                | 'Æ'
                | 'Ç'
                | 'È'
                | 'É'
                | 'Ê'
                | 'Ë'
                | 'Î'
                | 'Ï'
                | 'Ô'
                | 'Œ'
                | 'Ù'
                | 'Û'
                | 'Ü'
                | 'Ÿ'
        ) {
            french_char_count += 1;
        }

        if matches!(
            ch,
            'à' | 'è'
                | 'é'
                | 'ì'
                | 'í'
                | 'î'
                | 'ò'
                | 'ó'
                | 'ù'
                | 'ú'
                | 'À'
                | 'È'
                | 'É'
                | 'Ì'
                | 'Í'
                | 'Î'
                | 'Ò'
                | 'Ó'
                | 'Ù'
                | 'Ú'
        ) {
            italian_char_count += 1;
        }

        if matches!(ch, 'ĳ' | 'Ĳ') {
            dutch_char_count += 1;
        }
    }

    if kana_count > 0 || japanese_marker_count > 0 {
        return Some("ja".into());
    }

    if hangul_count > 0 {
        return Some("ko".into());
    }

    if arabic_count > 0 {
        return Some("ar".into());
    }

    if cyrillic_count > 0 {
        return Some("ru".into());
    }

    if thai_count > 0 {
        return Some("th".into());
    }

    if hebrew_count > 0 {
        return Some("he".into());
    }

    if greek_count > 0 {
        return Some("el".into());
    }

    if devanagari_count > 0 {
        return Some("hi".into());
    }

    if han_count > 0 {
        return Some("zh".into());
    }

    if latin_count > 0 {
        let lower = text.to_lowercase();
        let normalized = lower
            .split(|c: char| !c.is_alphabetic() && c != '\'' && c != '’')
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();

        let score_tokens = |tokens: &[&str], dictionary: &[&str]| -> usize {
            tokens
                .iter()
                .filter(|token| dictionary.contains(token))
                .count()
        };

        let german_score = german_char_count
            + score_tokens(
                &normalized,
                &["der", "die", "das", "und", "nicht", "ist", "ich"],
            );
        let french_score = french_char_count
            + score_tokens(
                &normalized,
                &[
                    "le", "la", "les", "des", "une", "est", "pas", "pour", "avec",
                ],
            );
        let italian_score = italian_char_count
            + score_tokens(
                &normalized,
                &["il", "lo", "gli", "che", "non", "per", "con", "una", "sono"],
            );
        let dutch_score = dutch_char_count
            + score_tokens(
                &normalized,
                &[
                    "de", "het", "een", "van", "niet", "met", "voor", "zijn", "dat",
                ],
            );

        let mut best = ("en", 0_usize);
        for candidate in [
            ("de", german_score),
            ("fr", french_score),
            ("it", italian_score),
            ("nl", dutch_score),
        ] {
            if candidate.1 > best.1 {
                best = candidate;
            }
        }

        if best.1 > 0 {
            return Some(best.0.into());
        }
    }

    if latin_count > 0 {
        return Some("en".into());
    }

    None
}

pub fn show_window_immediately(app: &AppHandle, image_path: &str) -> Result<(), FlickError> {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut snapshot) = state.translate_window_state.lock() {
            *snapshot = TranslateWindowState {
                image_path: image_path.to_string(),
                source_text: String::new(),
                translated_text: String::new(),
                provider: String::new(),
                detected_source_language: None,
                ocr_detected_source_language: None,
                target_language: "zh".into(),
                is_loading: true,
                is_translating: false,
            };
        }
    }

    windows::ensure_translate_window(app)?;
    windows::show_translate_window(app)?;

    let payload = serde_json::json!({
        "imagePath": image_path,
        "loading": true,
    });

    if let Some(window) = app.get_webview_window("translate") {
        let _ = window.emit("ocr-loading", payload.clone());
    }

    let _ = app.emit("ocr-loading", payload);

    Ok(())
}

pub fn emit_ocr_ready(
    app: &AppHandle,
    image_path: &str,
    source_text: &str,
    ocr_detected_source_language: Option<&str>,
    auto_translate_enabled: bool,
    target_language: &str,
) -> Result<(), FlickError> {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut snapshot) = state.translate_window_state.lock() {
            snapshot.image_path = image_path.to_string();
            snapshot.source_text = source_text.to_string();
            snapshot.translated_text.clear();
            snapshot.provider.clear();
            snapshot.detected_source_language = None;
            snapshot.ocr_detected_source_language =
                ocr_detected_source_language.map(str::to_string);
            snapshot.target_language = target_language.to_string();
            snapshot.is_loading = false;
            snapshot.is_translating = auto_translate_enabled;
        }
    }

    let payload = serde_json::json!({
        "imagePath": image_path,
        "sourceText": source_text,
        "ocrDetectedSourceLanguage": ocr_detected_source_language,
        "autoTranslateEnabled": auto_translate_enabled,
        "targetLanguage": target_language,
    });

    if let Some(window) = app.get_webview_window("translate") {
        let _ = window.emit("ocr-ready", payload.clone());
    }

    let _ = app.emit("ocr-ready", payload);

    Ok(())
}

pub fn emit_translation_ready(
    app: &AppHandle,
    image_path: &str,
    source_text: &str,
    target_language: &str,
    translation: TranslateResponse,
) -> Result<(), FlickError> {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut snapshot) = state.translate_window_state.lock() {
            snapshot.image_path = image_path.to_string();
            snapshot.source_text = source_text.to_string();
            snapshot.translated_text = translation.translated_text.clone();
            snapshot.provider = translation.provider.clone();
            snapshot.detected_source_language = translation.detected_source_language.clone();
            snapshot.target_language = target_language.to_string();
            snapshot.is_loading = false;
            snapshot.is_translating = false;
        }
    }

    let payload = serde_json::json!({
        "imagePath": image_path,
        "sourceText": source_text,
        "translatedText": translation.translated_text,
        "provider": translation.provider,
        "detectedSourceLanguage": translation.detected_source_language,
        "targetLanguage": target_language,
    });

    if let Some(window) = app.get_webview_window("translate") {
        let _ = window.emit("translation-ready", payload.clone());
    }

    let _ = app.emit("translation-ready", payload);

    Ok(())
}

pub fn show_text_translation_loading(
    app: &AppHandle,
    source_text: &str,
    detected_source_language: Option<&str>,
    target_language: &str,
) -> Result<(), FlickError> {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut snapshot) = state.translate_window_state.lock() {
            *snapshot = TranslateWindowState {
                image_path: String::new(),
                source_text: source_text.to_string(),
                translated_text: String::new(),
                provider: String::new(),
                detected_source_language: None,
                ocr_detected_source_language: detected_source_language.map(str::to_string),
                target_language: target_language.to_string(),
                is_loading: false,
                is_translating: true,
            };
        }
    }

    windows::ensure_translate_window(app)?;
    windows::show_translate_window(app)?;
    Ok(())
}

pub fn translate_selected_text_to_window(app: &AppHandle) -> Result<(), FlickError> {
    let selected_text = read_selected_text()
        .map_err(|error| FlickError::Message(format!("读取选中文本失败: {error}")))?;

    let target_language = {
        let state = app.state::<AppState>();
        state
            .settings
            .lock()
            .map_err(|_| FlickError::LockError("settings".into()))?
            .ocr_target_language
            .clone()
    };
    let pipeline = TranslationPipeline::new(TranslateRequest {
        text: selected_text.clone(),
        source_language: None,
        target_language: target_language.clone(),
    })
    .prepare();

    show_text_translation_loading(
        app,
        &selected_text,
        pipeline.request.source_language.as_deref(),
        &target_language,
    )?;

    let app_handle = app.clone();
    thread::spawn(move || {
        let run = || -> Result<(), FlickError> {
            let runtime = Runtime::new().map_err(|error| {
                FlickError::Message(format!("failed to create tokio runtime: {error}"))
            })?;
            let translation = runtime.block_on(async {
                let state = app_handle.state::<AppState>();
                let ai_settings = state
                    .settings
                    .lock()
                    .map_err(|_| FlickError::LockError("settings".into()))?
                    .ai
                    .clone();
                run_pipeline_with_ai_settings(&ai_settings, &pipeline).await
            })?;

            let state = app_handle.state::<AppState>();
            save_pipeline_history(&state, &pipeline, &translation)?;
            emit_translation_ready(
                &app_handle,
                "",
                &selected_text,
                &target_language,
                translation,
            )?;
            let _ = app_handle.emit("translation-history-updated", ());

            Ok(())
        };

        if let Err(error) = run() {
            eprintln!("selected text translation failed: {error}");

            if let Some(state) = app_handle.try_state::<AppState>() {
                if let Ok(mut snapshot) = state.translate_window_state.lock() {
                    snapshot.is_loading = false;
                    snapshot.is_translating = false;
                }
            }
        }
    });

    Ok(())
}
