use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use tauri::{
    ActivationPolicy, AppHandle, Manager, RunEvent,
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuEvent, MenuId, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_global_shortcut::{GlobalShortcutExt as _, ShortcutState};

#[cfg(not(target_os = "macos"))]
use crate::services::ScreenCaptureService;
use crate::{
    commands,
    models::{AppSettings, CaptureContexts, CaptureRecord},
    services::{CachedScreenCapture, MockOcrService, MockTranslationService, SettingsStore},
};

pub mod windows;

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
    #[cfg(not(target_os = "macos"))]
    pub capture_service: ScreenCaptureService,
    pub ocr_service: Arc<MockOcrService>,
    pub translation_service: Arc<MockTranslationService>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureIntent {
    Capture,
    Translate,
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            app.set_activation_policy(ActivationPolicy::Regular);
            windows::ensure_main_window(app.handle())?;
            windows::initialize_capture_windows(app.handle())?;
            windows::ensure_widget_window(app.handle())?;
            let state = build_state(app.handle())?;
            app.manage(state);

            setup_tray(app.handle())?;
            register_shortcuts(app.handle())?;
            windows::show_main_window(app.handle())?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::capture::start_capture,
            commands::capture::focus_capture_window,
            commands::capture::cancel_capture,
            commands::capture::complete_capture,
            commands::capture::get_global_cursor_position,
            commands::capture::refresh_capture_context,
            commands::capture::get_capture_context,
            commands::capture::list_capture_history,
            commands::capture::get_storage_info,
            commands::capture::pick_screenshot_directory,
            commands::capture::open_file_in_default_app,
            commands::capture::read_image_as_data_url,
            commands::capture::delete_capture,
            commands::capture::clear_all_captures,
            commands::settings::get_app_settings,
            commands::settings::get_autostart_status,
            commands::settings::set_autostart_enabled,
            commands::settings::set_shortcut_recording,
            commands::settings::update_capture_shortcut,
            commands::settings::update_interface_language,
            commands::settings::update_max_screenshots,
            commands::settings::update_screenshot_directory,
            commands::settings::update_translate_shortcut,
            commands::widget::show_translation_widget,
            commands::widget::get_translation_widget_pinned,
            commands::widget::set_translation_widget_pinned,
            commands::widget::minimize_translation_widget,
            commands::widget::close_translation_widget,
            commands::widget::begin_translation_widget_drag,
            commands::ocr::mock_ocr,
            commands::translation::mock_translate,
        ])
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(handle_tray_event)
        .build(tauri::generate_context!())
        .expect("failed to build Flick application");

    app.run(|app, event| match event {
        RunEvent::Ready | RunEvent::Reopen { .. } => {
            let _ = windows::show_main_window(app);
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
        #[cfg(not(target_os = "macos"))]
        capture_service: ScreenCaptureService::default(),
        ocr_service: Arc::new(MockOcrService),
        translation_service: Arc::new(MockTranslationService),
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
                let _ = commands::capture::begin_capture_session_with_intent(app, &state, intent);
            }
        })?;

    Ok(())
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
        let _ = windows::show_main_window(app);
    }
}
