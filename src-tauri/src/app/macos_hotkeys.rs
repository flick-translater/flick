use std::{
    sync::{Mutex, OnceLock, mpsc},
    thread,
    time::Duration,
};

use anyhow::{Context, anyhow};
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes, kCFRunLoopDefaultMode};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CallbackResult, EventField,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

use crate::{
    app::{AppState, CaptureIntent},
    commands,
};

#[derive(Clone, Copy)]
struct RegisteredHotkey {
    shortcut: Shortcut,
    intent: CaptureIntent,
}

#[derive(Default)]
struct MacosHotkeyRuntime {
    tap_installed: bool,
    paused: bool,
    shortcuts: Vec<RegisteredHotkey>,
}

fn hotkey_runtime() -> &'static Mutex<MacosHotkeyRuntime> {
    static RUNTIME: OnceLock<Mutex<MacosHotkeyRuntime>> = OnceLock::new();
    RUNTIME.get_or_init(|| Mutex::new(MacosHotkeyRuntime::default()))
}

pub fn install_hotkey_tap(app: &AppHandle) -> anyhow::Result<()> {
    {
        let runtime = hotkey_runtime()
            .lock()
            .map_err(|_| anyhow!("macOS hotkey runtime mutex poisoned"))?;
        if runtime.tap_installed {
            return Ok(());
        }
    }

    let (tx, rx) = mpsc::channel();
    let app_handle = app.clone();
    thread::spawn(move || run_hotkey_event_tap(app_handle, tx));

    rx.recv()
        .context("failed waiting for macOS hotkey event tap initialization")??;

    let mut runtime = hotkey_runtime()
        .lock()
        .map_err(|_| anyhow!("macOS hotkey runtime mutex poisoned"))?;
    runtime.tap_installed = true;
    Ok(())
}

pub fn apply_shortcuts(
    capture_shortcut: &str,
    translate_shortcut: Option<&str>,
) -> anyhow::Result<()> {
    let capture = capture_shortcut
        .parse::<Shortcut>()
        .map_err(|error| anyhow!("截图快捷键无效: {error}"))?;
    let translate = translate_shortcut
        .map(|shortcut| {
            shortcut
                .parse::<Shortcut>()
                .map_err(|error| anyhow!("截图翻译快捷键无效: {error}"))
        })
        .transpose()?;

    let mut runtime = hotkey_runtime()
        .lock()
        .map_err(|_| anyhow!("macOS hotkey runtime mutex poisoned"))?;
    runtime.shortcuts = vec![RegisteredHotkey {
        shortcut: capture,
        intent: CaptureIntent::Capture,
    }];
    if let Some(translate) = translate {
        runtime.shortcuts.push(RegisteredHotkey {
            shortcut: translate,
            intent: CaptureIntent::Translate,
        });
    }
    Ok(())
}

pub fn set_recording_paused(paused: bool) -> anyhow::Result<()> {
    let mut runtime = hotkey_runtime()
        .lock()
        .map_err(|_| anyhow!("macOS hotkey runtime mutex poisoned"))?;
    runtime.paused = paused;
    Ok(())
}

fn run_hotkey_event_tap(app: AppHandle, initialized: mpsc::Sender<anyhow::Result<()>>) {
    let tap = match CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![CGEventType::KeyDown],
        move |_proxy, event_type, event| handle_hotkey_event(&app, event_type, event),
    ) {
        Ok(tap) => tap,
        Err(()) => {
            let _ = initialized.send(Err(anyhow!("failed to create macOS hotkey event tap")));
            return;
        }
    };

    let source = match tap.mach_port().create_runloop_source(0) {
        Ok(source) => source,
        Err(()) => {
            let _ = initialized.send(Err(anyhow!(
                "failed to create macOS hotkey event runloop source"
            )));
            return;
        }
    };

    let run_loop = CFRunLoop::get_current();
    run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
    tap.enable();
    let _ = initialized.send(Ok(()));

    loop {
        let _ = CFRunLoop::run_in_mode(
            unsafe { kCFRunLoopDefaultMode },
            Duration::from_millis(50),
            true,
        );
    }
}

fn handle_hotkey_event(
    app: &AppHandle,
    event_type: CGEventType,
    event: &CGEvent,
) -> CallbackResult {
    if !matches!(event_type, CGEventType::KeyDown) {
        return CallbackResult::Keep;
    }

    let runtime = match hotkey_runtime().lock() {
        Ok(runtime) => runtime,
        Err(_) => return CallbackResult::Keep,
    };
    if runtime.paused {
        return CallbackResult::Keep;
    }

    let shortcuts = runtime.shortcuts.clone();
    drop(runtime);

    if shortcuts.is_empty() {
        return CallbackResult::Keep;
    }

    if event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0 {
        return CallbackResult::Keep;
    }

    let modifiers = modifiers_from_event_flags(event.get_flags());
    let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u32;

    let Some(intent) = shortcuts
        .into_iter()
        .find(|registered| shortcut_matches_event(&registered.shortcut, modifiers, keycode))
        .map(|registered| registered.intent)
    else {
        return CallbackResult::Keep;
    };

    let app_handle = app.clone();
    thread::spawn(move || {
        let state = app_handle.state::<AppState>();
        let _ = commands::capture::begin_capture_session_with_intent(&app_handle, &state, intent);
    });

    CallbackResult::Drop
}

