//! Application bootstrap and shared runtime state.

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use tauri::{
    AppHandle, Manager, WebviewWindow,
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuEvent, MenuId, MenuItemBuilder},
    path::BaseDirectory,
    tray::TrayIconBuilder,
};
#[cfg(target_os = "windows")]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri_plugin_autostart::{Builder as AutostartBuilder, ManagerExt as _};
#[cfg(target_os = "macos")]
use tauri_plugin_autostart::MacosLauncher;

use crate::{
    commands,
    models::{AppSettings, CaptureRecord, TranslateWindowState},
    services::{
        CachedScreenCapture, OcrService, SettingsStore, TranslationHistoryStore, TtsService,
        available_ocr_engines, create_ocr_service, default_ocr_provider,
    },
};

#[cfg(target_os = "macos")]
pub(crate) mod macos_hotkeys;
#[cfg(target_os = "macos")]
mod macos_permissions;
pub(crate) mod platform;
pub mod windows;

/// Shared application state injected into Tauri commands and feature modules.
pub struct AppState {
    pub capture_snapshots: Mutex<Vec<CachedScreenCapture>>,
    pub history: Mutex<VecDeque<CaptureRecord>>,
    pub data_dir: PathBuf,
    pub ocr_models_dir: PathBuf,
    pub screenshot_dir: Mutex<PathBuf>,
    pub translation_history_store: TranslationHistoryStore,
    pub settings_store: SettingsStore,
    pub settings: Mutex<AppSettings>,
    pub capture_intent: Mutex<CaptureIntent>,
    pub ocr_service: Mutex<Arc<dyn OcrService>>,
    pub tts_service: TtsService,
    pub translate_window_state: Mutex<TranslateWindowState>,
    pub translate_window_pinned: Mutex<bool>,
    #[cfg(target_os = "macos")]
    pub suppress_next_reopen: Mutex<bool>,
    #[cfg(target_os = "macos")]
    pub previous_frontmost_app_pid: Mutex<Option<i32>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureIntent {
    /// Plain screenshot flow.
    Capture,
    /// Screenshot followed by OCR + translation flow.
    Translate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutAction {
    Capture,
    TranslateCapture,
    TranslateSelectedText,
}

const AUTOSTART_ARG: &str = "--autostart";

/// Build and run the Tauri desktop application.
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(build_autostart_plugin())
        .setup(|app| {
            platform::configure_app_setup(app);
            windows::ensure_main_window(app.handle())?;
            windows::ensure_translate_window(app.handle())?;
            hide_windows_for_autostart_launch(app.handle());
            let state = build_state(app.handle())?;
            app.manage(state);

            if let Err(error) = initialize_autostart(app.handle()) {
                eprintln!("failed to initialize autostart: {error}");
            }

            setup_tray(app.handle())?;
            register_shortcuts(app.handle())?;

            if let Some(main_window) = app.get_webview_window("main") {
                setup_window_close_handler(&main_window);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::capture::list_capture_history,
            commands::capture::get_storage_info,
            commands::capture::pick_screenshot_directory,
            commands::capture::open_file_in_default_app,
            commands::capture::read_image_as_data_url,
            commands::capture::delete_capture,
            commands::capture::clear_all_captures,
            commands::capture::copy_capture_image,
            commands::capture::start_capture_session,
            commands::capture::start_translate_capture_session,
            commands::settings::get_app_settings,
            commands::settings::get_autostart_status,
            commands::settings::set_autostart_enabled,
            commands::settings::set_shortcut_recording,
            commands::settings::update_capture_shortcut,
            commands::settings::update_interface_language,
            commands::settings::update_max_screenshots,
            commands::settings::update_screenshot_directory,
            commands::settings::update_translate_shortcut,
            commands::settings::update_selected_translate_shortcut,
            commands::settings::update_ocr_shortcut_enabled,
            commands::settings::update_ocr_auto_translate,
            commands::settings::update_ocr_target_language,
            commands::settings::get_available_ocr_engines,
            commands::settings::update_ocr_provider,
            commands::settings::update_ai_settings,
            commands::translate_window::show_translate_window,
            commands::translate_window::get_translate_window_pinned,
            commands::translate_window::is_translate_window_pinning_supported,
            commands::translate_window::get_translate_window_state,
            commands::translate_window::swap_translate_window_content,
            commands::translate_window::set_translate_window_pinned,
            commands::translate_window::minimize_translate_window,
            commands::translate_window::close_translate_window,
            commands::translate_window::translate_selected_text,
            commands::translate_window::begin_translate_window_drag,
            commands::translate_window::speak_window_text,
            commands::translate_window::stop_window_tts,
            commands::translate_window::get_window_tts_snapshot,
            commands::translation::translate,
            commands::translation::list_translation_history,
            commands::translation::clear_translation_history,
            commands::translation::delete_translation_record,
            commands::translation::test_ai_connection,
        ])
        .on_menu_event(handle_menu_event)
        .build(tauri::generate_context!())
        .expect("failed to build Flick application");

    app.run(|app, event| platform::handle_run_event(app, &event));
}

fn build_state(app: &AppHandle) -> anyhow::Result<AppState> {
    // All runtime data lives under the app cache directory so the app bundle stays clean.
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
    let translation_history_store =
        TranslationHistoryStore::new(data_dir.join("translations.sqlite3"))?;
    let bundled_ocr_models_dir = app.path().resolve("ocr/onnx", BaseDirectory::Resource).ok();
    let dev_ocr_models_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/ocr/onnx");
    let ocr_models_dir = bundled_ocr_models_dir
        .filter(|path| path.join("text_detection.onnx").is_file())
        .or_else(|| {
            dev_ocr_models_dir
                .join("text_detection.onnx")
                .is_file()
                .then_some(dev_ocr_models_dir)
        })
        .unwrap_or_else(|| data_dir.join("ocr/onnx"));
    let mut settings = settings_store.load_settings()?;
    settings.ai.normalize();
    let available_engines = available_ocr_engines();
    if !available_engines
        .iter()
        .any(|engine| engine.id == settings.ocr_provider)
    {
        settings.ocr_provider = default_ocr_provider();
    }
    settings_store.save_settings(&settings)?;
    // When the user has not picked a UI language yet, initialize it from the system locale.
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
        capture_snapshots: Mutex::new(Vec::new()),
        history: Mutex::new(VecDeque::new()),
        data_dir: data_dir.clone(),
        ocr_models_dir: ocr_models_dir.clone(),
        screenshot_dir: Mutex::new(screenshot_dir),
        translation_history_store,
        settings_store,
        settings: Mutex::new(settings.clone()),
        capture_intent: Mutex::new(CaptureIntent::Capture),
        ocr_service: Mutex::new(create_ocr_service(&settings.ocr_provider, &ocr_models_dir)),
        tts_service: TtsService::new(data_dir.clone()),
        translate_window_state: Mutex::new(TranslateWindowState::default()),
        translate_window_pinned: Mutex::new(false),
        #[cfg(target_os = "macos")]
        suppress_next_reopen: Mutex::new(false),
        #[cfg(target_os = "macos")]
        previous_frontmost_app_pid: Mutex::new(None),
    })
}

