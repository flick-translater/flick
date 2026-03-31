//! Thin Tauri command adapters for the floating translation widget.

use std::sync::mpsc;

use tauri::AppHandle;

use crate::{app::windows, error::FlickError};

#[tauri::command]
pub fn show_translation_widget(app: AppHandle) -> Result<(), FlickError> {
    windows::show_widget_window(&app)?;
    Ok(())
}

#[tauri::command]
pub fn get_translation_widget_pinned(app: AppHandle) -> Result<bool, FlickError> {
    Ok(windows::ensure_widget_window(&app)?.is_always_on_top()?)
}

#[tauri::command]
pub fn set_translation_widget_pinned(app: AppHandle, pinned: bool) -> Result<(), FlickError> {
    windows::ensure_widget_window(&app)?.set_always_on_top(pinned)?;
    Ok(())
}

#[tauri::command]
pub fn minimize_translation_widget(app: AppHandle) -> Result<(), FlickError> {
    windows::ensure_widget_window(&app)?.minimize()?;
    Ok(())
}

#[tauri::command]
pub fn close_translation_widget(app: AppHandle) -> Result<(), FlickError> {
    windows::hide_widget_window(&app)?;
    Ok(())
}

#[tauri::command]
pub fn begin_translation_widget_drag(app: AppHandle) -> Result<(), FlickError> {
    let window = windows::ensure_widget_window(&app)?;

    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSApplication, NSWindow};

        let ns_window = window.ns_window()? as usize;
        let (sender, receiver) = mpsc::channel();

        app.run_on_main_thread(move || {
            let result = (|| -> Result<(), FlickError> {
                let mtm = MainThreadMarker::new()
                    .ok_or_else(|| FlickError::Message("main thread marker unavailable".into()))?;
                let app = NSApplication::sharedApplication(mtm);
                let window: &NSWindow =
                    unsafe { &*(ns_window as *mut std::ffi::c_void).cast::<NSWindow>() };

                app.activate();
                window.makeKeyAndOrderFront(None);

                let event = app
                    .currentEvent()
                    .ok_or_else(|| FlickError::Message("no current mouse event for drag".into()))?;
                window.performWindowDragWithEvent(&event);
                Ok(())
            })();

            let _ = sender.send(result);
        })?;

        return receiver
            .recv()
            .map_err(|error| FlickError::Message(error.to_string()))?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        let _ = window;
        Err(FlickError::Message(
            "translation widget native drag is only implemented on macOS".into(),
        ))
    }
}
