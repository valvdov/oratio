//! Wayland global hotkeys via the XDG GlobalShortcuts portal.
//!
//! The regular global-shortcut plugin only works on X11. On Wayland
//! (GNOME/KDE) shortcuts must be requested from the desktop portal, which
//! emits Activated/Deactivated signals — exactly what hold-to-talk needs.
//! The user confirms the binding once in a system dialog and can rebind it
//! in the desktop's own shortcut settings.

#![cfg(target_os = "linux")]

use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures_util::StreamExt;

pub fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
}

pub fn spawn(app: tauri::AppHandle, trigger: String) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run(app, trigger).await {
            tracing::warn!(
                "GlobalShortcuts portal failed ({e}); use the tray menu to dictate \
                 or run an X11 session"
            );
        }
    });
}

async fn run(app: tauri::AppHandle, trigger: String) -> Result<(), ashpd::Error> {
    let shortcuts = GlobalShortcuts::new().await?;
    let session = shortcuts.create_session().await?;

    // Portal trigger hint format, e.g. "CTRL+ALT+space".
    let hint = trigger.replace(' ', "").to_uppercase();
    let new_shortcuts = [
        NewShortcut::new("dictate", "Oratio: dictate (hold or tap)")
            .preferred_trigger(hint.as_str()),
        NewShortcut::new("dictate-raw", "Oratio: dictate without AI polish"),
    ];
    shortcuts
        .bind_shortcuts(&session, &new_shortcuts, None)
        .await?;
    tracing::info!("Wayland global shortcuts bound via portal");

    enum Ev {
        Press(bool),
        Release,
    }
    let activated = shortcuts.receive_activated().await?;
    let deactivated = shortcuts.receive_deactivated().await?;
    let a = activated.map(|s| Ev::Press(s.shortcut_id() == "dictate-raw"));
    let d = deactivated.map(|_| Ev::Release);
    let mut events = futures_util::stream::select(a, d);

    while let Some(ev) = events.next().await {
        match ev {
            Ev::Press(raw) => crate::pipeline::on_hotkey_press(&app, raw),
            Ev::Release => crate::pipeline::on_hotkey_release(&app),
        }
    }
    tracing::warn!("global shortcuts signal stream ended");
    Ok(())
}