fn modifiers_from_event_flags(flags: CGEventFlags) -> Modifiers {
    let mut modifiers = Modifiers::empty();
    if flags.contains(CGEventFlags::CGEventFlagShift) {
        modifiers |= Modifiers::SHIFT;
    }
    if flags.contains(CGEventFlags::CGEventFlagControl) {
        modifiers |= Modifiers::CONTROL;
    }
    if flags.contains(CGEventFlags::CGEventFlagAlternate) {
        modifiers |= Modifiers::ALT;
    }
    if flags.contains(CGEventFlags::CGEventFlagCommand) {
        modifiers |= Modifiers::SUPER;
    }
    modifiers
}

fn shortcut_matches_event(shortcut: &Shortcut, modifiers: Modifiers, keycode: u32) -> bool {
    shortcut.mods == modifiers && shortcut_keycode(shortcut.key) == Some(keycode)
}

fn shortcut_keycode(code: Code) -> Option<u32> {
    match code {
        Code::KeyA => Some(0x00),
        Code::KeyS => Some(0x01),
        Code::KeyD => Some(0x02),
        Code::KeyF => Some(0x03),
        Code::KeyH => Some(0x04),
        Code::KeyG => Some(0x05),
        Code::KeyZ => Some(0x06),
        Code::KeyX => Some(0x07),
        Code::KeyC => Some(0x08),
        Code::KeyV => Some(0x09),
        Code::KeyB => Some(0x0b),
        Code::KeyQ => Some(0x0c),
        Code::KeyW => Some(0x0d),
        Code::KeyE => Some(0x0e),
        Code::KeyR => Some(0x0f),
        Code::KeyY => Some(0x10),
        Code::KeyT => Some(0x11),
        Code::Digit1 => Some(0x12),
        Code::Digit2 => Some(0x13),
        Code::Digit3 => Some(0x14),
        Code::Digit4 => Some(0x15),
        Code::Digit6 => Some(0x16),
        Code::Digit5 => Some(0x17),
        Code::Equal => Some(0x18),
        Code::Digit9 => Some(0x19),
        Code::Digit7 => Some(0x1a),
        Code::Minus => Some(0x1b),
        Code::Digit8 => Some(0x1c),
        Code::Digit0 => Some(0x1d),
        Code::BracketRight => Some(0x1e),
        Code::KeyO => Some(0x1f),
        Code::KeyU => Some(0x20),
        Code::BracketLeft => Some(0x21),
        Code::KeyI => Some(0x22),
        Code::KeyP => Some(0x23),
        Code::Enter => Some(0x24),
        Code::KeyL => Some(0x25),
        Code::KeyJ => Some(0x26),
        Code::Quote => Some(0x27),
        Code::KeyK => Some(0x28),
        Code::Semicolon => Some(0x29),
        Code::Backslash => Some(0x2a),
        Code::Comma => Some(0x2b),
        Code::Slash => Some(0x2c),
        Code::KeyN => Some(0x2d),
        Code::KeyM => Some(0x2e),
        Code::Period => Some(0x2f),
        Code::Tab => Some(0x30),
        Code::Space => Some(0x31),
        Code::Backquote => Some(0x32),
        Code::Backspace => Some(0x33),
        Code::Escape => Some(0x35),
        Code::F17 => Some(0x40),
        Code::NumpadDecimal => Some(0x41),
        Code::NumpadMultiply => Some(0x43),
        Code::NumpadAdd => Some(0x45),
        Code::NumLock => Some(0x47),
        Code::AudioVolumeUp => Some(0x48),
        Code::AudioVolumeDown => Some(0x49),
        Code::AudioVolumeMute => Some(0x4a),
        Code::NumpadDivide => Some(0x4b),
        Code::NumpadEnter => Some(0x4c),
        Code::NumpadSubtract => Some(0x4e),
        Code::F18 => Some(0x4f),
        Code::F19 => Some(0x50),
        Code::NumpadEqual => Some(0x51),
        Code::Numpad0 => Some(0x52),
        Code::Numpad1 => Some(0x53),
        Code::Numpad2 => Some(0x54),
        Code::Numpad3 => Some(0x55),
        Code::Numpad4 => Some(0x56),
        Code::Numpad5 => Some(0x57),
        Code::Numpad6 => Some(0x58),
        Code::Numpad7 => Some(0x59),
        Code::F20 => Some(0x5a),
        Code::Numpad8 => Some(0x5b),
        Code::Numpad9 => Some(0x5c),
        Code::F5 => Some(0x60),
        Code::F6 => Some(0x61),
        Code::F7 => Some(0x62),
        Code::F3 => Some(0x63),
        Code::F8 => Some(0x64),
        Code::F9 => Some(0x65),
        Code::F11 => Some(0x67),
        Code::F13 => Some(0x69),
        Code::F16 => Some(0x6a),
        Code::F14 => Some(0x6b),
        Code::F10 => Some(0x6d),
        Code::F12 => Some(0x6f),
        Code::F15 => Some(0x71),
        Code::Insert => Some(0x72),
        Code::Home => Some(0x73),
        Code::PageUp => Some(0x74),
        Code::Delete => Some(0x75),
        Code::F4 => Some(0x76),
        Code::End => Some(0x77),
        Code::F2 => Some(0x78),
        Code::PageDown => Some(0x79),
        Code::F1 => Some(0x7a),
        Code::ArrowLeft => Some(0x7b),
        Code::ArrowRight => Some(0x7c),
        Code::ArrowDown => Some(0x7d),
        Code::ArrowUp => Some(0x7e),
        Code::CapsLock => Some(0x39),
        Code::PrintScreen => Some(0x46),
        _ => None,
    }
}
