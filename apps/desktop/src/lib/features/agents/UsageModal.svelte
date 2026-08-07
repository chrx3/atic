<script lang="ts">
  /**
   * Uso de la cuenta Claude (cupos) + costo/contexto de la sesión.
   * Al abrir consulta la API OAuth (misma fuente que `/usage`) y hace poll
   * mientras el modal esté abierto.
   */
  import { onMount } from "svelte";
  import type { AgentItem, AgentTurn, ClaudeAccountUsage } from "$lib/types";
  import { agentClaudeUsage } from "$ipc/agents";
  import Modal from "$ui/Modal.svelte";
  import ProgressBar from "$ui/ProgressBar.svelte";

  let {
    costUsd = 0,
    contextTokens = 0,
    contextSize = null,
    model = "",
    effort = null,
    mode = null,
    turns = [],
    archive = false,
    onClose,
  }: {
    costUsd?: number;
    contextTokens?: number;
    contextSize?: number | null;
    model?: string;
    effort?: string | null;
    mode?: string | null;
    turns?: AgentTurn[];
    archive?: boolean;
    onClose: () => void;
  } = $props();

  const ACCENT = "#da7756";
  const POLL_MS = 12_000;

  let account = $state<ClaudeAccountUsage | null>(null);
  let accountError = $state<string | null>(null);
  let accountLoading = $state(true);
  let refreshing = $state(false);

  async function loadAccount(opts?: { silent?: boolean }) {
    const silent = opts?.silent ?? false;
    if (!silent) accountLoading = account == null;
    else refreshing = true;
    try {
      const next = await agentClaudeUsage();
      account = next;
      accountError = null;
    } catch (e) {
      accountError =
        typeof e === "string"
          ? e
          : e instanceof Error
            ? e.message
            : String(e);
    } finally {
      accountLoading = false;
      refreshing = false;
    }
  }

  onMount(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    void (async () => {
      await loadAccount();
      if (cancelled) return;
      timer = setInterval(() => {
        if (!cancelled) void loadAccount({ silent: true });
      }, POLL_MS);
    })();
    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
    };
  });

  function formatTokens(n: number): string {
    if (n >= 1_000_000) {
      const m = n / 1_000_000;
      return `${m >= 10 ? Math.round(m) : m.toFixed(1).replace(/\.0$/, "")}M`;
    }
    if (n >= 1000) {
      const k = n / 1000;
      return `${k >= 10 ? Math.round(k) : k.toFixed(1).replace(/\.0$/, "")}k`;
    }
    return String(n);
  }

  function formatCost(n: number): string {
    if (n <= 0) return "$0.00";
    if (n < 0.01) return `$${n.toFixed(4)}`;
    return `$${n.toFixed(2)}`;
  }

  function formatReset(iso: string | null | undefined): string | null {
    if (!iso) return null;
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return null;
    const now = Date.now();
    const diff = d.getTime() - now;
    if (diff <= 0) return "se reinicia pronto";
    const mins = Math.round(diff / 60_000);
    if (mins < 60) return `se reinicia en ${mins} min`;
    const hours = Math.round(mins / 60);
    if (hours < 36) return `se reinicia en ${hours} h`;
    try {
      return `se reinicia ${d.toLocaleString(undefined, {
        weekday: "short",
        hour: "numeric",
        minute: "2-digit",
      })}`;
    } catch {
      return `se reinicia ${d.toISOString()}`;
    }
  }

  function barTone(pct: number): "accent" | "ok" | "warn" {
    if (pct > 85) return "warn";
    if (pct > 60) return "accent";
    return "ok";
  }

  const hasCost = $derived(costUsd > 0);
  const hasContext = $derived(contextTokens > 0);
  const contextPct = $derived.by(() => {
    if (!contextSize || contextSize <= 0 || contextTokens <= 0) return null;
    return Math.min(contextTokens / contextSize, 1);
  });
  const contextLabel = $derived.by(() => {
    if (!hasContext) return null;
    const used = formatTokens(contextTokens);
    if (contextSize && contextSize > 0) {
      return `${used} / ${formatTokens(contextSize)}`;
    }
    return used;
  });

  const finishedTurns = $derived(
    turns.filter((t) => t.status !== "running" && t.items.length > 0),
  );
  const turnRows = $derived(
    [...finishedTurns].reverse().slice(0, 12).map((t, i) => {
      const n = finishedTurns.length - i;
      const userMsg = t.items.find(
        (it): it is Extract<AgentItem, { kind: "message" }> =>
          it.kind === "message" && it.role === "user",
      );
      const anyMsg = t.items.find(
        (it): it is Extract<AgentItem, { kind: "message" }> =>
          it.kind === "message",
      );
      const raw = (userMsg?.text ?? anyMsg?.text ?? `Turno ${n}`).trim();
      return {
        id: t.id,
        index: n,
        cost: t.costUsd,
        preview: raw.length > 40 ? `${raw.slice(0, 40)}…` : raw,
      };
    }),
  );

  const turnsWithCost = $derived(
    finishedTurns.filter((t) => t.costUsd != null && t.costUsd > 0).length,
  );

  type QuotaRow = {
    key: string;
    label: string;
    utilization: number;
    reset: string | null;
  };

  const quotaRows = $derived.by((): QuotaRow[] => {
    if (!account) return [];
    const rows: QuotaRow[] = [];
    const push = (
      key: string,
      label: string,
      w: { utilization: number; resetsAt: string | null } | null,
    ) => {
      if (!w) return;
      rows.push({
        key,
        label,
        utilization: w.utilization,
        reset: formatReset(w.resetsAt),
      });
    };
    push("5h", "Sesión (5 h)", account.fiveHour);
    push("7d", "Semana (todos)", account.sevenDay);
    push("opus", "Semana · Opus", account.sevenDayOpus);
    push("sonnet", "Semana · Sonnet", account.sevenDaySonnet);
    const extra = account.extraUsage;
    if (extra?.isEnabled) {
      const util = extra.utilization ?? 0;
      let reset: string | null = null;
      if (extra.usedCredits != null && extra.monthlyLimit != null) {
        const cur = extra.currency ? ` ${extra.currency}` : "";
        reset = `${extra.usedCredits.toFixed(2)} / ${extra.monthlyLimit.toFixed(2)}${cur}`;
      }
      rows.push({
        key: "extra",
        label: "Uso extra",
        utilization: util,
        reset,
      });
    }
    return rows;
  });

  const subtitle = $derived.by(() => {
    if (archive) return "Archivo · solo lectura";
    if (account?.plan) return `Cuenta · ${account.plan}`;
    return "Cuenta Claude";
  });

  const updatedHint = $derived.by(() => {
    if (!account) return null;
    if (refreshing) return "actualizando…";
    const ago = Math.max(0, Math.round((Date.now() - account.fetchedAt) / 1000));
    if (ago < 5) return "ahora";
    if (ago < 60) return `hace ${ago} s`;
    return `hace ${Math.round(ago / 60)} min`;
  });
