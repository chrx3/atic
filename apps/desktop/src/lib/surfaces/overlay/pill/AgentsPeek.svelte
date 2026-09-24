<script lang="ts">
  /**
   * Cupos de los agentes, dentro de la isla.
   *
   * Es el mismo vistazo que pinta `PillPeekHost` flotando, en versión de un
   * solo cuerpo: allá el detalle es una gota aparte unida por un cuello; acá
   * la isla ya es el contenedor, así que el detalle va debajo de los anillos,
   * en el mismo flujo, y la isla crece para abrigarlo.
   */
  import AgentLogo from "$features/agents/AgentLogo.svelte";
  import { t } from "$domain/i18n.svelte";
  import { agentQuotas } from "$domain/agentQuotas.svelte";
  import { config } from "$domain/config.svelte";
  import { sessionEffect } from "$domain/session";
  import { isAgentShown } from "$features/agents/agentCatalog";
  import { quotaRows } from "./pillQuota";
  import {
    detailRowFor,
    headline,
    planText,
    spanText,
    spendText,
    windowText,
  } from "./quotaText";

  let { fallback, vertical = false }: { fallback: string; vertical?: boolean } =
    $props();

  let now = $state(Date.now());
  let hover = $state<string | null>(null);

  $effect(() => sessionEffect(["config"]));

  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 1_000);
    return () => clearInterval(timer);
  });

  const agentsShown = $derived(config.current?.agents_shown ?? []);
  const rows = $derived(
    quotaRows(agentQuotas.overview, now).filter((row) =>
      isAgentShown(row.agent, agentsShown),
    ),
  );
  const detailRow = $derived(detailRowFor(rows, hover));
</script>

