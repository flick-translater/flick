use tauri::{
    ActivationPolicy, App, AppHandle, LogicalPosition, Manager, RunEvent, Runtime, State,
    TitleBarStyle, WebviewWindowBuilder,
};

use crate::{
    app::{AppState, ShortcutAction},
    commands,
    error::FlickError,
    features::translation,
    models::AppSettings,
};

use super::{macos_hotkeys, macos_permissions};
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

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

pub fn on_main_window_close(app: &AppHandle) {
    let _ = app.hide();
    let _ = app.set_activation_policy(ActivationPolicy::Accessory);
}

pub fn show_main_window_before_focus(app: &AppHandle) {
    let _ = app.set_activation_policy(ActivationPolicy::Regular);
    let _ = app.show();
    let _ = app.set_dock_visibility(true);
}

pub fn configure_main_window_builder<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    builder
        .hidden_title(true)
        .title_bar_style(TitleBarStyle::Overlay)
        .traffic_light_position(LogicalPosition::new(16.0, 18.0))
}

pub fn show_translate_window_before_focus(app: &AppHandle) {
    remember_previous_frontmost_app(app);
}

pub fn refresh_previous_frontmost_app(app: &AppHandle) {
    remember_previous_frontmost_app(app);
}

pub fn hide_translate_window_before_hide(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut suppress) = state.suppress_next_reopen.lock() {
            *suppress = true;
        }
    }
    restore_previous_frontmost_app(app);
}

pub fn hide_translate_window_after_hide(app: &AppHandle) {
    if let Some(main_window) = app.get_webview_window("main") {
        let is_visible = main_window.is_visible().unwrap_or(false);
        if !is_visible {
            let _ = app.set_activation_policy(ActivationPolicy::Accessory);
            let _ = app.hide();
        }
    } else {
        let _ = app.set_activation_policy(ActivationPolicy::Accessory);
        let _ = app.hide();
    }
}

fn remember_previous_frontmost_app(app: &AppHandle) {
    let workspace = NSWorkspace::sharedWorkspace();
    let current_app = NSRunningApplication::currentApplication();
    let current_pid = current_app.processIdentifier();
    let previous_pid = workspace
        .frontmostApplication()
        .map(|frontmost| frontmost.processIdentifier())
        .filter(|pid| *pid > 0 && *pid != current_pid);

    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut stored_pid) = state.previous_frontmost_app_pid.lock() {
            *stored_pid = previous_pid;
        }
    }
}

fn restore_previous_frontmost_app(app: &AppHandle) {
    let previous_pid = app.try_state::<AppState>().and_then(|state| {
        state
            .previous_frontmost_app_pid
            .lock()
            .ok()
            .and_then(|mut stored_pid| stored_pid.take())
    });

    let Some(previous_pid) = previous_pid else {
        return;
    };

    let current_app = NSRunningApplication::currentApplication();
    let current_pid = current_app.processIdentifier();
    let frontmost_pid = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|frontmost| frontmost.processIdentifier());

    if frontmost_pid.is_some_and(|pid| pid > 0 && pid != current_pid) {
        return;
    }

    if let Some(previous_app) =
        NSRunningApplication::runningApplicationWithProcessIdentifier(previous_pid)
    {
        let _ = previous_app.activateWithOptions(NSApplicationActivationOptions(0));
    }
}