</script>

<div class="usage-root" style="--accent: {ACCENT}">
  <Modal title="Uso" subtitle={subtitle} size="sm" contained onClose={onClose}>
    <div class="stack">
      <section class="account" aria-label="Uso de la cuenta">
        <div class="sec-head">
          <h3 class="sec-h">Cupos de la cuenta</h3>
          {#if updatedHint}
            <span class="live" aria-live="polite">
              <span class="live-dot" class:is-pulse={refreshing} aria-hidden="true"></span>
              {updatedHint}
            </span>
          {/if}
        </div>

        {#if accountLoading}
          <div class="state">
            <p class="state-t">Consultando uso…</p>
            <p class="state-d">Misma fuente que <code>/usage</code> en Claude Code.</p>
          </div>
        {:else if accountError && !account}
          <div class="state is-err">
            <p class="state-t">No se pudo leer el uso</p>
            <p class="state-d">{accountError}</p>
            <button type="button" class="retry" onclick={() => loadAccount()}>
              Reintentar
            </button>
          </div>
        {:else if quotaRows.length === 0}
          <div class="state">
            <p class="state-t">Sin cupos de suscripción</p>
            <p class="state-d">
              Este dato solo aparece con plan Pro/Max (OAuth). Con API key no
              hay ventana de 5 h ni semanal.
            </p>
          </div>
        {:else}
          {#if accountError}
            <p class="soft-err" role="status">{accountError}</p>
          {/if}
          <ul class="quotas">
            {#each quotaRows as row (row.key)}
              <li class="quota">
                <ProgressBar
                  value={row.utilization / 100}
                  label={row.label}
                  tone={barTone(row.utilization)}
                />
                {#if row.reset}
                  <p class="quota-reset">{row.reset}</p>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      <section class="session" aria-label="Uso de esta sesión">
        <h3 class="sec-h">Esta sesión</h3>
        <div class="hero" role="group" aria-label="Resumen de sesión">
          <div class="stat">
            <span class="stat-l">Costo</span>
            <span class="stat-v" class:is-muted={!hasCost} data-numeric>
              {hasCost ? formatCost(costUsd) : "—"}
            </span>
            <span class="stat-h">
              {hasCost
                ? "Acumulado en esta sesión"
                : archive
                  ? "Sin costo en el archivo"
                  : "Aún no hay costo en este chat"}
            </span>
          </div>
          <div class="stat">
            <span class="stat-l">Contexto</span>
            <span class="stat-v" class:is-muted={!hasContext} data-numeric>
              {contextLabel ?? "—"}
            </span>
            <span class="stat-h">
              {#if contextPct != null}
                {Math.round(contextPct * 100)}% de la ventana
              {:else if hasContext}
                Tokens usados (ventana desconocida)
              {:else if archive}
                No disponible en archivo
              {:else}
                Sin lectura de contexto aún
              {/if}
            </span>
          </div>
        </div>

        {#if contextPct != null}
          <ProgressBar
            value={contextPct}
            label="Ventana de contexto"
            tone={contextPct > 0.85 ? "warn" : "accent"}
          />
        {/if}

        <dl class="meta">
          {#if model}
            <div class="meta-row">
              <dt>Modelo</dt>
              <dd>{model}</dd>
            </div>
          {/if}
          {#if effort}
            <div class="meta-row">
              <dt>Effort</dt>
              <dd>{effort}</dd>
            </div>
          {/if}
          {#if mode}
            <div class="meta-row">
              <dt>Modo</dt>
              <dd>{mode}</dd>
            </div>
          {/if}
          <div class="meta-row">
            <dt>Turnos</dt>
            <dd data-numeric>
              {finishedTurns.length}
              {#if turnsWithCost > 0}
                <span class="meta-note">· {turnsWithCost} con costo</span>
              {/if}
            </dd>
          </div>
        </dl>

        {#if turnRows.length > 0}
          <div class="turns" aria-label="Desglose por turno">
            <h4 class="turns-h">Últimos turnos</h4>
            <ul class="turns-list">
              {#each turnRows as row (row.id)}
                <li class="turn">
                  <span class="turn-n" data-numeric>#{row.index}</span>
                  <span class="turn-p" title={row.preview}>{row.preview}</span>
                  <span
                    class="turn-c"
                    class:is-bare={row.cost == null}
                    data-numeric
                  >
                    {row.cost != null ? formatCost(row.cost) : "—"}
                  </span>
                </li>
              {/each}
            </ul>
          </div>
        {/if}
      </section>
    </div>
  </Modal>
</div>

<style>
  .usage-root {
    display: contents;
  }

  .stack {
    display: flex;
    flex-direction: column;
    gap: 1.1rem;
  }

  .sec-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .sec-h {
    margin: 0 0 0.45rem;
    font-size: 0.65rem;
    font-weight: 550;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--rb-faint);
  }

  .sec-head .sec-h {
    margin-bottom: 0;
  }

  .live {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.65rem;
    color: var(--rb-faint);
  }

  .live-dot {
    width: 0.4rem;
    height: 0.4rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent, #da7756) 70%, transparent);
  }

  .live-dot.is-pulse {
    animation: pulse 1s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  .state {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    padding: 0.55rem 0 0.15rem;
  }

  .state-t {
    margin: 0;
    font-size: 0.85rem;
    font-weight: 550;
    color: var(--rb-text);
  }

  .state-d {
    margin: 0;
    font-size: 0.72rem;
    line-height: 1.45;
    color: var(--rb-faint);
    text-wrap: pretty;
  }

  .state-d code {
    font-family: var(--rb-mono, ui-monospace, monospace);
    font-size: 0.68rem;
    color: var(--rb-muted);
  }

  .state.is-err .state-t {
    color: var(--rb-text);
  }

  .retry {
    align-self: flex-start;
    margin-top: 0.35rem;
    padding: 0.28rem 0.55rem;
    border: 1px solid var(--rb-border);
    border-radius: 0.4rem;
    background: transparent;
    font-size: 0.72rem;
    color: var(--rb-muted);
    cursor: pointer;
  }

  .retry:hover {
    color: var(--rb-text);
    border-color: var(--rb-muted);
  }

  .soft-err {
    margin: 0 0 0.4rem;
    font-size: 0.7rem;
    color: var(--rb-faint);
  }

  .quotas {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    margin: 0.35rem 0 0;
    padding: 0;
    list-style: none;
  }

  .quota-reset {
    margin: 0.2rem 0 0;
    font-size: 0.65rem;
    color: var(--rb-faint);
  }

  .session {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    padding-top: 0.15rem;
    border-top: 1px solid var(--rb-hairline, var(--rb-border));
  }

  .hero {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.55rem;
  }

  .stat {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
    padding: 0.65rem 0.7rem;
    border: 1px solid var(--rb-border);
    border-radius: 0.55rem;
    background: color-mix(in srgb, var(--rb-text) 3%, transparent);
  }

  .stat-l {
    font-size: 0.65rem;
    font-weight: 550;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--rb-faint);
  }

  .stat-v {
    font-family: var(--rb-mono, ui-monospace, monospace);
    font-size: 1.15rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
    color: var(--accent, #da7756);
    line-height: 1.2;
  }

  .stat-v.is-muted {
    color: var(--rb-faint);
  }

  .stat-h {
    font-size: 0.65rem;
    line-height: 1.35;
    color: var(--rb-faint);
  }

  .meta {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    margin: 0;
  }

  .meta-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
    min-width: 0;
  }

  .meta-row dt {
    flex-shrink: 0;
    font-size: 0.75rem;
    color: var(--rb-faint);
  }

  .meta-row dd {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.75rem;
    font-weight: 500;
    color: var(--rb-text);
    text-align: right;
  }

  .meta-note {
    font-weight: 400;
    color: var(--rb-faint);
  }

  .turns {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .turns-h {
    margin: 0;
    font-size: 0.65rem;
    font-weight: 550;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--rb-faint);
  }

  .turns-list {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--rb-border);
    border-radius: 0.55rem;
    overflow: hidden;
  }

  .turn {
    display: grid;
    grid-template-columns: 1.6rem minmax(0, 1fr) auto;
    align-items: center;
    gap: 0.45rem;
    padding: 0.4rem 0.55rem;
    border-top: 1px solid var(--rb-hairline, var(--rb-border));
  }

  .turn:first-child {
    border-top: 0;
  }

  .turn-n {
    font-family: var(--rb-mono, ui-monospace, monospace);
    font-size: 0.65rem;
    font-variant-numeric: tabular-nums;
    color: var(--rb-faint);
  }

  .turn-p {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.72rem;
    color: var(--rb-muted);
  }

  .turn-c {
    font-family: var(--rb-mono, ui-monospace, monospace);
    font-size: 0.7rem;
    font-variant-numeric: tabular-nums;
    font-weight: 550;
    color: var(--rb-text);
  }

  .turn-c.is-bare {
    color: var(--rb-faint);
    font-weight: 400;
  }
</style>
