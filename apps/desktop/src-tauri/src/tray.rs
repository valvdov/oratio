use tauri::menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use crate::pipeline::AppState;

const TRAY_ID: &str = "oratio-tray";

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let dictate = MenuItemBuilder::with_id("dictate", "Start / stop dictation").build(app)?;
    let open = MenuItemBuilder::with_id("open", "Open Oratio").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Oratio").build(app)?;

    // Style switcher mirrors settings.styles: neutral + every defined style.
    let (default_style, style_ids) = {
        let state = app.state::<AppState>();
        let s = state.settings.lock().unwrap();
        (
            s.styles.default.clone(),
            s.styles
                .styles
                .iter()
                .map(|st| st.id.clone())
                .collect::<Vec<_>>(),
        )
    };
    let mut styles_menu = SubmenuBuilder::new(app, "Style");
    let neutral = CheckMenuItemBuilder::with_id("style:", "Neutral")
        .checked(default_style.is_empty())
        .build(app)?;
    styles_menu = styles_menu.item(&neutral);
    for id in &style_ids {
        let item = CheckMenuItemBuilder::with_id(format!("style:{id}"), id)
            .checked(&default_style == id)
            .build(app)?;
        styles_menu = styles_menu.item(&item);
    }
    let styles_menu = styles_menu.build()?;

    MenuBuilder::new(app)
        .items(&[&dictate, &styles_menu, &open, &quit])
        .build()
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
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
        .menu(&build_menu(app)?)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            if let Some(style_id) = id.strip_prefix("style:") {
                set_default_style(app, style_id);
                return;
            }
            match id {
                "dictate" => crate::pipeline::toggle_dictation(app),
                "open" => {
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .build(app)?;
    Ok(())
}

fn set_default_style(app: &AppHandle, style_id: &str) {
    let settings = {
        let state = app.state::<AppState>();
        let mut s = state.settings.lock().unwrap();
        s.styles.default = style_id.to_string();
        s.clone()
    };
    if let Err(e) = settings.save(&oratio_core::settings::settings_path()) {
        tracing::warn!("could not persist style change: {e}");
    }
    tracing::info!(
        "default style set to '{}'",
        if style_id.is_empty() { "neutral" } else { style_id }
    );
    refresh_menu(app);
}

/// Rebuild the tray menu (checkmarks / style list changed).
pub fn refresh_menu(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        match build_menu(app) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(e) => tracing::warn!("tray menu rebuild failed: {e}"),
        }
    }
}

/// Reflect recording state in the menu bar (red dot next to the mic while recording).
pub fn set_recording(app: &AppHandle, recording: bool) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_title(if recording { Some("●") } else { None::<&str> });
    }
}
