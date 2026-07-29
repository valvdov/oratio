use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum InjectError {
    #[error("a secure input field is focused (password?); refusing to paste")]
    SecureInput,
    #[error("clipboard error: {0}")]
    Clipboard(String),
    #[error("failed to synthesize paste keystroke: {0}")]
    Keystroke(String),
}

/// Put text on the clipboard without pasting (fallback when paste is impossible).
pub fn copy_only(text: &str) -> Result<(), InjectError> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| InjectError::Clipboard(e.to_string()))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| InjectError::Clipboard(e.to_string()))
}

/// Insert text into the focused input of the target app:
/// save clipboard -> set text -> synthesize Cmd+V -> restore clipboard.
/// When `target_pid` is known the keystroke is posted directly to that
/// process, which survives any focus theft.
pub fn insert_text(
    text: &str,
    restore_after_ms: u64,
    target_pid: Option<i32>,
) -> Result<(), InjectError> {
    #[cfg(target_os = "macos")]
    {
        if macos::secure_input_active() {
            return Err(InjectError::SecureInput);
        }
    }

    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| InjectError::Clipboard(e.to_string()))?;
    let previous = clipboard.get_text().ok();

    clipboard
        .set_text(text.to_string())
        .map_err(|e| InjectError::Clipboard(e.to_string()))?;

    // Give the pasteboard a moment to settle before the keystroke.
    std::thread::sleep(Duration::from_millis(80));

    #[cfg(target_os = "macos")]
    macos::synthesize_paste(target_pid)?;
    #[cfg(target_os = "windows")]
    {
        let _ = target_pid;
        win::synthesize_paste()?;
    }
    #[cfg(target_os = "linux")]
    {
        let _ = target_pid;
        linux::synthesize_paste()?;
    }

    // Wait long enough for the target app to actually read the pasteboard —
    // restoring too early makes slow apps paste the OLD clipboard contents.
    std::thread::sleep(Duration::from_millis(restore_after_ms));
    if let Some(prev) = previous {
        // Only restore if the clipboard still holds our text; if the user or
        // a clipboard manager changed it meanwhile, leave it alone.
        let still_ours = clipboard
            .get_text()
            .map(|current| current == text)
            .unwrap_or(false);
        if still_ours {
            let _ = clipboard.set_text(prev);
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
mod win {
    use super::InjectError;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_V,
    };

    fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    pub fn synthesize_paste() -> Result<(), InjectError> {
        let inputs = [
            key(VK_CONTROL, false),
            key(VK_V, false),
            key(VK_V, true),
            key(VK_CONTROL, true),
        ];
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent != inputs.len() as u32 {
            return Err(InjectError::Keystroke(format!(
                "SendInput delivered {sent}/{} events",
                inputs.len()
            )));
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::InjectError;

    /// Wayland-friendly paste synthesis. Tries wtype (wlroots/KDE), then
    /// ydotool (needs ydotoold). The XDG RemoteDesktop portal path (works on
    /// GNOME without extra tools) is the planned follow-up.
    pub fn synthesize_paste() -> Result<(), InjectError> {
        let attempts: [(&str, &[&str]); 3] = [
            ("wtype", &["-M", "ctrl", "-k", "v", "-m", "ctrl"]),
            ("ydotool", &["key", "29:1", "47:1", "47:0", "29:0"]),
            ("xdotool", &["key", "--clearmodifiers", "ctrl+v"]),
        ];
        for (bin, args) in attempts {
            match std::process::Command::new(bin).args(args).status() {
                Ok(status) if status.success() => return Ok(()),
                Ok(status) => {
                    tracing::warn!("{bin} exited with {status}");
                }
                Err(_) => continue,
            }
        }
        Err(InjectError::Keystroke(
            "no working paste tool found (tried wtype, ydotool, xdotool); \
             text is on the clipboard — paste manually with Ctrl+V"
                .into(),
        ))
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::InjectError;
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    const KEYCODE_V: u16 = 9;

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn IsSecureEventInputEnabled() -> bool;
    }

    pub fn secure_input_active() -> bool {
        unsafe { IsSecureEventInputEnabled() }
    }

    pub fn synthesize_paste(target_pid: Option<i32>) -> Result<(), InjectError> {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| InjectError::Keystroke("CGEventSource".into()))?;

        let key_down = CGEvent::new_keyboard_event(source.clone(), KEYCODE_V, true)
            .map_err(|_| InjectError::Keystroke("key down".into()))?;
        key_down.set_flags(CGEventFlags::CGEventFlagCommand);

        let key_up = CGEvent::new_keyboard_event(source, KEYCODE_V, false)
            .map_err(|_| InjectError::Keystroke("key up".into()))?;
        key_up.set_flags(CGEventFlags::CGEventFlagCommand);

        match target_pid {
            // Deliver straight to the target process: immune to focus changes.
            Some(pid) => {
                tracing::info!("posting Cmd+V directly to pid {pid}");
                key_down.post_to_pid(pid);
                std::thread::sleep(std::time::Duration::from_millis(10));
                key_up.post_to_pid(pid);
            }
            None => {
                tracing::info!("posting Cmd+V to the HID event stream");
                key_down.post(CGEventTapLocation::HID);
                std::thread::sleep(std::time::Duration::from_millis(10));
                key_up.post(CGEventTapLocation::HID);
            }
        }
        Ok(())
    }
}
