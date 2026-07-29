# Building Oratio

## macOS (Apple Silicon)

Prereqs: Xcode CLT, Rust, Node 20+, cmake (`brew install cmake`).

```sh
cd apps/desktop
npm install
npm run tauri dev     # development
npm run tauri build   # produces target/release/bundle/macos/Oratio.app
```

Optional local polish: `brew install ollama && ollama pull qwen3:4b-instruct`
(the app auto-starts `ollama serve` when needed).

## Windows 11 (including ARM64)

1. Install [Rust](https://rustup.rs) (host toolchain; on ARM VM it's `aarch64-pc-windows-msvc`).
2. Install **Visual Studio Build Tools** with "Desktop development with C++"
   (MSVC + Windows SDK + **CMake** component — cmake is needed for whisper.cpp).
3. Install [Node.js 20+](https://nodejs.org).
4. WebView2 is preinstalled on Windows 11.

```powershell
git clone <repo> oratio; cd oratio\apps\desktop
npm install
npm run tauri dev     # development
npm run tauri build   # produces an .msi/.exe under target\release\bundle
```

On **ARM64** use the launcher script instead of plain `npm run tauri dev` —
ggml refuses MSVC on ARM and must be built with clang-cl + Ninja:

```powershell
winget install Ninja-build.Ninja
scripts\dev-windows.bat
```

Notes for the first Windows run:
- Whisper runs on CPU there (no Metal); expect slower transcription in a VM —
  switch the model to `small-q5_1` in Settings → Speech to text if turbo is too slow.
- Hotkey hold-mode depends on the global-shortcut plugin's release events on
  Windows; if hold doesn't work, use tap-to-toggle and report it.
- LLM polish: install [Ollama for Windows](https://ollama.com/download/windows)
  and `ollama pull qwen3:4b-instruct`, or paste an OpenRouter/Gemini API key in
  Settings → AI polish. Without either, the regex fallback cleans the text.

## Linux (Wayland: GNOME / KDE)

Prereqs (Ubuntu/Debian):

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev cmake \
  libasound2-dev
```

Arch (system `rust` package works fine — do not add rustup on top of it):

```sh
sudo pacman -Syu --needed webkit2gtk-4.1 base-devel curl wget file openssl \
  appmenu-gtk-module libappindicator-gtk3 librsvg cmake alsa-lib clang \
  nodejs npm wtype gtk-layer-shell
```

```sh
cd apps/desktop && npm install && npm run tauri dev
```

Current Wayland status:
- Text insertion tries `wtype` (KDE/wlroots), then `ydotool`, then `xdotool` (X11).
  Install one of them: `sudo pacman -S wtype` (KDE) or set up
  [ydotool](https://github.com/ReimuNotMoe/ydotool) with its daemon (GNOME).
- Global hotkeys via the XDG GlobalShortcuts portal are in progress; on X11
  sessions the regular hotkey works already.
