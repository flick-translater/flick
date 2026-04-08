use tauri::{App, AppHandle, Manager, RunEvent, Runtime, State, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt as _, ShortcutState};
use tauri::path::BaseDirectory;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LR_LOADFROMFILE, LoadImageW, SendMessageW,
    WM_SETICON,
};

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

pub fn show_translate_window_after_show(_app: &AppHandle) {}

pub fn configure_built_window(window: &WebviewWindow) {
    if let Err(error) = set_window_icons(window) {
        eprintln!("failed to set explicit Windows window icons: {error}");
    }
}

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
            if shortcut_event_matches_action(event.state, action) {
                crate::app::trigger_shortcut_action(app, action);
            }
        })?;

    Ok(())
}

fn shortcut_event_matches_action(state: ShortcutState, action: ShortcutAction) -> bool {
    match action {
        ShortcutAction::TranslateSelectedText => state == ShortcutState::Released,
        ShortcutAction::Capture | ShortcutAction::TranslateCapture => {
            state == ShortcutState::Pressed
        }
    }
}

fn set_window_icons(window: &WebviewWindow) -> anyhow::Result<()> {
    let icon_path = resolve_windows_icon_path(window)?;
    let wide_path = icon_path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();

    let big_icon = unsafe {
        LoadImageW(
            std::ptr::null_mut(),
            wide_path.as_ptr(),
            IMAGE_ICON,
            0,
            0,
            LR_LOADFROMFILE | LR_DEFAULTCOLOR,
        )
    };
    if big_icon.is_null() {
        anyhow::bail!("LoadImageW failed for {}", icon_path.display());
    }

    let hwnd = window.hwnd()?.0 as *mut std::ffi::c_void;
    unsafe {
        SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big_icon as isize);
        SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, big_icon as isize);
    }

    Ok(())
}

fn resolve_windows_icon_path(window: &WebviewWindow) -> anyhow::Result<std::path::PathBuf> {
    let app = window.app_handle();
    if let Ok(path) = app.path().resolve("icons/icon.ico", BaseDirectory::Resource) {
        if path.is_file() {
            return Ok(path);
        }
    }

    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dev_path = manifest_dir.join("icons/icon.ico");
    if dev_path.is_file() {
        return Ok(dev_path);
    }

    anyhow::bail!("could not resolve icons/icon.ico for Windows window icon")
}
