<script lang="ts">
  /**
   * Panel flotante para afinar el picker en vivo. Solo montar en DEV.
   * Atajo: Ctrl+Alt+P. El lab general de líquido sigue en Ctrl+Alt+L.
   */
  import { sminReach } from "$liquid/sdf";
  import { commitPickerLab, pickerLab, type PickerLabValues } from "./pickerLab.svelte";

  const reach = $derived(Math.round(sminReach(pickerLab.blend) * 10) / 10);
  let copied = $state(false);

  type Row = {
    label: string;
    key: keyof PickerLabValues;
    min: number;
    max: number;
    step: number;
  };

  const rows: Row[] = [
    { label: "blend (fusión)", key: "blend", min: 0, max: 120, step: 1 },
    { label: "cell (muestreo)", key: "cell", min: 2, max: 16, step: 1 },
    {
      label: "cardFloat (hueco rueda→card)",
      key: "cardFloat",
      min: 0,
      max: 200,
      step: 1,
    },
    {
      label: "pitchPad (aire entre cards)",
      key: "pitchPad",
      min: 0,
      max: 120,
      step: 1,
    },
    { label: "heightFill", key: "heightFill", min: 0.5, max: 1, step: 0.01 },
    { label: "hotX", key: "hotX", min: 40, max: 160, step: 1 },
    { label: "hotX expanded", key: "hotXExpanded", min: 40, max: 180, step: 1 },
    { label: "step °", key: "stepDeg", min: 8, max: 45, step: 1 },
    { label: "step ° expanded", key: "stepDegExpanded", min: 8, max: 40, step: 1 },
    { label: "card hot W", key: "cardHotW", min: 180, max: 420, step: 2 },
    { label: "card hot H", key: "cardHotH", min: 80, max: 220, step: 2 },
    { label: "card cold W", key: "cardColdW", min: 140, max: 360, step: 2 },
    { label: "card cold H", key: "cardColdH", min: 40, max: 120, step: 2 },
  ];

  function valueOf(key: keyof PickerLabValues): number {
    return pickerLab.snapshot()[key];
  }

  function setValue(key: keyof PickerLabValues, value: number) {
    pickerLab.apply({ [key]: value });
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(pickerLab.asCode());
      copied = true;
      window.setTimeout(() => (copied = false), 1200);
    } catch {
      /* ignore */
    }
  }
</script>

<aside class="lab" aria-label="Picker lab">
  <header class="head">
    <div>
      <strong>Picker lab</strong>
      <p class="meta">REACH ≈ {reach}px · Esc o × cierra</p>
    </div>
    <button
      type="button"
      class="icon"
      aria-label="Cerrar"
      onclick={() => pickerLab.close()}
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
          onchange={() => commitPickerLab()}
        />
      </label>
    {/each}
  </div>

  <footer class="foot">
    <button type="button" onclick={() => pickerLab.reset()}>Reset</button>
    <button type="button" onclick={() => void copy()}>
      {copied ? "Copiado" : "Copiar valores"}
    </button>
  </footer>
</aside>

<style>
  .lab {
    position: fixed;
    top: 3.25rem;
    right: 0.75rem;
    z-index: 80;
    display: flex;
    width: min(20rem, calc(100vw - 1.5rem));
    max-height: min(70vh, 36rem);
    flex-direction: column;
    overflow: hidden;
    border: 1px solid var(--rb-line, #333);
    border-radius: 0.75rem;
    background: color-mix(in oklab, var(--rb-elevated, #1a1a1a) 92%, black);
    box-shadow: 0 12px 40px rgb(0 0 0 / 45%);
    color: var(--rb-text, #eee);
    font-size: 0.75rem;
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
