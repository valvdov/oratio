<script lang="ts">
  import type { Settings } from "../lib/ipc";

  let { settings, save }: { settings: Settings; save: () => void } = $props();

  let newTerm = $state("");

  function add() {
    const term = newTerm.trim();
    if (!term || settings.dictionary.includes(term)) return;
    settings.dictionary.push(term);
    newTerm = "";
    save();
  }

  function remove(index: number) {
    settings.dictionary.splice(index, 1);
    save();
  }
</script>

<h2>Dictionary</h2>
<p class="page-desc">
  Terms, names and jargon spelled exactly as you want them. They prime both the
  recognizer and the polish step — «кубернетес» becomes «Kubernetes».
</p>

<div class="card">
  <div class="row" style="border: none">
    <input
      style="flex: 1"
      placeholder="Kubernetes, Oratio, Валерий…"
      bind:value={newTerm}
      onkeydown={(e) => e.key === "Enter" && add()}
    />
    <button class="primary" onclick={add}>Add</button>
  </div>
</div>

<div class="terms">
  {#each settings.dictionary as term, index}
    <span class="chip term">
      {term}
      <button class="x" onclick={() => remove(index)} aria-label="Remove {term}">×</button>
    </span>
  {/each}
  {#if settings.dictionary.length === 0}
    <p style="color: var(--faint); font-size: 13px">No terms yet.</p>
  {/if}
</div>

<style>
  .terms {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .term {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
  }
  .x {
    background: none;
    padding: 0 2px;
    color: var(--chip-text);
    font-size: 14px;
    line-height: 1;
  }
</style>
