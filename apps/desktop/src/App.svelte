<script lang="ts">
  import "./app.css";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { getSettings, saveSettings, type Settings } from "./lib/ipc";

  // Linux runs without native decorations (GTK CSD buttons are broken under
  // KDE Wayland) — we draw our own titlebar there.
  const customTitlebar = navigator.userAgent.includes("Linux");
  const appWindow = getCurrentWindow();
  import History from "./pages/History.svelte";
  import General from "./pages/General.svelte";
  import Speech from "./pages/Speech.svelte";
  import Providers from "./pages/Providers.svelte";
  import Dictionary from "./pages/Dictionary.svelte";
  import Snippets from "./pages/Snippets.svelte";
  import Styles from "./pages/Styles.svelte";
  import Permissions from "./pages/Permissions.svelte";

  const pages = [
    { id: "history", label: "History", component: History },
    { id: "general", label: "General", component: General },
    { id: "speech", label: "Speech to text", component: Speech },
    { id: "providers", label: "AI polish", component: Providers },
    { id: "dictionary", label: "Dictionary", component: Dictionary },
    { id: "snippets", label: "Snippets", component: Snippets },
    { id: "styles", label: "Styles", component: Styles },
    { id: "permissions", label: "Permissions", component: Permissions },
  ];

  let current = $state("history");
  let settings = $state<Settings | null>(null);
  let saveState = $state<"idle" | "saving" | "saved" | "error">("idle");
  let saveError = $state("");
  let saveTimer: ReturnType<typeof setTimeout> | null = null;

  getSettings().then((s) => (settings = s));

  // The theme IS the mode: Cream/Peach are light, Ember is dark.
  $effect(() => {
    if (!settings) return;
    const theme = settings.appearance.theme;
    document.documentElement.dataset.theme = theme;
    document.documentElement.dataset.mode = theme === "ember" ? "dark" : "light";
  });

  export function scheduleSave() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
      if (!settings) return;
      saveState = "saving";
      try {
        await saveSettings($state.snapshot(settings) as Settings);
        saveState = "saved";
        setTimeout(() => (saveState = "idle"), 1500);
      } catch (e) {
        saveState = "error";
        saveError = String(e);
      }
    }, 400);
  }

  const CurrentPage = $derived(pages.find((p) => p.id === current)!.component);
</script>

{#if customTitlebar}
  <div class="titlebar" data-tauri-drag-region>
    <span class="tb-title" data-tauri-drag-region>Oratio</span>
    <div class="tb-buttons">
      <button class="tb-btn" aria-label="Minimize" onclick={() => appWindow.minimize()}>–</button>
      <button class="tb-btn" aria-label="Maximize" onclick={() => appWindow.toggleMaximize()}>□</button>
      <button class="tb-btn tb-close" aria-label="Close" onclick={() => appWindow.close()}>×</button>
    </div>
  </div>
{/if}

<div class="layout" class:with-titlebar={customTitlebar}>
  <aside>
    <div class="brand">
      <span class="dot"></span>
      Oratio
    </div>
    <nav>
      {#each pages as page}
        <button
          class="nav-item"
          class:active={current === page.id}
          onclick={() => (current = page.id)}
        >
          {page.label}
        </button>
      {/each}
    </nav>
    <div class="save-state">
      {#if saveState === "saving"}Saving…{/if}
      {#if saveState === "saved"}Saved{/if}
      {#if saveState === "error"}<span class="err" title={saveError}>Save failed</span>{/if}
    </div>
  </aside>
  <main>
    {#if settings}
      <CurrentPage {settings} save={scheduleSave} />
    {:else}
      <p style="color: var(--faint)">Loading…</p>
    {/if}
  </main>
</div>

<style>
  .titlebar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 36px;
    padding: 0 6px 0 14px;
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
    user-select: none;
  }
  .tb-title {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--muted);
  }
  .tb-buttons {
    display: flex;
    gap: 2px;
  }
  .tb-btn {
    width: 34px;
    height: 28px;
    background: transparent;
    color: var(--muted);
    font-size: 15px;
    line-height: 1;
    padding: 0;
    border-radius: 6px;
  }
  .tb-btn:hover {
    background: var(--surface);
  }
  .tb-close:hover {
    background: #c0392b;
    color: #fff;
  }
  .layout {
    display: flex;
    height: 100vh;
  }
  .layout.with-titlebar {
    height: calc(100vh - 36px);
  }
  aside {
    width: 176px;
    flex-shrink: 0;
    border-right: 1px solid var(--border);
    padding: 16px 10px;
    display: flex;
    flex-direction: column;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
    font-size: 15px;
    padding: 0 8px 14px;
    color: var(--accent);
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--accent);
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
  }
  .nav-item {
    text-align: left;
    background: transparent;
    color: var(--muted);
    padding: 7px 10px;
    border-radius: 7px;
    font-size: 13.5px;
  }
  .nav-item:hover {
    background: var(--surface-2);
    filter: none;
  }
  .nav-item.active {
    background: var(--surface-2);
    color: var(--text);
    font-weight: 500;
  }
  .save-state {
    font-size: 12px;
    color: var(--faint);
    padding: 8px;
    min-height: 28px;
  }
  .err {
    color: #c0392b;
  }
  main {
    flex: 1;
    overflow-y: auto;
    padding: 28px 32px;
  }
</style>
