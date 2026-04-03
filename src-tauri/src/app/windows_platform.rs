use tauri::{App, AppHandle, Manager, RunEvent, Runtime, State, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt as _, ShortcutState};

use crate::{
    app::{AppState, ShortcutAction},
    commands,
    error::FlickError,
    features::translation,
    models::AppSettings,
};

pub fn configure_app_setup(_app: &mut App) {}

pub fn handle_run_event<R: Runtime>(_app: &AppHandle<R>, _event: &RunEvent) {}

pub fn register_platform_shortcuts(_app: &AppHandle) -> anyhow::Result<()> {
    Ok(())
}

pub fn apply_shortcut_bindings(app: &AppHandle, settings: &AppSettings) -> anyhow::Result<()> {
    let global_shortcut = app.global_shortcut();

    for shortcut in [
        &settings.capture_shortcut,
        &settings.translate_shortcut,
        &settings.selected_translate_shortcut,
    ] {
        if global_shortcut.is_registered(shortcut.as_str()) {
            global_shortcut.unregister(shortcut.as_str())?;
        }
    }

    register_shortcut_handler(
        app,
        settings.capture_shortcut.as_str(),
        ShortcutAction::Capture,
    )?;
    register_shortcut_handler(
        app,
        settings.translate_shortcut.as_str(),
        ShortcutAction::TranslateCapture,
    )?;
    register_shortcut_handler(
        app,
        settings.selected_translate_shortcut.as_str(),
        ShortcutAction::TranslateSelectedText,
    )?;

    Ok(())
}

pub fn trigger_shortcut_action(app: &AppHandle, action: ShortcutAction) {
    match action {
        ShortcutAction::Capture => {
            let state = app.state::<AppState>();
            let _ = commands::capture::begin_capture_session(app, &state);
        }
        ShortcutAction::TranslateCapture => {
            let state = app.state::<AppState>();
            let _ = commands::capture::begin_capture_session_with_intent(
                app,
                &state,
                crate::app::CaptureIntent::Translate,
            );
        }
        ShortcutAction::TranslateSelectedText => {
            if let Err(error) = translation::translate_selected_text_to_window(app) {
                eprintln!("selected text shortcut failed: {error}");
            }
        }
    }
}

pub fn set_shortcut_recording(
    app: &AppHandle,
    state: &State<'_, AppState>,
    recording: bool,
) -> Result<(), FlickError> {
    let settings = state
        .settings
        .lock()
        .map_err(|_| FlickError::Message("settings mutex poisoned".into()))?
        .clone();
    let global_shortcut = app.global_shortcut();

    for shortcut in [
        &settings.capture_shortcut,
        &settings.translate_shortcut,
        &settings.selected_translate_shortcut,
    ] {
        if recording {
            if global_shortcut.is_registered(shortcut.as_str()) {
                global_shortcut.unregister(shortcut.as_str())?;
            }
        } else if !global_shortcut.is_registered(shortcut.as_str()) {
            apply_shortcut_bindings(app, &settings)
                .map_err(|error| FlickError::Message(format!("恢复快捷键失败: {error}")))?;
            break;
        }
    }

    Ok(())
}

pub fn on_main_window_close(_app: &AppHandle) {}

pub fn show_main_window_before_focus(_app: &AppHandle) {}

pub fn configure_main_window_builder<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    builder
}

pub fn show_translate_window_before_focus(_app: &AppHandle) {}

pub fn refresh_previous_frontmost_app(_app: &AppHandle) {}

pub fn hide_translate_window_before_hide(_app: &AppHandle) {}

pub fn hide_translate_window_after_hide(_app: &AppHandle) {}

fn register_shortcut_handler(
    app: &AppHandle,
    shortcut: &str,
    action: ShortcutAction,
) -> anyhow::Result<()> {
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _, event| {
            if event.state == ShortcutState::Pressed {
                trigger_shortcut_action(app, action);
            }
        })?;

    Ok(())
}
