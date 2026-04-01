use std::{sync::Arc, thread};

use chrono::Utc;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::{
    app::{windows::emit_capture_status, AppState, CaptureIntent},
    error::FlickError,
    features::translation,
    models::{CaptureRecord, SelectionRect, TranslateRequest},
    services::{OcrService, ScreenCaptureService},
};

use super::{history, platform};

fn detect_ocr_language(text: &str) -> Option<String> {
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
            'à'
                | 'â'
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
            'à'
                | 'è'
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
            + score_tokens(&normalized, &["der", "die", "das", "und", "nicht", "ist", "ich"]);
        let french_score = french_char_count
            + score_tokens(
                &normalized,
                &["le", "la", "les", "des", "une", "est", "pas", "pour", "avec"],
            );
        let italian_score = italian_char_count
            + score_tokens(
                &normalized,
                &["il", "lo", "gli", "che", "non", "per", "con", "una", "sono"],
            );
        let dutch_score = dutch_char_count
            + score_tokens(
                &normalized,
                &["de", "het", "een", "van", "niet", "met", "voor", "zijn", "dat"],
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

pub fn cancel_capture(app: &AppHandle) -> Result<(), FlickError> {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut guard) = state.capture_snapshots.lock() {
            guard.clear();
        }
        platform::cancel_interactive_capture(app, &state);
        platform::cleanup_after_cancel(app, &state);
    }

    emit_capture_status(app, "capture-cancelled", "cancelled");
    Ok(())
}

pub fn complete_capture(
    app: &AppHandle,
    state: &State<'_, AppState>,
    selection: SelectionRect,
) -> Result<(), FlickError> {
    let screenshot_dir = history::current_screenshot_dir(state)?;
    let intent = *state
        .capture_intent
        .lock()
        .map_err(|_| FlickError::Message("capture intent mutex poisoned".into()))?;

    let ocr_service: Arc<dyn OcrService> = state
        .ocr_service
        .lock()
        .map_err(|_| FlickError::Message("ocr service mutex poisoned".into()))?
        .clone();
    let translation_service = state.translation_service.clone();
    let cached_screens = platform::complete_ui_before_capture_processing(app, state)?;

    let app_handle = app.clone();
    let should_restore_previous_frontmost = intent == CaptureIntent::Capture;
    thread::spawn(move || {
        let run = || -> Result<(), FlickError> {
            let capture_service = ScreenCaptureService::default();
            let image = platform::capture_image(&capture_service, &selection, &cached_screens)?;

            let state = app_handle.state::<AppState>();
            platform::finalize_capture_session(
                &app_handle,
                &state,
                should_restore_previous_frontmost,
            );

            let id = Uuid::new_v4().to_string();
            let created_at = Utc::now();
            let path = screenshot_dir.join(format!(
                "{}-{}.png",
                created_at.format("%Y%m%d-%H%M%S"),
                &id[..8]
            ));

            let record = CaptureRecord {
                id: id.clone(),
                created_at,
                width: image.width(),
                height: image.height(),
                path: path.display().to_string(),
            };

            let max_screenshots = state
                .settings
                .lock()
                .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
                .max_screenshots;

            if intent == CaptureIntent::Translate {
                if let Err(e) = translation::show_window_immediately(&app_handle, &record.path) {
                    eprintln!("Failed to show window: {}", e);
                }

                let ocr_result = {
                    let mut image_bytes = Vec::new();
                    image
                        .write_to(
                            &mut std::io::Cursor::new(&mut image_bytes),
                            image::ImageFormat::Png,
                        )
                        .map_err(|e| FlickError::Message(format!("failed to encode PNG: {}", e)))?;

                    ocr_service.run_with_data(&image_bytes)
                };

                let save_path = path.clone();
                let save_app = app_handle.clone();
                let save_screenshot_dir = screenshot_dir.clone();
                let save_max_screenshots = max_screenshots;
                let save_record = record.clone();
                thread::spawn(move || {
                    let capture_service = ScreenCaptureService::default();
                    if let Err(e) = capture_service.save_png(&image, &save_path) {
                        eprintln!("Failed to save image: {}", e);
                    }

                    if let Err(error) = capture_service.copy_to_clipboard(&image) {
                        eprintln!("failed to write screenshot to clipboard: {error}");
                    }

                    if let Err(e) =
                        history::prune_capture_history(&save_screenshot_dir, save_max_screenshots)
                    {
                        eprintln!("Failed to prune history: {}", e);
                    }

                    let state = save_app.state::<AppState>();
                    if let Ok(mut history_guard) = state.history.lock() {
                        history_guard.push_front(save_record.clone());
                        history_guard.truncate(save_max_screenshots as usize);
                    }

                    emit_capture_status(&save_app, "capture-finished", &save_record);
                });

                match ocr_result {
                    Ok(ocr) => {
                        let detected_source_language = detect_ocr_language(&ocr.text);
                        translation::emit_ocr_ready(
                            &app_handle,
                            &record.path,
                            &ocr.text,
                            detected_source_language.as_deref(),
                        )?;

                        let translation_result = translation::run_with_service(
                            translation_service.as_ref(),
                            TranslateRequest {
                                text: ocr.text.clone(),
                                source_language: detected_source_language.clone(),
                                target_language: "zh".into(),
                            },
                        );

                        match translation_result {
                            Ok(translation) => {
                                translation::emit_translation_ready(
                                    &app_handle,
                                    &record.path,
                                    &ocr.text,
                                    translation,
                                )?;
                            }
                            Err(e) => {
                                eprintln!("translation failed: {}", e);
                                return Err(e.into());
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("OCR failed: {}", e);
                        return Err(e.into());
                    }
                }
            } else {
                capture_service.save_png(&image, &path)?;

                if let Err(error) = capture_service.copy_to_clipboard(&image) {
                    eprintln!("failed to write screenshot to clipboard: {error}");
                }
                history::prune_capture_history(&screenshot_dir, max_screenshots)?;

                let mut history_guard = state
                    .history
                    .lock()
                    .map_err(|_| FlickError::Message("history mutex poisoned".into()))?;
                history_guard.push_front(record.clone());
                history_guard.truncate(max_screenshots as usize);
                drop(history_guard);

                emit_capture_status(&app_handle, "capture-finished", &record);
            }
            Ok(())
        };

        if let Err(error) = run() {
            eprintln!("capture process failed: {}", error);
            let state = app_handle.state::<AppState>();
            platform::restore_after_failed_capture(
                &app_handle,
                &state,
                should_restore_previous_frontmost,
            );
            emit_capture_status(&app_handle, "capture-error", error.to_string());
        }
    });

    Ok(())
}

pub fn begin_capture_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), FlickError> {
    begin_capture_session_with_intent(app, state, CaptureIntent::Capture)
}

pub fn begin_capture_session_with_intent(
    app: &AppHandle,
    state: &State<'_, AppState>,
    intent: CaptureIntent,
) -> Result<(), FlickError> {
    {
        let mut guard = state
            .capture_intent
            .lock()
            .map_err(|_| FlickError::Message("capture intent mutex poisoned".into()))?;
        *guard = intent;
    }

    platform::prepare_for_capture_session(app, state)?;
    platform::start_interactive_capture(app, state)?;
    Ok(())
}
