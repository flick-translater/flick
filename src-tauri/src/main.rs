#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod error;
mod models;
mod services;

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use commands::{
    cancel_capture, clear_all_captures, close_translation_widget, complete_capture, delete_capture,
    focus_capture_window, get_app_settings, get_autostart_status, get_capture_context,
    get_global_cursor_position,
    get_storage_info,
    refresh_capture_context,
    get_translation_widget_pinned, list_capture_history, minimize_translation_widget, mock_ocr,
    mock_translate, open_file_in_default_app, pick_screenshot_directory, read_image_as_data_url,
    set_autostart_enabled, set_shortcut_recording, set_translation_widget_pinned,
    show_translation_widget, start_capture, update_capture_shortcut, update_interface_language,
    update_max_screenshots, update_screenshot_directory, update_translate_shortcut,
};
use models::{AppSettings, CaptureContexts, CaptureRecord};
use services::{
    CachedScreenCapture, MockOcrService, MockTranslationService, ScreenCaptureService,
    SettingsStore,
};
use tauri::{
    ActivationPolicy, AppHandle, Emitter, LogicalPosition, Manager, RunEvent, TitleBarStyle,
    WebviewUrl, WebviewWindowBuilder,
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuEvent, MenuId, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_global_shortcut::{GlobalShortcutExt as _, ShortcutState};

const MAIN_WINDOW_LABEL: &str = "main";
const CAPTURE_WINDOW_LABEL_PREFIX: &str = "capture";
const WIDGET_WINDOW_LABEL: &str = "widget";
pub struct AppState {
    pub capture_contexts: Mutex<CaptureContexts>,
    pub capture_snapshots: Mutex<Vec<CachedScreenCapture>>,
    pub history: Mutex<VecDeque<CaptureRecord>>,
    pub data_dir: PathBuf,
    pub screenshot_dir: Mutex<PathBuf>,
    pub settings_store: SettingsStore,
    pub settings: Mutex<AppSettings>,
    pub capture_intent: Mutex<CaptureIntent>,
    #[cfg(target_os = "macos")]
    pub capture_previous_frontmost_pid: Mutex<Option<i32>>,
    #[cfg(target_os = "macos")]
    pub capture_main_window_suppressed: Mutex<bool>,
    pub capture_service: ScreenCaptureService,
    pub ocr_service: Arc<MockOcrService>,
    pub translation_service: Arc<MockTranslationService>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureIntent {
    Capture,
    Translate,
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            app.set_activation_policy(ActivationPolicy::Regular);
            ensure_main_window(app.handle())?;
            initialize_capture_windows(app.handle())?;
            ensure_widget_window(app.handle())?;
            let state = build_state(app.handle())?;
            app.manage(state);

            setup_tray(app.handle())?;
            register_shortcuts(app.handle())?;
            show_main_window(app.handle())?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_capture,
            focus_capture_window,
            cancel_capture,
            complete_capture,
            get_global_cursor_position,
            delete_capture,
            clear_all_captures,
            refresh_capture_context,
            get_capture_context,
            list_capture_history,
            get_storage_info,
            pick_screenshot_directory,
            open_file_in_default_app,
            read_image_as_data_url,
            get_app_settings,
            get_autostart_status,
            set_autostart_enabled,
            set_shortcut_recording,
            show_translation_widget,
            get_translation_widget_pinned,
            set_translation_widget_pinned,
            minimize_translation_widget,
            close_translation_widget,
            update_capture_shortcut,
            update_interface_language,
            update_max_screenshots,
            update_screenshot_directory,
            update_translate_shortcut,
            mock_ocr,
            mock_translate,
        ])
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(handle_tray_event)
        .build(tauri::generate_context!())
        .expect("failed to build Flick application");

    app.run(|app, event| match event {
        RunEvent::Ready => {
            let _ = show_main_window(app);
        }
        RunEvent::Reopen { .. } => {
            let _ = show_main_window(app);
        }
        _ => {}
    });
}

fn build_state(app: &AppHandle) -> anyhow::Result<AppState> {
    let data_dir = app
        .path()
        .app_cache_dir()
        .or_else(|_| {
            dirs::cache_dir()
                .map(|dir| dir.join("Flick"))
                .ok_or_else(|| tauri::Error::AssetNotFound("cache directory".into()))
        })
        .map_err(anyhow::Error::from)?;

    let settings_store = SettingsStore::new(data_dir.join("settings.json"));
    let mut settings = settings_store.load_settings()?;
    if !settings.interface_language_set {
        settings.interface_language = detect_system_language();
        settings.interface_language_set = false;
        settings_store.save_settings(&settings)?;
    }
    let default_screenshot_dir = data_dir.join("captures");
    let screenshot_dir = if settings.screenshot_directory.trim().is_empty() {
        default_screenshot_dir
    } else {
        PathBuf::from(&settings.screenshot_directory)
    };
    std::fs::create_dir_all(&screenshot_dir)?;

    Ok(AppState {
        capture_contexts: Mutex::new(CaptureContexts::default()),
        capture_snapshots: Mutex::new(Vec::new()),
        history: Mutex::new(VecDeque::new()),
        data_dir,
        screenshot_dir: Mutex::new(screenshot_dir),
        settings_store,
        settings: Mutex::new(settings),
        capture_intent: Mutex::new(CaptureIntent::Capture),
        #[cfg(target_os = "macos")]
        capture_previous_frontmost_pid: Mutex::new(None),
        #[cfg(target_os = "macos")]
        capture_main_window_suppressed: Mutex::new(false),
        capture_service: ScreenCaptureService::default(),
        ocr_service: Arc::new(MockOcrService),
        translation_service: Arc::new(MockTranslationService),
    })
}

