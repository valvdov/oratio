use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::polish::openai_compat::ProviderConfig;
use crate::{paths, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub hotkeys: Hotkeys,
    pub stt: SttSettings,
    pub polish: PolishSettings,
    pub dictionary: Vec<String>,
    pub snippets: Vec<crate::snippets::Snippet>,
    pub styles: crate::styles::StyleSettings,
    pub appearance: AppearanceSettings,
    pub insertion: InsertionSettings,
    /// Subtle sounds on record start/stop.
    pub sound_cues: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    /// "cream" | "ember" | "peach"
    pub theme: String,
    /// "system" | "light" | "dark"
    pub mode: String,
    /// Distance of the recording pill from the bottom screen edge (px).
    pub pill_bottom_margin: u32,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: "cream".into(),
            mode: "system".into(),
            pill_bottom_margin: 24,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PolishSettings {
    pub enabled: bool,
    /// Provider id from `providers`; falls back to regex cleanup when the
    /// provider is unreachable or exceeds `timeout_ms`.
    pub active_provider: String,
    pub timeout_ms: u64,
    pub providers: Vec<ProviderConfig>,
}

impl Default for PolishSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            active_provider: "ollama-local".into(),
            timeout_ms: 6000,
            providers: vec![
                ProviderConfig {
                    id: "ollama-local".into(),
                    base_url: "http://127.0.0.1:11434/v1".into(),
                    model: "qwen3:4b-instruct".into(),
                    api_key: None,
                    keep_alive: Some("2h".into()),
                },
                ProviderConfig {
                    id: "openrouter".into(),
                    base_url: "https://openrouter.ai/api/v1".into(),
                    model: "meta-llama/llama-3.3-70b-instruct:free".into(),
                    api_key: None,
                    keep_alive: None,
                },
                ProviderConfig {
                    id: "gemini".into(),
                    base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
                    model: "gemini-2.0-flash".into(),
                    api_key: None,
                    keep_alive: None,
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    /// Shortcut in Tauri accelerator format. Short press toggles, hold = push-to-talk.
    pub main: String,
    /// Press shorter than this (ms) counts as a toggle, longer as push-to-talk.
    pub toggle_threshold_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SttSettings {
    pub model: String,
    pub language: String,
    /// Recordings with less detected speech than this are dropped (hallucination guard).
    pub min_speech_ms: u32,
    pub keep_model_loaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InsertionSettings {
    /// Delay before restoring the previous clipboard contents after paste.
    pub restore_clipboard_ms: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            hotkeys: Hotkeys::default(),
            stt: SttSettings::default(),
            polish: PolishSettings::default(),
            dictionary: Vec::new(),
            snippets: Vec::new(),
            styles: crate::styles::StyleSettings::default(),
            appearance: AppearanceSettings::default(),
            insertion: InsertionSettings::default(),
            sound_cues: true,
        }
    }
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            main: "Ctrl+Alt+Space".into(),
            toggle_threshold_ms: 350,
        }
    }
}

impl Default for SttSettings {
    fn default() -> Self {
        Self {
            model: "large-v3-turbo-q5_0".into(),
            language: "ru".into(),
            min_speech_ms: 300,
            keep_model_loaded: true,
        }
    }
}

impl Default for InsertionSettings {
    fn default() -> Self {
        Self {
            restore_clipboard_ms: 900,
        }
    }
}

pub fn settings_path() -> PathBuf {
    paths::data_dir().join("settings.json")
}

impl Settings {
    /// Load settings, falling back to defaults when the file is missing or invalid.
    /// API keys come from the OS credential store when the file has none.
    pub fn load(path: &Path) -> Self {
        let mut settings: Self = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("settings.json is invalid ({e}), using defaults");
                Self::default()
            }),
            Err(_) => Self::default(),
        };
        for provider in &mut settings.polish.providers {
            let missing = provider
                .api_key
                .as_deref()
                .map(|k| k.trim().is_empty())
                .unwrap_or(true);
            if missing {
                if let Some(key) = crate::secrets::get(&provider.id) {
                    provider.api_key = Some(key);
                }
            }
        }
        settings
    }

    /// Persist settings. Keys go to the credential store; the file keeps them
    /// only when the store is unavailable (e.g. no Secret Service on Linux).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut on_disk = self.clone();
        for provider in &mut on_disk.polish.providers {
            match provider.api_key.as_deref().map(str::trim) {
                Some(key) if !key.is_empty() => {
                    if crate::secrets::set(&provider.id, key) {
                        provider.api_key = None;
                    }
                }
                _ => {
                    crate::secrets::delete(&provider.id);
                    provider.api_key = None;
                }
            }
        }
        let text = serde_json::to_string_pretty(&on_disk).expect("settings serialize");
        std::fs::write(path, text)?;
        Ok(())
    }
}
