import { invoke } from "@tauri-apps/api/core";

export interface ProviderConfig {
  id: string;
  base_url: string;
  model: string;
  api_key: string | null;
  keep_alive?: string | null;
}

export interface Snippet {
  trigger: string;
  expansion: string;
}

export interface Style {
  id: string;
  instruction: string;
}

export interface Settings {
  version: number;
  hotkeys: { main: string; toggle_threshold_ms: number };
  stt: {
    model: string;
    language: string;
    min_speech_ms: number;
    keep_model_loaded: boolean;
  };
  polish: {
    enabled: boolean;
    active_provider: string;
    timeout_ms: number;
    providers: ProviderConfig[];
  };
  dictionary: string[];
  snippets: Snippet[];
  styles: {
    default: string;
    per_app: Record<string, string>;
    styles: Style[];
  };
  appearance: { theme: string; mode: string; pill_bottom_margin: number };
  insertion: { restore_clipboard_ms: number };
  sound_cues: boolean;
}

export interface ModelInfo {
  name: string;
  size_mb: number;
  downloaded: boolean;
}

export const getSettings = () => invoke<Settings>("get_settings");
export const saveSettings = (settings: Settings) =>
  invoke<void>("save_settings", { newSettings: settings });
export const listModels = () => invoke<ModelInfo[]>("list_models");
export const downloadModel = (name: string) =>
  invoke<void>("download_model", { name });
export const testPolishProvider = (provider: ProviderConfig, timeoutMs: number) =>
  invoke<string>("test_polish_provider", { provider, timeoutMs });
export const permissionsStatus = () =>
  invoke<{ accessibility: boolean }>("permissions_status");
