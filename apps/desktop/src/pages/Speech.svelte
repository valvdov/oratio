<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { downloadModel, listModels, type ModelInfo, type Settings } from "../lib/ipc";

  let { settings, save }: { settings: Settings; save: () => void } = $props();

  let models = $state<ModelInfo[]>([]);
  let progress = $state<Record<string, number>>({});

  async function refresh() {
    models = await listModels();
  }

  onMount(() => {
    refresh();
    const unlisten = listen<{ name: string; done: number; total: number }>(
      "models://progress",
      (e) => {
        progress[e.payload.name] = e.payload.total
          ? e.payload.done / e.payload.total
          : 0;
      },
    );
    return () => {
      unlisten.then((f) => f());
    };
  });

  async function download(name: string) {
    progress[name] = 0;
    try {
      await downloadModel(name);
    } finally {
      delete progress[name];
      refresh();
    }
  }
</script>

<h2>Speech to text</h2>
<p class="page-desc">Local whisper models. Larger = better RU+EN accuracy.</p>

<div class="card">
  {#each models as model}
    <div class="row">
      <div>
        <div>{model.name}</div>
        <div style="font-size: 12px; color: var(--faint)">{model.size_mb} MB</div>
      </div>
      {#if progress[model.name] !== undefined}
        <span class="chip">{Math.round(progress[model.name] * 100)}%</span>
      {:else if model.downloaded}
        {#if settings.stt.model === model.name}
          <span class="chip">active</span>
        {:else}
          <button
            onclick={() => {
              settings.stt.model = model.name;
              save();
            }}
          >
            Use
          </button>
        {/if}
      {:else}
        <button onclick={() => download(model.name)}>Download</button>
      {/if}
    </div>
  {/each}
</div>

<div class="card">
  <div class="row">
    <label for="lang">Spoken language</label>
    <select id="lang" bind:value={settings.stt.language} onchange={save}>
      <option value="ru">Русский (+ EN термины)</option>
      <option value="en">English</option>
      <option value="auto">Auto-detect</option>
    </select>
  </div>
  <div class="row">
    <label for="keep">Keep model in memory</label>
    <input
      id="keep"
      type="checkbox"
      bind:checked={settings.stt.keep_model_loaded}
      onchange={save}
    />
  </div>
</div>
