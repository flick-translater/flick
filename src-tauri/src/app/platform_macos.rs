use tauri::{ActivationPolicy, App, AppHandle, RunEvent, Runtime, State};

use crate::{
    app::{AppState, ShortcutAction},
    commands,
    error::FlickError,
    features::translation,
    models::AppSettings,
};

use super::{macos_hotkeys, macos_permissions};

pub fn configure_app_setup(app: &mut App) {
    app.set_activation_policy(ActivationPolicy::Accessory);
    let _ = macos_permissions::request_startup_permissions();
}

pub fn handle_run_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    if let RunEvent::Reopen { .. } = event {
        let state = app.state::<AppState>();
        if let Ok(mut suppress) = state.suppress_next_reopen.lock() {
            if *suppress {
                *suppress = false;
                return;
            }
        }
        let _ = crate::app::windows::show_main_window(app);
    }
}

pub fn register_platform_shortcuts(app: &AppHandle) -> anyhow::Result<()> {
    let permissions = macos_permissions::current_permission_status();
    if permissions.hotkeys_ready() {
        macos_hotkeys::install_hotkey_tap(app)?;
    } else {
        eprintln!(
            "skipping macOS hotkey event tap during startup because accessibility/input monitoring permissions are not ready"
        );
    }
    Ok(())
}

pub fn apply_shortcut_bindings(app: &AppHandle, settings: &AppSettings) -> anyhow::Result<()> {
    let _ = app;
    macos_hotkeys::apply_shortcuts(
        settings.capture_shortcut.as_str(),
        settings.translate_shortcut.as_str(),
        settings.selected_translate_shortcut.as_str(),
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
    let _ = app;
    let _ = state;
    macos_hotkeys::set_recording_paused(recording)
        .map_err(|error| FlickError::Message(format!("切换快捷键录制状态失败: {error}")))
}
