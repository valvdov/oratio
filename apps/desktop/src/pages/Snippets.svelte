<script lang="ts">
  import type { Settings } from "../lib/ipc";

  let { settings, save }: { settings: Settings; save: () => void } = $props();

  function add() {
    settings.snippets.push({ trigger: "", expansion: "" });
  }

  function remove(index: number) {
    settings.snippets.splice(index, 1);
    save();
  }
</script>

<h2>Snippets</h2>
<p class="page-desc">
  Say the trigger phrase — the expansion is inserted verbatim, no AI involved.
</p>

{#each settings.snippets as snippet, index}
  <div class="card">
    <div class="row" style="border: none">
      <input
        style="flex: 1"
        placeholder="Trigger, e.g. «моя подпись»"
        bind:value={snippet.trigger}
        onchange={save}
      />
      <button class="ghost" onclick={() => remove(index)}>Delete</button>
    </div>
    <textarea
      style="width: 100%; min-height: 70px; resize: vertical"
      placeholder="Expansion text…"
      bind:value={snippet.expansion}
      onchange={save}
    ></textarea>
  </div>
{/each}

<button class="primary" onclick={add}>Add snippet</button>