fn detect_system_language() -> String {
    let locale = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());
    let normalized = locale.split(['-', '_']).next().unwrap_or("en").to_lowercase();
    match normalized.as_str() {
        "zh" => "zh".into(),
        "ja" => "ja".into(),
        _ => "en".into(),
    }
}

fn setup_tray(app: &AppHandle) -> anyhow::Result<()> {
    let show = MenuItemBuilder::with_id(MenuId::new("show"), "显示主界面").build(app)?;
    let capture = MenuItemBuilder::with_id(MenuId::new("capture"), "开始截图").build(app)?;
    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItemBuilder::with_id(MenuId::new("autostart"), "开机启动")
        .checked(autostart_enabled)
        .build(app)?;
    let quit = MenuItemBuilder::with_id(MenuId::new("quit"), "退出 Flick").build(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[&show, &capture, &autostart, &quit])
        .build()?;

    TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| handle_menu_event(app, event))
        .on_tray_icon_event(|tray, event| handle_tray_event(tray.app_handle(), event))
        .build(app)?;

    Ok(())
}

fn register_shortcuts(app: &AppHandle) -> anyhow::Result<()> {
    app.plugin(tauri_plugin_global_shortcut::Builder::new().build())?;

    let settings = {
        let state = app.state::<AppState>();
        let settings = state
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings mutex poisoned"))?;
        settings.clone()
    };
    apply_shortcut_bindings(app, &settings)?;

    Ok(())
}

pub fn apply_shortcut_bindings(app: &AppHandle, settings: &AppSettings) -> anyhow::Result<()> {
    let global_shortcut = app.global_shortcut();

    if settings.capture_shortcut == settings.translate_shortcut {
        anyhow::bail!("截图和截图翻译快捷键不能相同");
    }

    for shortcut in [&settings.capture_shortcut, &settings.translate_shortcut] {
        if global_shortcut.is_registered(shortcut.as_str()) {
            global_shortcut.unregister(shortcut.as_str())?;
        }
    }

    register_shortcut_handler(
        app,
        settings.capture_shortcut.as_str(),
        CaptureIntent::Capture,
    )?;
    register_shortcut_handler(
        app,
        settings.translate_shortcut.as_str(),
        CaptureIntent::Translate,
    )?;

    Ok(())
}

fn register_shortcut_handler(
    app: &AppHandle,
    shortcut: &str,
    intent: CaptureIntent,
) -> anyhow::Result<()> {
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _, event| {
            if event.state == ShortcutState::Pressed {
                let state = app.state::<AppState>();
                let _ = commands::begin_capture_session_with_intent(app, &state, intent);
            }
        })?;

    Ok(())
}

fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();

    match id {
        "show" => {
            let _ = show_main_window(app);
        }
        "capture" => {
            let state = app.state::<AppState>();
            let _ = commands::begin_capture_session(app, &state);
        }
        "autostart" => {
            let enabled = app.autolaunch().is_enabled().unwrap_or(false);
            if enabled {
                let _ = app.autolaunch().disable();
            } else {
                let _ = app.autolaunch().enable();
            }
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

fn handle_tray_event(app: &AppHandle, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        let _ = show_main_window(app);
    }
}

pub fn show_main_window(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(ActivationPolicy::Regular);
        let _ = app.show();
        let _ = app.set_dock_visibility(true);
    }

    let window = ensure_main_window(app)?;
    let _ = window.center();
    let _ = window.set_visible_on_all_workspaces(true);
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    let _ = window.set_visible_on_all_workspaces(false);

    Ok(())
}

pub fn ensure_main_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("index.html".into()))
        .title("Flick")
        .inner_size(1240.0, 800.0)
        .min_inner_size(1040.0, 680.0)
        .resizable(true)
        .visible(true)
        .focused(true)
        .center()
        .hidden_title(true)
        .title_bar_style(TitleBarStyle::Overlay)
        .traffic_light_position(LogicalPosition::new(16.0, 18.0))
        .build()
}

pub fn capture_window_label(index: usize) -> String {
    format!("{CAPTURE_WINDOW_LABEL_PREFIX}-{index}")
}

pub fn is_capture_window_label(label: &str) -> bool {
    label.starts_with(&format!("{CAPTURE_WINDOW_LABEL_PREFIX}-"))
}

pub fn ensure_capture_window(app: &AppHandle, label: &str) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window(label) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App("capture.html".into()),
    )
    .transparent(true)
    .decorations(false)
    .shadow(false)
    .skip_taskbar(true)
    .always_on_top(true)
    .visible(false)
    .resizable(false)
    .build()
}

fn initialize_capture_windows(app: &AppHandle) -> tauri::Result<()> {
    let monitors = app.available_monitors()?;
    for index in 0..monitors.len() {
        let label = capture_window_label(index);
        ensure_capture_window(app, &label)?;
    }
    Ok(())
}

pub fn ensure_widget_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window(WIDGET_WINDOW_LABEL) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(
        app,
        WIDGET_WINDOW_LABEL,
        WebviewUrl::App("translation-window.html".into()),
    )
    .title("Flick Widget")
    .inner_size(480.0, 640.0)
    .min_inner_size(360.0, 480.0)
    .resizable(true)
    .visible(false)
    .focused(false)
    .always_on_top(false)
    .transparent(true)
    .decorations(false)
    .shadow(true)
    .build()
}

pub fn show_widget_window(app: &AppHandle) -> tauri::Result<()> {
    let window = ensure_widget_window(app)?;
    let _ = window.center();
    let _ = window.set_visible_on_all_workspaces(true);
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    let _ = window.set_visible_on_all_workspaces(false);
    Ok(())
}

pub fn emit_capture_status(app: &AppHandle, event: &str, payload: impl serde::Serialize + Clone) {
    let _ = app.emit(event, payload);
}