{#if rows.length > 0}
  <div class="ap" class:is-vertical={vertical}>
    <div class="ap-rings" role="group" onpointerleave={() => (hover = null)}>
      {#each rows as row (row.agent)}
        {@const head = headline(row)}
        <div
          class="ap-agent"
          class:is-hovered={detailRow?.agent === row.agent}
          aria-hidden="true"
          onpointerenter={() => (hover = row.agent)}
        >
          <span class="ap-ring">
            <svg viewBox="0 0 40 40">
              <circle class="ap-ring-track" cx="20" cy="20" r="17" pathLength="100"
              ></circle>
              <circle
                class="ap-ring-fill is-{head.tone}"
                cx="20"
                cy="20"
                r="17"
                pathLength="100"
                style:stroke-dasharray="{Math.max(head.percent, 1)} 100"
              ></circle>
            </svg>
            <span class="ap-ring-logo"><AgentLogo agent={row.agent} size={16} /></span>
          </span>
          <span class="ap-pct" data-numeric>
            {row.bars.length > 0 ? `${Math.round(head.percent)}%` : "—"}
          </span>
        </div>
      {/each}
    </div>

    <!--
      El detalle de TODOS los agentes, apilados en la misma celda; se ve el
      apuntado. Así el alto es el del más largo y no cambia con el hover: si
      cambiara, la isla se reacomodaría y los anillos se correrían bajo el
      puntero. De paso el cambio es un fundido y no un salto.
    -->
    <div class="ap-details">
      {#each rows as row (row.agent)}
        <div class="ap-detail" class:is-active={detailRow?.agent === row.agent}>
          <div class="ap-detail-head">
            <span class="ap-detail-name">{row.name}</span>
            {#if row.staleAt != null}
              <span class="ap-detail-meta"
                >{t("pill.quota.stale", { when: spanText(now - row.staleAt) })}</span
              >
            {:else if row.plan}
              <span class="ap-detail-meta">{planText(row.plan)}</span>
            {/if}
          </div>

          {#if row.error}
            <p class="ap-error">{row.error}</p>
          {:else}
            {#each row.bars as bar (bar.window + (bar.model ?? "") + bar.minutes + (bar.resetsAt ?? ""))}
              <p class="ap-line is-{bar.tone}">
                <span class="ap-line-win">{windowText(bar)}</span>
                <span class="ap-line-val">
                  <strong data-numeric>{Math.round(bar.percent)}%</strong>
                  {#if bar.resetsAt != null && bar.resetsAt > now}
                    <span class="ap-line-reset"
                      >· {t("pill.quota.reset", {
                        when: spanText(bar.resetsAt - now),
                      })}</span
                    >
                  {/if}
                </span>
              </p>
            {/each}
            {#if row.spend}
              <p class="ap-note">{spendText(row, now)}</p>
            {/if}
          {/if}
        </div>
      {/each}
    </div>
  </div>
{:else if agentQuotas.loading}
  <p class="ap-fallback">{t("pill.quota.loading")}</p>
{:else}
  <p class="ap-fallback">{fallback}</p>
{/if}

<style>
  .ap {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .ap-rings {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: 0.45rem 0.6rem;
  }

  .ap-agent {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.2rem;
  }

  /* Al costado de la tira: un agente por fila, anillo y % en línea, como la
     tira misma. */
  .ap.is-vertical .ap-rings {
    flex-flow: column nowrap;
    align-items: stretch;
    gap: 0.35rem;
  }

  .ap.is-vertical .ap-agent {
    flex-direction: row;
    gap: 0.5rem;
  }

  .ap-ring {
    position: relative;
    display: grid;
    width: 2.15rem;
    height: 2.15rem;
    place-items: center;
  }

  .ap-ring svg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    transform: rotate(-90deg);
  }

  .ap-ring-track,
  .ap-ring-fill {
    fill: none;
    stroke-width: 3;
    transition:
      stroke var(--duration-fast, 125ms) var(--ease-smooth-out, ease-out),
      stroke-dasharray var(--duration-slow, 200ms) var(--ease-smooth-out, ease-out);
  }

  .ap-ring-track {
    stroke: color-mix(in sRGB, var(--text) 13%, transparent);
  }

  .ap-agent.is-hovered .ap-ring-track {
    stroke: color-mix(in sRGB, var(--text) 26%, transparent);
  }

  .ap-ring-fill {
    stroke: var(--accent);
    stroke-linecap: round;
  }

  .ap-ring-fill.is-warn {
    stroke: var(--warn);
  }

  .ap-ring-fill.is-hot {
    stroke: var(--danger);
  }

  .ap-ring-logo {
    display: grid;
    place-items: center;
    color: var(--text);
  }

  .ap-pct {
    color: var(--muted);
    font-size: 0.6875rem;
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }

  .ap-agent.is-hovered .ap-pct {
    color: var(--text);
  }

  .ap-details {
    display: grid;
    padding-top: 0.5rem;
    border-top: 1px solid var(--line);
  }

  .ap-detail {
    display: flex;
    grid-area: 1 / 1;
    flex-direction: column;
    gap: 0.25rem;
    opacity: 0;
    visibility: hidden;
    transition:
      opacity var(--duration-fast, 125ms) var(--ease-smooth-out, ease-out),
      visibility 0s linear var(--duration-fast, 125ms);
  }

  .ap-detail.is-active {
    opacity: 1;
    visibility: visible;
    transition-delay: 0s;
  }

  .ap-detail-head {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
  }

  .ap-detail-name {
    color: var(--text);
    font-weight: 600;
  }

  .ap-detail-meta {
    margin-left: auto;
    color: var(--faint);
    font-size: 0.6875rem;
  }

  .ap-line {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
    margin: 0;
    font-size: 0.6875rem;
    line-height: 1.4;
  }

  .ap-line-win {
    overflow: hidden;
    color: var(--muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ap-line-val {
    color: var(--muted);
    white-space: nowrap;
  }

  .ap-line-val strong {
    color: var(--text);
    font-variant-numeric: tabular-nums;
    font-weight: 650;
  }

  .ap-line.is-warn .ap-line-val strong {
    color: var(--warn);
  }

  .ap-line.is-hot .ap-line-val strong {
    color: var(--danger);
  }

  .ap-line-reset {
    color: var(--faint);
  }

  .ap-error,
  .ap-note,
  .ap-fallback {
    margin: 0;
    color: var(--muted);
    font-size: 0.6875rem;
    line-height: 1.4;
  }

  .ap-error {
    color: var(--warn);
    text-wrap: pretty;
  }

  @media (prefers-reduced-motion: reduce) {
    .ap-ring-track,
    .ap-ring-fill,
    .ap-detail {
      transition: none;
    }
  }
</style>
