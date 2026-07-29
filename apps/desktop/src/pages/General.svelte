<script lang="ts">
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
  import { onMount } from "svelte";
  import type { Settings } from "../lib/ipc";

  let { settings, save }: { settings: Settings; save: () => void } = $props();

  let autostart = $state(false);
  onMount(async () => {
    try {
      autostart = await isEnabled();
    } catch {}
  });
  async function toggleAutostart(e: Event) {
    const on = (e.target as HTMLInputElement).checked;
    try {
      if (on) await enable();
      else await disable();
      autostart = on;
    } catch (err) {
      autostart = !on;
      console.error(err);
    }
  }

  const themes = [
    { id: "cream", label: "Cream", accent: "#c56a3d", bg: "#f6f1e9" },
    { id: "peach", label: "Peach", accent: "#e2704f", bg: "#fbf3ee" },
    { id: "ember", label: "Ember", accent: "#e8a33d", bg: "#221e19" },
  ];

  let capturing = $state(false);

  function captureHotkey(event: KeyboardEvent) {
    if (!capturing) return;
    event.preventDefault();
    event.stopPropagation();
    if (["Control", "Alt", "Shift", "Meta"].includes(event.key)) return;

    const parts: string[] = [];
    if (event.ctrlKey) parts.push("Ctrl");
    if (event.altKey) parts.push("Alt");
    if (event.metaKey) parts.push("Super");
    if (event.shiftKey) parts.push("Shift");

    let key = event.code;
    if (key.startsWith("Key")) key = key.slice(3);
    else if (key.startsWith("Digit")) key = key.slice(5);
    if (key === "Escape") {
      capturing = false;
      return;
    }
    if (parts.length === 0) return; // require at least one modifier
    parts.push(key);
    settings.hotkeys.main = parts.join("+");
    capturing = false;
    save();
  }
</script>

<svelte:window onkeydown={captureHotkey} />

<h2>General</h2>
<p class="page-desc">Hotkey and appearance.</p>

<div class="card">
  <div class="row">
    <label for="hotkey-btn">Dictation hotkey</label>
    <button
      id="hotkey-btn"
      class:primary={capturing}
      onclick={() => (capturing = !capturing)}
    >
      {capturing ? "Press keys… (Esc to cancel)" : settings.hotkeys.main}
    </button>
  </div>
  <div class="row">
    <label>Raw mode (no AI polish)</label>
    <span class="chip">Shift + {settings.hotkeys.main}</span>
  </div>
  <div class="row">
    <label for="threshold">Tap vs hold threshold (ms)</label>
    <input
      id="threshold"
      type="number"
      style="width: 90px"
      bind:value={settings.hotkeys.toggle_threshold_ms}
      onchange={save}
    />
  </div>
  <div class="row">
    <label for="sounds">Sound cues on start/stop</label>
    <input
      id="sounds"
      type="checkbox"
      bind:checked={settings.sound_cues}
      onchange={save}
    />
  </div>
  <div class="row">
    <label for="autostart">Launch at login</label>
    <input id="autostart" type="checkbox" checked={autostart} onchange={toggleAutostart} />
  </div>
  <div class="row">
    <label for="pill-margin">Pill distance from bottom (px)</label>
    <input
      id="pill-margin"
      type="number"
      min="0"
      max="500"
      style="width: 90px"
      bind:value={settings.appearance.pill_bottom_margin}
      onchange={save}
    />
  </div>
</div>

<div class="card">
  <div class="row" style="border: none">
    <label>Theme</label>
  </div>
  <div class="themes">
    {#each themes as theme}
      <button
        class="theme-card"
        class:selected={settings.appearance.theme === theme.id}
        style="background: {theme.bg}"
        onclick={() => {
          settings.appearance.theme = theme.id;
          settings.appearance.mode = theme.id === "ember" ? "dark" : "light";
          save();
        }}
      >
        <span class="swatch" style="background: {theme.accent}"></span>
        <span class="theme-name" style="color: {theme.accent}">{theme.label}</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .themes {
    display: flex;
    gap: 10px;
    padding-bottom: 10px;
  }
  .theme-card {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 16px 0 12px;
    border-radius: 10px;
    border: 2px solid var(--border);
  }
  .theme-card.selected {
    border-color: var(--accent);
  }
  .swatch {
    width: 22px;
    height: 22px;
    border-radius: 50%;
  }
  .theme-name {
    font-size: 12.5px;
    font-weight: 600;
  }
</style>