fn detect_system_language() -> String {
    let locale = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());
    let normalized = locale
        .split(['-', '_'])
        .next()
        .unwrap_or("en")
        .to_lowercase();
    match normalized.as_str() {
        "zh" => "zh".into(),
        "ja" => "ja".into(),
        _ => "en".into(),
    }
}

pub fn initialize_autostart(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let (configured, desired_enabled) = {
        let settings = state
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings mutex poisoned"))?;
        (settings.autostart_configured, settings.autostart_enabled)
    };

    if !configured {
        // Honor explicit config on first sync so a persisted `autostart_enabled = true`
        // immediately provisions the OS startup entry on Windows/Linux/macOS.
        let actual_enabled = sync_autostart_state(app, desired_enabled)?;
        persist_autostart_preference(app, actual_enabled, true)?;
        return Ok(());
    }

    let actual_enabled = sync_autostart_state(app, desired_enabled)?;
    persist_autostart_preference(app, actual_enabled, true)?;

    Ok(())
}

pub fn set_autostart_enabled(app: &AppHandle, enabled: bool) -> anyhow::Result<bool> {
    let actual_enabled = sync_autostart_state(app, enabled)?;
    persist_autostart_preference(app, actual_enabled, true)?;
    Ok(actual_enabled)
}

fn sync_autostart_state(app: &AppHandle, enabled: bool) -> anyhow::Result<bool> {
    let current_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    if enabled != current_enabled {
        if enabled {
            app.autolaunch().enable()?;
        } else {
            app.autolaunch().disable()?;
        }
    }

    Ok(app.autolaunch().is_enabled().unwrap_or(enabled))
}

fn persist_autostart_preference(
    app: &AppHandle,
    enabled: bool,
    configured: bool,
) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let updated = {
        let mut settings = state
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings mutex poisoned"))?;
        settings.autostart_enabled = enabled;
        settings.autostart_configured = configured;
        settings.clone()
    };

    state.settings_store.save_settings(&updated)?;
    Ok(())
}

fn build_autostart_plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    #[cfg(target_os = "macos")]
    {
        return AutostartBuilder::new()
            .app_name("Flick")
            .arg(AUTOSTART_ARG)
            .macos_launcher(MacosLauncher::LaunchAgent)
            .build();
    }

    #[cfg(not(target_os = "macos"))]
    {
        AutostartBuilder::new()
            .app_name("Flick")
            .arg(AUTOSTART_ARG)
            .build()
    }
}

fn hide_windows_for_autostart_launch(app: &AppHandle) {
    if !is_autostart_launch() {
        return;
    }

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }

    if let Some(window) = app.get_webview_window("translate") {
        let _ = window.hide();
    }
}

fn is_autostart_launch() -> bool {
    std::env::args_os().any(|arg| arg == AUTOSTART_ARG)
}

