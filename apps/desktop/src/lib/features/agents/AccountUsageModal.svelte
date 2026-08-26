<script lang="ts">
  import { onMount } from "svelte";
  import type {
    ClaudeAccountUsage,
    CodexAccountUsage,
    CodexUsageWindow,
  } from "$lib/types";
  import { agentClaudeUsage, agentCodexUsage } from "$ipc/agents";
  import Modal from "$ui/Modal.svelte";
  import ProgressBar from "$ui/ProgressBar.svelte";
  import AgentLogo from "./AgentLogo.svelte";

  let {
    agent,
    onClose,
    onRunUsageCommand,
  }: {
    agent: string;
    onClose: () => void;
    /** Escribe `/usage` en la consola activa del agente (si hay sesión viva). */
    onRunUsageCommand?: () => void;
  } = $props();

  type UsageRow = {
    key: string;
    label: string;
    used: number;
    reset: string | null;
  };

  const POLL_MS = 15_000;
  const provider = $derived(
    agent === "claude"
      ? "Claude Code"
      : agent === "codex"
        ? "Codex"
        : agent === "opencode"
          ? "OpenCode"
          : agent === "cursor-agent"
            ? "Cursor"
            : "Agente",
  );
  const hasLiveQuota = $derived(agent === "claude" || agent === "codex");

  let claude = $state<ClaudeAccountUsage | null>(null);
  let codex = $state<CodexAccountUsage | null>(null);
  let loading = $state(false);
  let refreshing = $state(false);
  let error = $state<string | null>(null);

  function resetLabel(value: string | number | null | undefined): string | null {
    if (value == null) return null;
    const date = new Date(typeof value === "number" ? value * 1000 : value);
    if (Number.isNaN(date.getTime())) return null;
    const minutes = Math.max(0, Math.round((date.getTime() - Date.now()) / 60_000));
    if (minutes < 60) return `Reinicia en ${minutes} min`;
    const hours = Math.round(minutes / 60);
    if (hours < 36) return `Reinicia en ${hours} h`;
    const days = Math.round(hours / 24);
    return `Reinicia en ${days} d`;
  }

  function durationLabel(minutes: number): string {
    if (minutes % 10_080 === 0) {
      const weeks = minutes / 10_080;
      return weeks === 1 ? "Semana" : `${weeks} semanas`;
    }
    if (minutes % 1_440 === 0) return `${minutes / 1_440} días`;
    if (minutes % 60 === 0) return `${minutes / 60} h`;
    return `${minutes} min`;
  }

  function codexRow(key: string, window: CodexUsageWindow | null): UsageRow | null {
    if (!window) return null;
    return {
      key,
      label: durationLabel(window.windowDurationMins),
      used: window.usedPercent,
      reset: resetLabel(window.resetsAt),
    };
  }

  const rows = $derived.by((): UsageRow[] => {
    if (claude) {
      const out: UsageRow[] = [];
      const add = (
        key: string,
        label: string,
        window: { utilization: number; resetsAt: string | null } | null,
      ) => {
        if (window) {
          out.push({
            key,
            label,
            used: window.utilization,
            reset: resetLabel(window.resetsAt),
          });
        }
      };
      add("5h", "5 horas", claude.fiveHour);
      add("7d", "Semana", claude.sevenDay);
      add("opus", "Semana · Opus", claude.sevenDayOpus);
      add("sonnet", "Semana · Sonnet", claude.sevenDaySonnet);
      return out;
    }
    if (codex) {
      return [
        codexRow("primary", codex.primary),
        codexRow("secondary", codex.secondary),
      ].filter((row): row is UsageRow => row != null);
    }
    return [];
  });

  const plan = $derived(claude?.plan ?? codex?.plan ?? null);

  async function load(silent = false) {
    if (!hasLiveQuota) return;
    if (silent) refreshing = true;
    else loading = true;
    try {
      if (agent === "claude") claude = await agentClaudeUsage();
      else if (agent === "codex") codex = await agentCodexUsage();
      error = null;
    } catch (cause) {
      error =
        typeof cause === "string"
          ? cause
          : cause instanceof Error
            ? cause.message
            : String(cause);
    } finally {
      loading = false;
      refreshing = false;
    }
  }

  onMount(() => {
    if (!hasLiveQuota) return;
    void load();
    const timer = window.setInterval(() => void load(true), POLL_MS);
    return () => window.clearInterval(timer);
  });
</script>

