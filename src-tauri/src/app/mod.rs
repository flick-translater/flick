//! Application bootstrap and shared runtime state.

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use tauri::{
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuEvent, MenuId, MenuItemBuilder},
    path::BaseDirectory,
    tray::TrayIconBuilder,
    ActivationPolicy, AppHandle, Manager, RunEvent, WebviewWindow,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
#[cfg(not(target_os = "macos"))]
use tauri_plugin_global_shortcut::GlobalShortcutExt as _;

#[cfg(not(target_os = "macos"))]
use tauri_plugin_global_shortcut::ShortcutState;

use crate::{
    commands,
    models::{AppSettings, CaptureRecord},
    services::{
        CachedScreenCapture, MockOcrService, MockTranslationService, OcrService, SettingsStore,
        VisionOcrService,
    },
};

#[cfg(target_os = "macos")]
pub(crate) mod macos_hotkeys;
#[cfg(target_os = "macos")]
mod macos_permissions;
pub mod windows;

/// Shared application state injected into Tauri commands and feature modules.
pub struct AppState {
    pub capture_snapshots: Mutex<Vec<CachedScreenCapture>>,
    pub history: Mutex<VecDeque<CaptureRecord>>,
    pub data_dir: PathBuf,
    pub screenshot_dir: Mutex<PathBuf>,
    pub settings_store: SettingsStore,
    pub settings: Mutex<AppSettings>,
    pub capture_intent: Mutex<CaptureIntent>,
    pub ocr_service: Mutex<Arc<dyn OcrService>>,
    pub translation_service: Arc<MockTranslationService>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureIntent {
    /// Plain screenshot flow.
    Capture,
    /// Screenshot followed by OCR + translation flow.
    Translate,
}

/// Build and run the Tauri desktop application.
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            app.set_activation_policy(ActivationPolicy::Accessory);
            windows::ensure_main_window(app.handle())?;
            windows::ensure_widget_window(app.handle())?;
            let state = build_state(app.handle())?;
            app.manage(state);

            setup_tray(app.handle())?;
            register_shortcuts(app.handle())?;

            if let Some(main_window) = app.get_webview_window("main") {
                setup_window_close_handler(&main_window);
            }

            #[cfg(target_os = "macos")]
            macos_permissions::launch_startup_permission_check(app.handle());

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
            commands::settings::get_app_settings,
            commands::settings::get_autostart_status,
            commands::settings::set_autostart_enabled,
            commands::settings::set_shortcut_recording,
            commands::settings::update_capture_shortcut,
            commands::settings::update_interface_language,
            commands::settings::update_max_screenshots,
            commands::settings::update_screenshot_directory,
            commands::settings::update_translate_shortcut,
            commands::settings::update_ocr_provider,
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
        .build(tauri::generate_context!())
        .expect("failed to build Flick application");

    app.run(|app, event| match event {
        RunEvent::Reopen { .. } => {
            let _ = windows::show_main_window(app);
        }
        _ => {}
    });
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
    let mut settings = settings_store.load_settings()?;
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
        data_dir,
        screenshot_dir: Mutex::new(screenshot_dir),
        settings_store,
        settings: Mutex::new(settings.clone()),
        capture_intent: Mutex::new(CaptureIntent::Capture),
        ocr_service: Mutex::new(create_ocr_service(&settings.ocr_provider)),
        translation_service: Arc::new(MockTranslationService),
    })
}

pub fn create_ocr_service(provider: &str) -> Arc<dyn OcrService> {
    match provider {
        "mock" => Arc::new(MockOcrService),
        _ => Arc::new(VisionOcrService),
    }
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

    tray.menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| handle_menu_event(app, event))
        .build(app)?;

    Ok(())
}

fn load_tray_icon(app: &AppHandle) -> Option<tauri::image::Image<'static>> {
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

    resource_icon.or_else(|| {
        app.default_window_icon()
            .cloned()
            .map(|icon| icon.to_owned())
    })
}

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
    #[cfg(target_os = "macos")]
    macos_hotkeys::install_hotkey_tap(app)?;

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
    // Two global actions are exposed today, so we fail fast on conflicting bindings.
    if settings.capture_shortcut == settings.translate_shortcut {
        anyhow::bail!("截图和截图翻译快捷键不能相同");
    }

    #[cfg(target_os = "macos")]
    {
        let _ = app;
        macos_hotkeys::apply_shortcuts(
            settings.capture_shortcut.as_str(),
            settings.translate_shortcut.as_str(),
        )?;
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let global_shortcut = app.global_shortcut();

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
}

#[cfg(not(target_os = "macos"))]
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
                let _ = app_handle.hide();

                #[cfg(target_os = "macos")]
                {
                    let _ = app_handle.set_activation_policy(ActivationPolicy::Accessory);
                }
            }
        }
    });
}
