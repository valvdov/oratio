use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use oratio_core::models;
use oratio_core::settings::{settings_path, Settings};

use crate::pipeline::AppState;

#[tauri::command]
pub fn get_settings(state: tauri::State<'_, AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, new_settings: Settings) -> Result<(), String> {
    let state = app.state::<AppState>();

    let main: Shortcut = new_settings
        .hotkeys
        .main
        .parse()
        .map_err(|e| format!("invalid hotkey '{}': {e}", new_settings.hotkeys.main))?;
    let raw: Option<Shortcut> = if new_settings.hotkeys.main.contains("Shift") {
        None
    } else {
        format!("Shift+{}", new_settings.hotkeys.main).parse().ok()
    };

    let (old_hotkey, old_model) = {
        let s = state.settings.lock().unwrap();
        (s.hotkeys.main.clone(), s.stt.model.clone())
    };

    if old_hotkey != new_settings.hotkeys.main {
        app.global_shortcut()
            .unregister_all()
            .map_err(|e| e.to_string())?;
        app.global_shortcut().register(main).map_err(|e| {
            format!("could not register '{}': {e}", new_settings.hotkeys.main)
        })?;
        if let Some(r) = raw {
            let _ = app.global_shortcut().register(r);
        }
        *state.raw_shortcut.lock().unwrap() = raw;
        tracing::info!("hotkey changed to {}", new_settings.hotkeys.main);
    }

    if old_model != new_settings.stt.model {
        // Drop the loaded engine; it reloads with the new model on next use.
        state.engine.lock().unwrap().take();
        crate::pipeline::preload_engine(app.clone());
    }

    *state.settings.lock().unwrap() = new_settings.clone();
    new_settings.save(&settings_path()).map_err(|e| e.to_string())?;
    let _ = app.emit("settings://changed", &new_settings.appearance);
    crate::apply_pill_position(&app);
    Ok(())
}

#[derive(serde::Serialize)]
pub struct ModelInfo {
    name: String,
    size_mb: u64,
    downloaded: bool,
}

#[tauri::command]
pub fn list_models() -> Vec<ModelInfo> {
    models::CATALOG
        .iter()
        .map(|spec| ModelInfo {
            name: spec.name.to_string(),
            size_mb: spec.approx_size / (1024 * 1024),
            downloaded: models::is_downloaded(spec),
        })
        .collect()
}

#[derive(Clone, serde::Serialize)]
struct DownloadProgress {
    name: String,
    done: u64,
    total: u64,
}

#[tauri::command]
pub async fn download_model(app: AppHandle, name: String) -> Result<(), String> {
    let spec = models::find(&name).map_err(|e| e.to_string())?;
    let emit_app = app.clone();
    let model_name = name.clone();
    models::download(spec, move |done, total| {
        let _ = emit_app.emit(
            "models://progress",
            DownloadProgress {
                name: model_name.clone(),
                done,
                total,
            },
        );
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn test_polish_provider(
    provider: oratio_core::polish::openai_compat::ProviderConfig,
    timeout_ms: u64,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        oratio_core::polish::ollama::ensure_local_running(&provider.base_url, 15);
        oratio_core::polish::openai_compat::test_provider(&provider, timeout_ms.max(20_000))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Serialize)]
pub struct OllamaStatus {
    installed: bool,
    running: bool,
    models: Vec<String>,
}

fn ollama_base_url(state: &tauri::State<'_, AppState>) -> String {
    let s = state.settings.lock().unwrap();
    s.polish
        .providers
        .iter()
        .find(|p| p.id == "ollama-local")
        .map(|p| p.base_url.clone())
        .unwrap_or_else(|| "http://127.0.0.1:11434/v1".into())
}

#[tauri::command]
pub async fn ollama_status(app: AppHandle) -> OllamaStatus {
    let base_url = {
        let state = app.state::<AppState>();
        ollama_base_url(&state)
    };
    tauri::async_runtime::spawn_blocking(move || {
        let installed = oratio_core::polish::ollama::is_installed();
        let models = oratio_core::polish::ollama::list_models(&base_url).unwrap_or_default();
        let running = !models.is_empty()
            || oratio_core::polish::ollama::list_models(&base_url).is_ok();
        OllamaStatus {
            installed,
            running,
            models,
        }
    })
    .await
    .unwrap_or(OllamaStatus {
        installed: false,
        running: false,
        models: vec![],
    })
}

#[derive(Clone, serde::Serialize)]
struct OllamaProgress {
    stage: String,
    done: u64,
    total: u64,
    detail: String,
}

#[tauri::command]
pub async fn ollama_install(app: AppHandle) -> Result<(), String> {
    let emit_app = app.clone();
    oratio_core::polish::ollama::install(move |done, total| {
        let _ = emit_app.emit(
            "ollama://progress",
            OllamaProgress {
                stage: "download".into(),
                done,
                total,
                detail: String::new(),
            },
        );
    })
    .await
    .map_err(|e| e.to_string())?;

    // Bring the server up right away so model pulls can start.
    let state = app.state::<AppState>();
    let base_url = ollama_base_url(&state);
    tauri::async_runtime::spawn_blocking(move || {
        oratio_core::polish::ollama::ensure_local_running(&base_url, 20);
    })
    .await
    .ok();
    let _ = app.emit(
        "ollama://progress",
        OllamaProgress {
            stage: "done".into(),
            done: 0,
            total: 0,
            detail: String::new(),
        },
    );
    Ok(())
}

#[tauri::command]
pub async fn ollama_pull(app: AppHandle, model: String) -> Result<(), String> {
    let base_url = {
        let state = app.state::<AppState>();
        ollama_base_url(&state)
    };
    {
        let url = base_url.clone();
        tauri::async_runtime::spawn_blocking(move || {
            oratio_core::polish::ollama::ensure_local_running(&url, 20)
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    let emit_app = app.clone();
    let model_name = model.clone();
    oratio_core::polish::ollama::pull_model(&base_url, &model, move |done, total, status| {
        let _ = emit_app.emit(
            "ollama://progress",
            OllamaProgress {
                stage: format!("pull:{model_name}"),
                done,
                total,
                detail: status,
            },
        );
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn history_search(
    state: tauri::State<'_, AppState>,
    query: String,
    limit: u32,
    offset: u32,
) -> Result<Vec<oratio_core::history::HistoryEntry>, String> {
    let guard = state.history.lock().unwrap();
    let Some(history) = guard.as_ref() else {
        return Ok(Vec::new());
    };
    history.search(&query, limit, offset).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn history_delete(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    let guard = state.history.lock().unwrap();
    if let Some(history) = guard.as_ref() {
        history.delete(id).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn history_count(state: tauri::State<'_, AppState>) -> Result<i64, String> {
    let guard = state.history.lock().unwrap();
    match guard.as_ref() {
        Some(history) => history.count().map_err(|e| e.to_string()),
        None => Ok(0),
    }
}

#[tauri::command]
pub fn copy_text(text: String) -> Result<(), String> {
    crate::inject::copy_only(&text).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn permissions_status() -> serde_json::Value {
    serde_json::json!({
        "accessibility": crate::permissions::accessibility_ok(),
    })
}
