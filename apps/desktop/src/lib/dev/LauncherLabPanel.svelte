<script lang="ts">
  /**
   * Panel flotante para afinar launcher + liquid del overlay. Solo montar en DEV.
   * Atajo: Ctrl+Alt+F. Esc o × cierra.
   */
  import {
    commitLauncherLab,
    launcherLab,
    type LauncherLabValues,
  } from "./launcherLab.svelte";

  let copied = $state(false);

  type Row = {
    label: string;
    key: keyof LauncherLabValues;
    min: number;
    max: number;
    step: number;
  };

  const rows: Row[] = [
    { label: "blend (→ REACH)", key: "blend", min: 0, max: 90, step: 1 },
    { label: "cell (muestreo)", key: "cell", min: 3, max: 12, step: 1 },
    { label: "favGap barra→favs", key: "favGap", min: 0, max: 80, step: 1 },
    { label: "dotGap entre favs", key: "dotGap", min: 0, max: 80, step: 1 },
    { label: "openDur (ms)", key: "openDur", min: 40, max: 700, step: 5 },
    { label: "closeDur (ms)", key: "closeDur", min: 40, max: 300, step: 5 },
    { label: "barW (px)", key: "barW", min: 240, max: 560, step: 4 },
    { label: "barH (px)", key: "barH", min: 36, max: 72, step: 1 },
    { label: "gooGrow (px)", key: "gooGrow", min: 0, max: 8, step: 0.1 },
  ];

  function valueOf(key: keyof LauncherLabValues): number {
    return launcherLab.snapshot()[key];
  }

  function setValue(key: keyof LauncherLabValues, value: number) {
    launcherLab.apply({ [key]: value });
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(launcherLab.asJson());
      copied = true;
      window.setTimeout(() => (copied = false), 1200);
    } catch {
      /* ignore */
    }
  }
</script>

<aside class="lab" aria-label="Launcher liquid lab">
  <header class="head">
    <div>
      <strong>Launcher lab</strong>
      <p class="meta">
        REACH ≈ {launcherLab.reach}px · Ctrl+Alt+F · Esc cierra
        {#if launcherLab.favGap <= launcherLab.reach || launcherLab.dotGap <= launcherLab.reach}
          · gap ≤ REACH → se fusionan
        {/if}
      </p>
    </div>
    <button
      type="button"
      class="icon"
      aria-label="Cerrar"
      onclick={() => launcherLab.close()}
    >
      ×
    </button>
  </header>

  <div class="rows">
    {#each rows as row (row.key)}
      <label class="row">
        <span class="label">
          {row.label}
          <em>{valueOf(row.key)}</em>
        </span>
        <input
          type="range"
          min={row.min}
          max={row.max}
          step={row.step}
          value={valueOf(row.key)}
          oninput={(e) => setValue(row.key, Number(e.currentTarget.value))}
          onchange={() => commitLauncherLab()}
        />
      </label>
    {/each}
  </div>

  <footer class="foot">
    <button type="button" onclick={() => launcherLab.reset()}>Reset</button>
    <button type="button" onclick={() => void copy()}>
      {copied ? "Copiado" : "Copiar config"}
    </button>
  </footer>
</aside>

<style>
  .lab {
    position: relative;
    display: flex;
    width: 100%;
    max-height: min(70vh, 34rem);
    flex-direction: column;
    overflow: hidden;
    border: 1px solid var(--rb-line, #333);
    border-radius: 0.75rem;
    background: color-mix(in oklab, var(--rb-elevated, #1a1a1a) 92%, black);
    box-shadow: 0 12px 40px rgb(0 0 0 / 45%);
    color: var(--rb-text, #eee);
    font-size: 0.75rem;
    pointer-events: auto;
  }

  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.65rem 0.75rem;
    border-bottom: 1px solid var(--rb-line, #333);
  }

  .head strong {
    font-size: 0.8rem;
  }

  .meta {
    margin: 0.15rem 0 0;
    color: var(--rb-muted, #999);
    font-size: 0.68rem;
  }

  .icon {
    display: grid;
    width: 1.5rem;
    height: 1.5rem;
    place-items: center;
    border: 0;
    border-radius: 0.35rem;
    background: transparent;
    color: inherit;
    cursor: pointer;
    font-size: 1.1rem;
    line-height: 1;
  }

  .icon:hover {
    background: color-mix(in oklab, var(--rb-text, #fff) 10%, transparent);
  }

  .rows {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.55rem;
    overflow-y: auto;
    padding: 0.65rem 0.75rem;
  }

  .row {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  .label {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
    color: var(--rb-muted, #999);
  }

  .label em {
    font-style: normal;
    font-variant-numeric: tabular-nums;
    color: var(--rb-text, #eee);
  }

  input[type="range"] {
    width: 100%;
    accent-color: var(--rb-text, #ddd);
  }

  .foot {
    display: flex;
    gap: 0.4rem;
    padding: 0.55rem 0.75rem;
    border-top: 1px solid var(--rb-line, #333);
  }

  .foot button {
    flex: 1;
    height: 1.75rem;
    border: 1px solid var(--rb-line, #333);
    border-radius: 0.4rem;
    background: color-mix(in oklab, var(--rb-text, #fff) 6%, transparent);
    color: inherit;
    cursor: pointer;
    font-size: 0.7rem;
  }

  .foot button:hover {
    background: color-mix(in oklab, var(--rb-text, #fff) 12%, transparent);
  }
</style>
