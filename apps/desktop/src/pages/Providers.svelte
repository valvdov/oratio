<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { testPolishProvider, type Settings } from "../lib/ipc";

  let { settings, save }: { settings: Settings; save: () => void } = $props();

  let testResult = $state<Record<string, string>>({});
  let testing = $state<string | null>(null);

  interface OllamaStatus {
    installed: boolean;
    running: boolean;
    models: string[];
  }
  let ollama = $state<OllamaStatus | null>(null);
  let busy = $state<"install" | "pull" | null>(null);
  let progressText = $state("");
  let pullName = $state("");

  const recommended = [
    { name: "qwen3:4b-instruct", label: "qwen3 4B — recommended, ~2.5 GB" },
    { name: "qwen3:1.7b", label: "qwen3 1.7B — light, ~1.4 GB" },
  ];

  async function refreshOllama() {
    ollama = await invoke<OllamaStatus>("ollama_status");
  }

  onMount(() => {
    refreshOllama();
    const unlisten = listen<{ stage: string; done: number; total: number; detail: string }>(
      "ollama://progress",
      (e) => {
        const p = e.payload;
        if (p.stage === "download") {
          progressText =
            p.total > 0
              ? `Downloading Ollama… ${Math.round((p.done / p.total) * 100)}% of ${(p.total / 1e9).toFixed(1)} GB`
              : `Downloading Ollama… ${(p.done / 1e6).toFixed(0)} MB`;
        } else if (p.stage.startsWith("pull:")) {
          const pct = p.total > 0 ? ` ${Math.round((p.done / p.total) * 100)}%` : "";
          progressText = `${p.stage.slice(5)}: ${p.detail}${pct}`;
        } else if (p.stage === "done") {
          progressText = "";
          refreshOllama();
        }
      },
    );
    return () => {
      unlisten.then((f) => f());
    };
  });

  async function installOllama() {
    busy = "install";
    progressText = "Starting download…";
    try {
      await invoke("ollama_install");
    } catch (e) {
      progressText = `Install failed: ${e}`;
    } finally {
      busy = null;
      refreshOllama();
    }
  }

  async function pull(name: string) {
    if (!name.trim()) return;
    busy = "pull";
    progressText = `Pulling ${name}…`;
    try {
      await invoke("ollama_pull", { model: name.trim() });
      progressText = "";
    } catch (e) {
      progressText = `Pull failed: ${e}`;
    } finally {
      busy = null;
      refreshOllama();
    }
  }

  function useModel(name: string) {
    const local = settings.polish.providers.find((p) => p.id === "ollama-local");
    if (local) {
      local.model = name;
      settings.polish.active_provider = "ollama-local";
      save();
    }
  }

  async function test(id: string) {
    const provider = settings.polish.providers.find((p) => p.id === id);
    if (!provider) return;
    testing = id;
    testResult[id] = "";
    try {
      const cleaned = await testPolishProvider(
        $state.snapshot(provider),
        settings.polish.timeout_ms,
      );
      testResult[id] = `OK: "${cleaned}"`;
    } catch (e) {
      testResult[id] = `Error: ${e}`;
    } finally {
      testing = null;
    }
  }
</script>

<h2>AI polish</h2>
<p class="page-desc">
  Removes fillers, applies self-corrections, punctuation and lists. Falls back
  to simple cleanup when the provider is unreachable.
</p>

<div class="card">
  <div class="row" style="border: none">
    <div>
      <strong>Local AI engine</strong>
      <div style="font-size: 12px; color: var(--faint)">
        {#if !ollama}
          checking…
        {:else if !ollama.installed}
          Not installed — polish uses simple cleanup until you install it or add an API key
        {:else if ollama.running}
          Running · {ollama.models.length} model(s)
        {:else}
          Installed, starts automatically on next dictation
        {/if}
      </div>
    </div>
    {#if ollama && !ollama.installed}
      <button class="primary" onclick={installOllama} disabled={busy !== null}>
        {busy === "install" ? "Installing…" : "Install"}
      </button>
    {/if}
  </div>

  {#if ollama?.installed}
    {#each ollama.models as model}
      <div class="row">
        <span>{model}</span>
        {#if settings.polish.providers.find((p) => p.id === "ollama-local")?.model === model}
          <span class="chip">active</span>
        {:else}
          <button onclick={() => useModel(model)}>Use</button>
        {/if}
      </div>
    {/each}
    {#each recommended.filter((r) => !ollama?.models.some((m) => m.startsWith(r.name))) as rec}
      <div class="row">
        <span style="color: var(--muted)">{rec.label}</span>
        <button onclick={() => pull(rec.name)} disabled={busy !== null}>Download</button>
      </div>
    {/each}
    <div class="row">
      <input
        style="flex: 1"
        placeholder="Any Ollama model, e.g. llama3.2:3b"
        bind:value={pullName}
        onkeydown={(e) => e.key === "Enter" && pull(pullName)}
      />
      <button onclick={() => pull(pullName)} disabled={busy !== null}>Pull</button>
    </div>
  {/if}

  {#if progressText}
    <div style="font-size: 12.5px; color: var(--muted); padding-top: 8px">{progressText}</div>
  {/if}
</div>

<div class="card">
  <div class="row">
    <label for="enabled">Polish dictations with AI</label>
    <input
      id="enabled"
      type="checkbox"
      bind:checked={settings.polish.enabled}
      onchange={save}
    />
  </div>
  <div class="row">
    <label for="active">Active provider</label>
    <select id="active" bind:value={settings.polish.active_provider} onchange={save}>
      {#each settings.polish.providers as provider}
        <option value={provider.id}>{provider.id}</option>
      {/each}
    </select>
  </div>
  <div class="row">
    <label for="timeout">Timeout (ms), then fallback</label>
    <input
      id="timeout"
      type="number"
      style="width: 100px"
      bind:value={settings.polish.timeout_ms}
      onchange={save}
    />
  </div>
</div>

{#each settings.polish.providers as provider (provider.id)}
  <div class="card">
    <div class="row" style="border: none">
      <strong>{provider.id}</strong>
      <div>
        <button onclick={() => test(provider.id)} disabled={testing === provider.id}>
          {testing === provider.id ? "Testing…" : "Test"}
        </button>
      </div>
    </div>
    <div class="row">
      <label for="{provider.id}-url">Base URL</label>
      <input
        id="{provider.id}-url"
        style="width: 320px"
        bind:value={provider.base_url}
        onchange={save}
      />
    </div>
    <div class="row">
      <label for="{provider.id}-model">Model</label>
      <input
        id="{provider.id}-model"
        style="width: 320px"
        bind:value={provider.model}
        onchange={save}
      />
    </div>
    <div class="row">
      <label for="{provider.id}-key">API key</label>
      <input
        id="{provider.id}-key"
        type="password"
        style="width: 320px"
        placeholder="not set"
        bind:value={provider.api_key}
        onchange={save}
      />
    </div>
    {#if testResult[provider.id]}
      <div
        style="font-size: 12.5px; padding-top: 8px; color: {testResult[
          provider.id
        ].startsWith('OK')
          ? 'var(--muted)'
          : '#c0392b'}"
      >
        {testResult[provider.id]}
      </div>
    {/if}
  </div>
{/each}
