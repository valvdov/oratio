use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

const TRAY_ID: &str = "oratio-tray";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let dictate = MenuItemBuilder::with_id("dictate", "Start / stop dictation").build(app)?;
    let open = MenuItemBuilder::with_id("open", "Open Oratio").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Oratio").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&dictate, &open, &quit])
        .build()?;

    // macOS gets the monochrome template (menu bar recolors it); elsewhere a
    // black-on-transparent glyph is invisible on dark panels — use the colored icon.
    #[cfg(target_os = "macos")]
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    #[cfg(not(target_os = "macos"))]
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))?;

    let builder = TrayIconBuilder::with_id(TRAY_ID).icon(icon);
    #[cfg(target_os = "macos")]
    let builder = builder.icon_as_template(true);

    builder
        .tooltip("Oratio")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "dictate" => crate::pipeline::toggle_dictation(app),
            "open" => {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

/// Reflect recording state in the menu bar (red dot next to the mic while recording).
pub fn set_recording(app: &AppHandle, recording: bool) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_title(if recording { Some("●") } else { None::<&str> });
    }
}
