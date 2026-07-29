pub mod commands;
pub mod frontmost;
#[cfg(target_os = "linux")]
pub mod hotkey_linux;
pub mod inject;
pub mod permissions;
pub mod pipeline;
pub mod tray;

use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use oratio_core::settings::{settings_path, Settings};

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| {
                    "info,oratio_core=debug,oratio_desktop=debug,zbus=error".into()
                }),
        )
        .init();

    let settings = Settings::load(&settings_path());
    // Persist defaults on first run so the user has a file to look at.
    if !settings_path().exists() {
        if let Err(e) = settings.save(&settings_path()) {
            tracing::warn!("could not write default settings: {e}");
        }
    }

    let builder = tauri::Builder::default().plugin(tauri_plugin_single_instance::init(
        |app, _args, _cwd| {
            // A second launch just brings up the settings window of the running app.
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
        },
    ));
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());
    let builder = builder.plugin(tauri_plugin_autostart::init(
        tauri_plugin_autostart::MacosLauncher::LaunchAgent,
        None,
    ));

    // Main hotkey polishes via LLM; the same combo with Shift inserts the raw transcript.
    let main_hotkey: Shortcut = settings
        .hotkeys
        .main
        .parse()
        .unwrap_or_else(|_| "Ctrl+Alt+Space".parse().unwrap());
    let raw_hotkey: Option<Shortcut> = if settings.hotkeys.main.contains("Shift") {
        None
    } else {
        format!("Shift+{}", settings.hotkeys.main).parse().ok()
    };

    builder
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    let is_raw = app
                        .try_state::<pipeline::AppState>()
                        .map(|st| {
                            st.raw_shortcut
                                .lock()
                                .unwrap()
                                .map(|r| shortcut == &r)
                                .unwrap_or(false)
                        })
                        .unwrap_or(false);
                    match event.state() {
                        ShortcutState::Pressed => pipeline::on_hotkey_press(app, is_raw),
                        ShortcutState::Released => pipeline::on_hotkey_release(app),
                    }
                })
                .build(),
        )
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let state = pipeline::AppState::new(settings.clone(), app.handle().clone());
            *state.raw_shortcut.lock().unwrap() = raw_hotkey;
            app.manage(state);

            tray::create(app.handle())?;
            position_pill(app.handle());
            permissions::ensure_accessibility_prompt();

            // On Wayland the plugin cannot grab keys — go through the XDG portal.
            #[cfg(target_os = "linux")]
            let use_portal = hotkey_linux::is_wayland();
            #[cfg(not(target_os = "linux"))]
            let use_portal = false;

            if use_portal {
                #[cfg(target_os = "linux")]
                hotkey_linux::spawn(app.handle().clone(), settings.hotkeys.main.clone());
            } else {
                if let Err(e) = app.global_shortcut().register(main_hotkey) {
                    tracing::error!("hotkey registration failed: {e}");
                }
                if let Some(raw) = raw_hotkey {
                    let _ = app.global_shortcut().register(raw);
                }
                tracing::info!(
                    "registered global hotkey {} (with Shift = raw, no polish)",
                    settings.hotkeys.main
                );
            }

            if settings.stt.keep_model_loaded {
                pipeline::preload_engine(app.handle().clone());
            }
            pipeline::warm_up_polish(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pipeline::app_status,
            pipeline::stop_dictation,
            pipeline::cancel_dictation,
            commands::get_settings,
            commands::save_settings,
            commands::list_models,
            commands::download_model,
            commands::test_polish_provider,
            commands::permissions_status,
            commands::history_search,
            commands::history_delete,
            commands::history_count,
            commands::copy_text,
            commands::ollama_status,
            commands::ollama_install,
            commands::ollama_pull
        ])
        .on_window_event(|window, event| {
            // Closing the settings window hides it; the app lives in the tray.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Oratio");
}

fn position_pill(app: &tauri::AppHandle) {
    let Some(pill) = app.get_webview_window("pill") else {
        return;
    };
    // Convert the pill into a non-activating NSPanel so its buttons can be
    // clicked without stealing focus from the app we paste into.
    #[cfg(target_os = "macos")]
    {
        use tauri_nspanel::WebviewWindowExt;
        match pill.to_panel() {
            Ok(panel) => {
                #[allow(non_upper_case_globals)]
                const NSWindowStyleMaskNonactivatingPanel: i32 = 1 << 7;
                panel.set_style_mask(NSWindowStyleMaskNonactivatingPanel);
                panel.set_floating_panel(true);
            }
            Err(e) => tracing::warn!("pill panel conversion failed: {e:?}"),
        }
    }
    // Wayland forbids clients from positioning their own windows — the pill
    // would open dead-center. The layer-shell protocol (supported by KWin and
    // wlroots compositors) pins it to the bottom edge as a proper overlay.
    #[cfg(target_os = "linux")]
    {
        use gtk_layer_shell::LayerShell;
        match pill.gtk_window() {
            Ok(gtk_win) => {
                gtk_win.init_layer_shell();
                gtk_win.set_layer(gtk_layer_shell::Layer::Overlay);
                gtk_win.set_anchor(gtk_layer_shell::Edge::Bottom, true);
                tracing::info!("pill attached to the bottom edge via layer-shell");
            }
            Err(e) => tracing::warn!("layer-shell setup failed ({e}); pill position may be off"),
        }
    }

    apply_pill_position(app);
}

/// (Re)apply the pill's distance from the bottom edge from settings.
pub fn apply_pill_position(app: &tauri::AppHandle) {
    let Some(pill) = app.get_webview_window("pill") else {
        return;
    };
    let margin = app
        .try_state::<pipeline::AppState>()
        .map(|st| st.settings.lock().unwrap().appearance.pill_bottom_margin)
        .unwrap_or(24);

    #[cfg(target_os = "linux")]
    {
        use gtk_layer_shell::LayerShell;
        if let Ok(gtk_win) = pill.gtk_window() {
            gtk_win.set_layer_shell_margin(gtk_layer_shell::Edge::Bottom, margin as i32);
            return;
        }
    }

    if let Ok(Some(monitor)) = pill.primary_monitor() {
        let scale = monitor.scale_factor();
        // work_area excludes the Dock and the menu bar.
        let area_pos = monitor.work_area().position.to_logical::<f64>(scale);
        let area = monitor.work_area().size.to_logical::<f64>(scale);
        let pill_size = pill.outer_size().map(|s| s.to_logical::<f64>(scale));
        let (w, h) = pill_size.map(|s| (s.width, s.height)).unwrap_or((330.0, 64.0));
        let x = area_pos.x + (area.width - w) / 2.0;
        let y = area_pos.y + area.height - h - margin as f64;
        let _ = pill.set_position(tauri::LogicalPosition::new(x, y));
    }
}
