# Oratio

Voice dictation that types for you — anywhere. Hold a hotkey, speak (Russian,
English, or both mixed), release — clean, punctuated text appears in whatever
app you were typing in.

Local-first: speech recognition (whisper.cpp) and AI text polish (Ollama) run
on your machine. No accounts, no cloud required — cloud LLM providers
(OpenRouter, Gemini) are optional plug-ins via API key.

## Features

- **Global hotkey** — hold `Ctrl+Alt+Space` for push-to-talk or tap to toggle;
  add `Shift` to insert the raw transcript without AI polish
- **Speech to text** — whisper.cpp (`large-v3-turbo` by default), tuned for
  RU+EN code-switched speech («запушь в репозиторий и задеплой на прод» →
  tech terms come out in Latin script); Metal-accelerated on Apple Silicon
- **Streaming transcription** — long dictations are split at natural pauses
  and transcribed while you are still speaking
- **AI polish** — a local LLM removes fillers («эээ», «ну», "um"), applies
  self-corrections («в два… нет, в три» → «в три»), adds punctuation, and
  formats spoken lists. One click installs the local engine (Ollama) and
  downloads models from Settings; falls back to regex cleanup when offline
- **Dictionary** — your terms and names, spelled exactly right, fed to both
  the recognizer and the polish step
- **Snippets** — say a trigger phrase, insert canned text verbatim
- **Styles** — formal/casual/custom tone, switchable per app (by bundle id)
- **History** — every dictation stored locally in SQLite with full-text search
  that understands Russian morphology («депло» finds «задеплоил»)
- **Recording pill** — a floating indicator with live waveform and stop/cancel
  buttons that never steals focus
- **Three themes** — Cream, Peach (light) and Ember (dark)

## Platforms

| Platform | Status |
|---|---|
| macOS (Apple Silicon) | ✅ daily-driver ready |
| Windows 11 (x64 / ARM64) | ✅ builds and runs; ARM64 needs the clang toolchain (see BUILDING) |
| Linux — Wayland KDE/GNOME | 🧪 in testing: hotkeys via XDG GlobalShortcuts portal, paste via wtype/ydotool |
| Linux — X11 | 🧪 hotkeys via standard grabs, paste via xdotool |
| iOS (keyboard extension) | 🔜 planned (Phase 3) |

## Quick start

See [BUILDING.md](BUILDING.md) for per-platform toolchain setup. Short version:

```sh
cd apps/desktop
npm install
npm run tauri dev
```

First run: download a whisper model in **Settings → Speech to text**, then
(optionally) click **Install** under **Settings → AI polish → Local AI engine**
and download a polish model (qwen3 4B recommended, 1.7B for weaker machines).

On Linux install a paste helper: `wtype` (KDE/wlroots) or
[ydotool](https://github.com/ReimuNotMoe/ydotool) (GNOME), and approve the
global-shortcut binding when the system dialog appears.

## Architecture

Cargo workspace; all product logic lives in the platform-agnostic
`crates/oratio-core` (audio capture → VAD → whisper → LLM polish → history),
reused later by the iOS app via UniFFI. The Tauri app in `apps/desktop` owns
only OS integration: hotkeys, text injection, tray, permissions, settings UI
(Svelte 5).

```
hotkey ──► always-fresh mic stream ──► 16 kHz mono ──► Silero VAD segmenting
       ──► whisper.cpp (streaming, per-segment) ──► snippet match / LLM polish
       ──► clipboard + synthesized paste into the focused app ──► history (FTS5)
```

`crates/oratio-cli` is a headless harness for debugging the pipeline
(`oratio-cli listen`, `transcribe`, `stream`, `polish`).

## License

MIT
