<script lang="ts">
  /**
   * Vistazo de Sistema: CPU, memoria y lo que más consume, en vivo.
   *
   * Refresca al ritmo de la pestaña de recursos, y solo mientras está a la
   * vista: enumerar procesos cada segundo con el vistazo cerrado sería el
   * gasto que el panel completo ya evita. Las barras se tiñen con los mismos
   * umbrales de los avisos: rojo acá es lo mismo que un aviso allá.
   */
  import { system } from "$domain/system.svelte";
  import { t } from "$domain/i18n.svelte";
  import { formatBytes, formatPercent } from "$features/system/systemFormat";

  let { onopen }: { onopen: () => void } = $props();

  /** Mismo ritmo que la pestaña de recursos del panel. */
  const REFRESH_MS = 1500;
  const TOP = 3;

  $effect(() => {
    void system.hydrate("resources");
    const timer = window.setInterval(
      () => void system.hydrate("resources"),
      REFRESH_MS,
    );
    return () => window.clearInterval(timer);
  });

  const snap = $derived(system.snapshot);
  const cpu = $derived(snap?.cpu ?? 0);
  const ram = $derived(
    snap && snap.ram_total > 0 ? (snap.ram_used / snap.ram_total) * 100 : 0,
  );
  const top = $derived(
    [...(snap?.apps ?? [])].sort((a, b) => b.cpu - a.cpu).slice(0, TOP),
  );

  function tone(value: number, limit: number): "ok" | "warn" {
    return value >= limit ? "warn" : "ok";
  }
</script>

<div class="sp">
  <div class="sp-meter is-{tone(cpu, system.alerts.cpu)}">
    <span class="sp-label">{t("pill.peek.cpu")}</span>
    <span class="sp-track"
      ><span class="sp-fill" style:width="{Math.max(cpu, 2)}%"></span></span
    >
    <span class="sp-val" data-numeric>{formatPercent(cpu)}</span>
  </div>
  <div class="sp-meter is-{tone(ram, system.alerts.ram)}">
    <span class="sp-label">{t("pill.peek.ram")}</span>
    <span class="sp-track"
      ><span class="sp-fill" style:width="{Math.max(ram, 2)}%"></span></span
    >
    <span class="sp-val" data-numeric>
      {snap ? formatBytes(snap.ram_used) : "—"}
    </span>
  </div>

  {#if top.length > 0}
    <p class="sp-head">{t("pill.peek.top")}</p>
    <ul class="sp-apps">
      {#each top as app (app.id)}
        <li class="sp-app">
          {#if app.icon}
            <img class="sp-icon" src={app.icon} alt="" />
          {:else}
            <span class="sp-icon is-empty" aria-hidden="true"></span>
          {/if}
          <span class="sp-name">{app.name}</span>
          <span class="sp-app-val" data-numeric>{formatPercent(app.cpu)}</span>
        </li>
      {/each}
    </ul>
  {/if}

  <button type="button" class="sp-open" onclick={onopen}
    >{t("pill.peek.openSystem")}</button
  >
</div>

<style>
  .sp {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .sp-meter {
    display: grid;
    grid-template-columns: 4.2rem 1fr 3.4rem;
    align-items: center;
    gap: 0.5rem;
  }

  .sp-label {
    color: var(--muted);
  }

  .sp-track {
    position: relative;
    height: 0.3rem;
    overflow: hidden;
    border-radius: 999px;
    background: color-mix(in sRGB, var(--text) 12%, transparent);
  }

  .sp-fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: inherit;
    background: var(--ok);
    transition:
      width var(--duration-slow, 200ms) var(--ease-smooth-out, ease-out),
      background var(--duration-fast, 125ms) var(--ease-smooth-out, ease-out);
  }

  .is-warn .sp-fill {
    background: var(--warn);
  }

  .sp-val,
  .sp-app-val {
    color: var(--text);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .sp-head {
    margin: 0.2rem 0 0;
    color: var(--faint);
    font-size: 0.6875rem;
  }

  .sp-apps {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .sp-app {
    display: grid;
    grid-template-columns: 1rem 1fr auto;
    align-items: center;
    gap: 0.5rem;
  }

  .sp-icon {
    width: 1rem;
    height: 1rem;
    border-radius: 4px;
  }

  .sp-icon.is-empty {
    background: color-mix(in sRGB, var(--text) 10%, transparent);
  }

  .sp-name {
    min-width: 0;
    overflow: hidden;
    color: var(--muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sp-open {
    align-self: flex-start;
    margin-top: 0.15rem;
    border: 0;
    padding: 0;
    background: none;
    color: var(--faint);
    font-size: 0.6875rem;
    cursor: pointer;
  }

  .sp-open:hover {
    color: var(--text);
  }

  .sp-open:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }
</style>