<div class="usage-modal">
  <Modal
    title={`Uso de ${provider}`}
    subtitle={plan ? `Plan ${plan}` : "Uso restante"}
    size="sm"
    contained
    {onClose}
  >
    <div class="usage-stack">
      <div class="provider-mark">
        <AgentLogo {agent} size={24} />
        <div>
          <strong>{provider}</strong>
          <span>{refreshing ? "Actualizando…" : "Cuenta local activa"}</span>
        </div>
      </div>

      {#if loading}
        <div class="usage-state" aria-live="polite">
          <ProgressBar indeterminate label="Consultando cupos" />
        </div>
      {:else if error && rows.length === 0}
        <div class="usage-state is-error">
          <strong>No se pudo leer el uso</strong>
          <span>{error}</span>
          <button type="button" onclick={() => load()}>Reintentar</button>
        </div>
      {:else if rows.length > 0}
        {#if error}
          <p class="soft-error" role="status">{error}</p>
        {/if}
        <ul class="usage-list">
          {#each rows as row (row.key)}
            <li>
              <div class="usage-head">
                <span>{row.label}</span>
                <strong>{Math.max(0, Math.round(100 - row.used))}% restante</strong>
              </div>
              <ProgressBar
                value={row.used / 100}
                label={`${Math.round(row.used)}% usado`}
                tone={row.used >= 85 ? "warn" : row.used >= 60 ? "accent" : "ok"}
              />
              {#if row.reset}<span class="reset">{row.reset}</span>{/if}
            </li>
          {/each}
        </ul>
        <p class="source">
          {agent === "claude"
            ? "Misma fuente que /usage en Claude Code."
            : "Lectura oficial de account/rateLimits/read en Codex."}
        </p>
      {:else if agent === "opencode"}
        <div class="usage-state">
          <strong>No existe un saldo único de OpenCode</strong>
          <span>
            OpenCode usa el proveedor que configures. Puedes ver consumo y costo
            histórico con <code>opencode stats</code>; el cupo restante depende de
            OpenAI, Anthropic u otro proveedor.
          </span>
        </div>
      {:else if agent === "cursor-agent"}
        <div class="usage-state">
          <strong>Cursor muestra su cupo dentro de la consola</strong>
          <span>
            Escribe <code>/usage</code> en la sesión de Cursor para ver el uso y los
            límites de tu plan. El desglose completo sigue en el dashboard de Cursor.
          </span>
          {#if onRunUsageCommand}
            <button
              type="button"
              onclick={() => {
                onRunUsageCommand();
                onClose();
              }}
            >
              Escribir /usage en la consola
            </button>
          {/if}
        </div>
      {:else}
        <div class="usage-state">
          <strong>Uso no disponible</strong>
          <span>Esta consola no pertenece a un agente compatible.</span>
        </div>
      {/if}
    </div>
  </Modal>
</div>

<style>
  .usage-stack {
    display: flex;
    flex-direction: column;
    gap: 0.85rem;
  }

  .provider-mark {
    display: flex;
    align-items: center;
    gap: 0.7rem;
    color: var(--rb-text);
  }

  .provider-mark > div {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 0.08rem;
  }

  .provider-mark strong,
  .usage-state strong {
    font-size: 0.75rem;
    font-weight: 700;
  }

  .provider-mark span,
  .usage-state span,
  .source,
  .reset {
    color: var(--rb-muted);
    font-size: 0.65rem;
    line-height: 1.45;
  }

  .usage-list {
    display: flex;
    flex-direction: column;
    gap: 0.8rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .usage-list li {
    display: flex;
    flex-direction: column;
    gap: 0.28rem;
  }

  .usage-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
    font-size: 0.7rem;
  }

  .usage-head strong {
    font-variant-numeric: tabular-nums;
  }

  .reset {
    align-self: flex-end;
    color: var(--rb-faint);
  }

  .source,
  .soft-error {
    margin: 0;
  }

  .usage-state {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    border-radius: 0.7rem;
    padding: 0.75rem;
    background: color-mix(in sRGB, var(--rb-surface-2) 72%, transparent);
  }

  .usage-state.is-error,
  .soft-error {
    color: var(--rb-record);
  }

  .usage-state button {
    align-self: flex-start;
    border: 1px solid color-mix(in sRGB, var(--rb-border) 84%, transparent);
    border-radius: 0.5rem;
    padding: 0.28rem 0.55rem;
    background: var(--rb-surface);
    color: var(--rb-text);
    font: inherit;
    font-size: 0.65rem;
    cursor: pointer;
  }

  code {
    font-family: var(--rb-mono, ui-monospace, monospace);
    color: var(--rb-text);
  }
</style>
