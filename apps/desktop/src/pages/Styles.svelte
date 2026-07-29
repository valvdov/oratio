<script lang="ts">
  import type { Settings } from "../lib/ipc";

  let { settings, save }: { settings: Settings; save: () => void } = $props();

  let perApp = $state(
    Object.entries(settings.styles.per_app).map(([bundle, style]) => ({ bundle, style })),
  );

  function syncPerApp() {
    settings.styles.per_app = Object.fromEntries(
      perApp.filter((r) => r.bundle.trim()).map((r) => [r.bundle.trim(), r.style]),
    );
    save();
  }

  function addStyle() {
    settings.styles.styles.push({ id: "new-style", instruction: "" });
  }
</script>

<h2>Styles</h2>
<p class="page-desc">
  Tone instructions for the polish step: default, or per app (by bundle id).
</p>

<div class="card">
  <div class="row">
    <label for="default-style">Default style</label>
    <select id="default-style" bind:value={settings.styles.default} onchange={save}>
      <option value="">Neutral (no styling)</option>
      {#each settings.styles.styles as style}
        <option value={style.id}>{style.id}</option>
      {/each}
    </select>
  </div>
</div>

{#each settings.styles.styles as style, index}
  <div class="card">
    <div class="row" style="border: none">
      <input style="width: 160px" bind:value={style.id} onchange={save} />
      <button
        class="ghost"
        onclick={() => {
          settings.styles.styles.splice(index, 1);
          save();
        }}
      >
        Delete
      </button>
    </div>
    <textarea
      style="width: 100%; min-height: 56px; resize: vertical"
      placeholder="Instruction for the AI, e.g. «Formal tone, full sentences»"
      bind:value={style.instruction}
      onchange={save}
    ></textarea>
  </div>
{/each}
<button onclick={addStyle}>Add style</button>

<h2 style="margin-top: 28px">Per-app overrides</h2>
<p class="page-desc">
  Find an app's bundle id with: <code>osascript -e 'id of app "Slack"'</code>
</p>

{#each perApp as row, index}
  <div class="card">
    <div class="row" style="border: none">
      <input
        style="flex: 1"
        placeholder="com.tinyspeck.slackmacgap"
        bind:value={row.bundle}
        onchange={syncPerApp}
      />
      <select bind:value={row.style} onchange={syncPerApp}>
        {#each settings.styles.styles as style}
          <option value={style.id}>{style.id}</option>
        {/each}
      </select>
      <button
        class="ghost"
        onclick={() => {
          perApp.splice(index, 1);
          syncPerApp();
        }}
      >
        ×
      </button>
    </div>
  </div>
{/each}
<button
  onclick={() =>
    perApp.push({ bundle: "", style: settings.styles.styles[0]?.id ?? "" })}
>
  Add override
</button>