fn setup_tray(app: &AppHandle) -> anyhow::Result<()> {
    let show = MenuItemBuilder::with_id(MenuId::new("show"), "显示主界面").build(app)?;
    let capture = MenuItemBuilder::with_id(MenuId::new("capture"), "开始截图").build(app)?;
    let translate_capture =
        MenuItemBuilder::with_id(MenuId::new("translate_capture"), "截图翻译").build(app)?;
    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItemBuilder::with_id(MenuId::new("autostart"), "开机启动")
        .checked(autostart_enabled)
        .build(app)?;
    let quit = MenuItemBuilder::with_id(MenuId::new("quit"), "退出 Flick").build(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[&show, &capture, &translate_capture, &autostart, &quit])
        .build()?;

    let mut tray = TrayIconBuilder::with_id("main-tray");

    if let Some(icon) = load_tray_icon(app) {
        tray = tray.icon(icon);
        #[cfg(target_os = "macos")]
        {
            tray = tray.icon_as_template(true);
        }
    }

    #[cfg(target_os = "windows")]
    {
        tray = tray
            .show_menu_on_left_click(false)
            .on_tray_icon_event(|tray, event| handle_windows_tray_event(&tray.app_handle(), event));
    }

    #[cfg(not(target_os = "windows"))]
    {
        tray = tray.show_menu_on_left_click(true);
    }

    tray.menu(&menu)
        .on_menu_event(|app, event| handle_menu_event(app, event))
        .build(app)?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn handle_windows_tray_event(app: &AppHandle, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        let _ = windows::show_main_window(app);
    }
}

fn load_tray_icon(app: &AppHandle) -> Option<tauri::image::Image<'static>> {
    #[cfg(target_os = "macos")]
    let resource_icon = ["icons/trayTemplate@2x.png", "icons/trayTemplate.png"]
        .into_iter()
        .find_map(|relative_path| {
            app.path()
                .resolve(relative_path, BaseDirectory::Resource)
                .ok()
                .filter(|path| path.exists())
                .and_then(|path| std::fs::read(path).ok())
                .and_then(|bytes| decode_tray_icon(&bytes))
        });

    #[cfg(not(target_os = "macos"))]
    let resource_icon: Option<tauri::image::Image<'static>> = None;

    #[cfg(target_os = "linux")]
    let resource_icon = ["icons/icon_256x256.png", "icons/icon_128x128.png", "icons/icon_32x32.png"]
        .into_iter()
        .find_map(|relative_path| {
            app.path()
                .resolve(relative_path, BaseDirectory::Resource)
                .ok()
                .filter(|path| path.exists())
                .and_then(|path| std::fs::read(path).ok())
                .and_then(|bytes| decode_tray_icon(&bytes))
        });

    resource_icon.or_else(|| {
        app.default_window_icon()
            .cloned()
            .map(|icon| icon.to_owned())
    })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn decode_tray_icon(bytes: &[u8]) -> Option<tauri::image::Image<'static>> {
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(tauri::image::Image::new_owned(
        image.into_raw(),
        width,
        height,
    ))
}

fn register_shortcuts(app: &AppHandle) -> anyhow::Result<()> {
    app.plugin(tauri_plugin_global_shortcut::Builder::new().build())?;
    platform::register_platform_shortcuts(app)?;

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
    validate_shortcut_conflicts(settings)?;
    platform::apply_shortcut_bindings(app, settings)
}

fn validate_shortcut_conflicts(settings: &AppSettings) -> anyhow::Result<()> {
    let shortcuts = [
        ("截图", settings.capture_shortcut.as_str()),
        ("截图翻译", settings.translate_shortcut.as_str()),
        ("选中翻译", settings.selected_translate_shortcut.as_str()),
    ];

    for (index, (left_label, left_shortcut)) in shortcuts.iter().enumerate() {
        for (right_label, right_shortcut) in shortcuts.iter().skip(index + 1) {
            if left_shortcut == right_shortcut {
                anyhow::bail!("{left_label}和{right_label}快捷键不能相同");
            }
        }
    }

    Ok(())
}

pub fn trigger_shortcut_action(app: &AppHandle, action: ShortcutAction) {
    platform::trigger_shortcut_action(app, action);
}

fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "show" => {
            let _ = windows::show_main_window(app);
        }
        "capture" => {
            let state = app.state::<AppState>();
            let _ = commands::capture::begin_capture_session(app, &state);
        }
        "translate_capture" => {
            let state = app.state::<AppState>();
            let _ = commands::capture::begin_capture_session_with_intent(
                app,
                &state,
                CaptureIntent::Translate,
            );
        }
        "autostart" => {
            let enabled = app.autolaunch().is_enabled().unwrap_or(false);
            let _ = set_autostart_enabled(app, !enabled);
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

fn setup_window_close_handler(window: &WebviewWindow) {
    let app_handle = window.app_handle().clone();
    let window_label = window.label().to_string();

    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();

            if window_label == "main" {
                if let Some(win) = app_handle.get_webview_window(&window_label) {
                    let _ = win.hide();
                }
                platform::on_main_window_close(&app_handle);
            }
        }
    });
}
