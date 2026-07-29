<script lang="ts">
  import { onMount } from "svelte";
  import { permissionsStatus, type Settings } from "../lib/ipc";

  let { settings, save }: { settings: Settings; save: () => void } = $props();
  void settings;
  void save;

  let accessibility = $state<boolean | null>(null);

  async function refresh() {
    const status = await permissionsStatus();
    accessibility = status.accessibility;
  }

  onMount(() => {
    refresh();
    const timer = setInterval(refresh, 3000);
    return () => clearInterval(timer);
  });
</script>

<h2>Permissions</h2>
<p class="page-desc">macOS permissions Oratio needs to work.</p>

<div class="card">
  <div class="row">
    <div>
      <div>Accessibility</div>
      <div style="font-size: 12px; color: var(--faint)">
        Required to paste text into other apps (synthesized Cmd+V)
      </div>
    </div>
    {#if accessibility === null}
      <span class="chip">checking…</span>
    {:else if accessibility}
      <span class="chip">granted</span>
    {:else}
      <span class="chip" style="background: #f8d7da; color: #842029">missing</span>
    {/if}
  </div>
  <div class="row">
    <div>
      <div>Microphone</div>
      <div style="font-size: 12px; color: var(--faint)">
        Requested automatically on first dictation
      </div>
    </div>
    <span class="chip">system prompt</span>
  </div>
</div>

<p style="font-size: 13px; color: var(--muted); line-height: 1.6">
  In dev mode both permissions belong to the terminal you launch Oratio from:
  System Settings → Privacy &amp; Security → Accessibility / Microphone.
</p>
