<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { Settings } from "../lib/ipc";

  let { settings, save }: { settings: Settings; save: () => void } = $props();
  void settings;
  void save;

  interface Entry {
    id: number;
    created_at: string;
    app_bundle_id: string | null;
    raw_text: string;
    polished_text: string | null;
    duration_ms: number | null;
  }

  const PAGE = 30;
  let query = $state("");
  let entries = $state<Entry[]>([]);
  let total = $state(0);
  let exhausted = $state(false);
  let showRaw = $state<Record<number, boolean>>({});
  let copied = $state<number | null>(null);
  let searchTimer: ReturnType<typeof setTimeout> | null = null;

  async function load(reset: boolean) {
    const offset = reset ? 0 : entries.length;
    const batch = await invoke<Entry[]>("history_search", {
      query,
      limit: PAGE,
      offset,
    });
    entries = reset ? batch : [...entries, ...batch];
    exhausted = batch.length < PAGE;
    total = await invoke<number>("history_count");
  }

  onMount(() => {
    load(true);
    const timer = setInterval(() => {
      if (!query) load(true);
    }, 5000);
    return () => clearInterval(timer);
  });

  function onSearch() {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => load(true), 250);
  }

  async function copy(entry: Entry) {
    await invoke("copy_text", { text: entry.polished_text ?? entry.raw_text });
    copied = entry.id;
    setTimeout(() => (copied = null), 1200);
  }

  async function remove(entry: Entry) {
    await invoke("history_delete", { id: entry.id });
    entries = entries.filter((e) => e.id !== entry.id);
    total -= 1;
  }

  function appName(bundle: string | null): string {
    if (!bundle) return "";
    const part = bundle.split(".").pop() ?? bundle;
    return part.charAt(0).toUpperCase() + part.slice(1);
  }

  function timeAgo(iso: string): string {
    const then = new Date(iso.replace(" ", "T") + "Z").getTime();
    const mins = Math.round((Date.now() - then) / 60000);
    if (mins < 1) return "now";
    if (mins < 60) return `${mins}m ago`;
    const hours = Math.round(mins / 60);
    if (hours < 24) return `${hours}h ago`;
    return `${Math.round(hours / 24)}d ago`;
  }
</script>

<h2>History</h2>
<p class="page-desc">{total} dictations, stored locally only.</p>

<input
  style="width: 100%; margin-bottom: 14px"
  placeholder="Search… («депло» найдёт «задеплоил»)"
  bind:value={query}
  oninput={onSearch}
/>

{#each entries as entry (entry.id)}
  <div class="card entry">
    <div class="text">{showRaw[entry.id] ? entry.raw_text : (entry.polished_text ?? entry.raw_text)}</div>
    <div class="meta">
      <span>
        {timeAgo(entry.created_at)}
        {#if entry.app_bundle_id}
          · {appName(entry.app_bundle_id)}{/if}
        {#if entry.duration_ms}
          · {(entry.duration_ms / 1000).toFixed(0)}s{/if}
        {#if showRaw[entry.id]}
          · raw{/if}
      </span>
      <span class="actions">
        {#if entry.polished_text}
          <button class="ghost" onclick={() => (showRaw[entry.id] = !showRaw[entry.id])}>
            {showRaw[entry.id] ? "polished" : "raw"}
          </button>
        {/if}
        <button class="ghost" onclick={() => copy(entry)}>
          {copied === entry.id ? "copied!" : "copy"}
        </button>
        <button class="ghost" onclick={() => remove(entry)}>delete</button>
      </span>
    </div>
  </div>
{/each}

{#if entries.length === 0}
  <p style="color: var(--faint)">
    {query ? "Nothing found." : "No dictations yet — hold the hotkey and speak."}
  </p>
{/if}

{#if !exhausted && entries.length > 0}
  <button onclick={() => load(false)}>Load more</button>
{/if}

<style>
  .entry {
    padding: 12px 14px;
  }
  .text {
    white-space: pre-wrap;
    user-select: text;
    line-height: 1.5;
  }
  .meta {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-top: 8px;
    font-size: 12px;
    color: var(--faint);
  }
  .actions button {
    font-size: 12px;
    padding: 2px 8px;
  }
</style>
