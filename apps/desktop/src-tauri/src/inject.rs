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
    use std::sync::OnceLock;

    use ashpd::desktop::remote_desktop::{DeviceType, KeyState, RemoteDesktop};
    use ashpd::desktop::{PersistMode, Session};

    // Linux evdev keycodes.
    const KEY_LEFTCTRL: i32 = 29;
    const KEY_V: i32 = 47;

    /// A persistent XDG RemoteDesktop portal session. This is the only paste
    /// path that works on stock KDE and GNOME Wayland (KWin/Mutter do not
    /// expose the virtual-keyboard protocol to regular clients). The user
    /// approves it once; the restore token skips the dialog afterwards.
    struct Portal {
        proxy: RemoteDesktop<'static>,
        session: Session<'static, RemoteDesktop<'static>>,
    }

    static PORTAL: OnceLock<tauri::async_runtime::Mutex<Option<Portal>>> = OnceLock::new();

    fn portal_cell() -> &'static tauri::async_runtime::Mutex<Option<Portal>> {
        PORTAL.get_or_init(|| tauri::async_runtime::Mutex::new(None))
    }

    fn token_path() -> std::path::PathBuf {
        oratio_core::paths::data_dir().join("portal-restore-token")
    }

    async fn connect() -> ashpd::Result<Portal> {
        let proxy = RemoteDesktop::new().await?;
        let session = proxy.create_session().await?;
        let token = std::fs::read_to_string(token_path()).ok();
        proxy
            .select_devices(
                &session,
                DeviceType::Keyboard.into(),
                token.as_deref().map(str::trim),
                PersistMode::ExplicitlyRevoked,
            )
            .await?
            .response()?;
        let devices = proxy.start(&session, None).await?.response()?;
        if let Some(t) = devices.restore_token() {
            let _ = std::fs::write(token_path(), t);
        }
        tracing::info!("RemoteDesktop portal session established");
        Ok(Portal { proxy, session })
    }

    async fn send_ctrl_v(portal: &Portal) -> ashpd::Result<()> {
        let keys = [
            (KEY_LEFTCTRL, KeyState::Pressed),
            (KEY_V, KeyState::Pressed),
            (KEY_V, KeyState::Released),
            (KEY_LEFTCTRL, KeyState::Released),
        ];
        for (code, state) in keys {
            portal
                .proxy
                .notify_keyboard_keycode(&portal.session, code, state)
                .await?;
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        Ok(())
    }

    async fn portal_paste() -> ashpd::Result<()> {
        let cell = portal_cell();
        let mut guard = cell.lock().await;
        if guard.is_none() {
            *guard = Some(connect().await?);
        }
        if let Err(e) = send_ctrl_v(guard.as_ref().unwrap()).await {
            tracing::warn!("portal session stale ({e}), reconnecting");
            *guard = Some(connect().await?);
            send_ctrl_v(guard.as_ref().unwrap()).await?;
        }
        Ok(())
    }

    pub fn synthesize_paste() -> Result<(), InjectError> {
        // Portal first: works on KDE and GNOME Wayland with one-time approval.
        match tauri::async_runtime::block_on(portal_paste()) {
            Ok(()) => return Ok(()),
            Err(e) => tracing::warn!("portal paste failed ({e}); trying CLI tools"),
        }

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
            "no working paste path (portal denied and wtype/ydotool/xdotool absent); \
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
