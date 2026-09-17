<script lang="ts">
  import { onMount } from "svelte";
  import type {
    ClaudeAccountUsage,
    CodexAccountUsage,
    CodexUsageWindow,
  } from "$lib/types";
  import { agentClaudeUsage, agentCodexUsage, agentQuotaOverview } from "$ipc/agents";
  import { quotaRows, type QuotaRow } from "$surfaces/overlay/pill/pillQuota";
  import { t } from "$domain/i18n.svelte";
  import Modal from "$ui/Modal.svelte";
  import ProgressBar from "$ui/ProgressBar.svelte";
  import AgentLogo from "./AgentLogo.svelte";

  let {
    agent,
    onClose,
    onRunUsageCommand,
    onOpenConsole,
  }: {
    agent: string;
    onClose: () => void;
    /** Escribe `/usage` en la consola activa del agente (si hay sesión viva). */
    onRunUsageCommand?: () => void;
    /** Enfoca la consola: para agentes que pintan su uso en el propio TUI. */
    onOpenConsole?: () => void;
  } = $props();

  type UsageRow = {
    key: string;
    label: string;
    used: number;
    reset: string | null;
    /**
     * `remaining` = el número grande es lo que queda (Claude/Codex, formato
     * histórico del modal). `used` = el número grande es lo consumido, igual
     * que el hover de la pill: la barra crece con el uso en los dos.
     */
    mode: "remaining" | "used";
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
            : t("page.agents.usageModal.providerFallback"),
  );
  const hasLiveQuota = $derived(agent === "claude" || agent === "codex");
  /**
   * Los que leen el mismo snapshot normalizado que el hover de la pill.
   * OpenCode no tiene modal propio del proveedor, pero su cupo YA se consulta
   * y se muestra en la pill: repetir acá el «no hay forma de verlo» era
   * mentirle a quien lo tenía al lado.
   */
  const usesOverview = $derived(agent === "opencode" || agent === "cursor-agent");

  let claude = $state<ClaudeAccountUsage | null>(null);
  let codex = $state<CodexAccountUsage | null>(null);
  let overviewRow = $state<QuotaRow | null>(null);
  let loading = $state(false);
  let refreshing = $state(false);
  let error = $state<string | null>(null);

  function resetLabel(value: string | number | null | undefined): string | null {
    if (value == null) return null;
    const date = new Date(typeof value === "number" ? value * 1000 : value);
    if (Number.isNaN(date.getTime())) return null;
    const minutes = Math.max(0, Math.round((date.getTime() - Date.now()) / 60_000));
    if (minutes < 60) return t("page.agents.usageModal.resetMin", { n: minutes });
    const hours = Math.round(minutes / 60);
    if (hours < 36) return t("page.agents.usageModal.resetH", { n: hours });
    const days = Math.round(hours / 24);
    return t("page.agents.usageModal.resetD", { n: days });
  }

  function durationLabel(minutes: number): string {
    if (minutes % 10_080 === 0) {
      const weeks = minutes / 10_080;
      return weeks === 1
        ? t("page.agents.usageModal.week")
        : t("page.agents.usageModal.weeks", { n: weeks });
    }
    if (minutes % 1_440 === 0) {
      return t("page.agents.usageModal.days", { n: minutes / 1_440 });
    }
    if (minutes % 60 === 0) {
      return t("page.agents.usageModal.hours", { n: minutes / 60 });
    }
    return t("page.agents.usageModal.minutes", { n: minutes });
  }

  function codexRow(key: string, window: CodexUsageWindow | null): UsageRow | null {
    if (!window) return null;
    return {
      key,
      label: durationLabel(window.windowDurationMins),
      used: window.usedPercent,
      reset: resetLabel(window.resetsAt),
      mode: "remaining",
    };
  }

  /** Etiqueta de ventana con las mismas palabras que el hover de la pill. */
  function overviewLabel(row: QuotaRow, bar: QuotaRow["bars"][number]): string {
    if (bar.window === "custom") {
      // Ventana sin nombre conocido: el largo en minutos la describe.
      return bar.minutes == null
        ? t("pill.quota.window.unknown")
        : durationLabel(bar.minutes);
    }
    if (bar.window === "model") {
      return t("pill.quota.window.modelWeek", { model: bar.model ?? "" });
    }
    return t(`pill.quota.window.${bar.window}`);
  }

  const overviewRows = $derived.by((): UsageRow[] => {
    const row = overviewRow;
    if (!row) return [];
    return row.bars.map((bar) => ({
      key: `${bar.window}:${bar.model ?? ""}`,
      label: overviewLabel(row, bar),
      used: bar.percent,
      // `resetLabel` espera epoch en SEGUNDOS; el snapshot trae milisegundos.
      reset: resetLabel(bar.resetsAt == null ? null : Math.round(bar.resetsAt / 1000)),
      // Igual que el hover de la pill: el número grande es lo consumido.
      mode: "used" as const,
    }));
  });

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
            mode: "remaining",
          });
        }
      };
      add("5h", t("page.agents.usageModal.fiveHours"), claude.fiveHour);
      add("7d", t("page.agents.usageModal.week"), claude.sevenDay);
      add("opus", t("page.agents.usageModal.weekOpus"), claude.sevenDayOpus);
      add("sonnet", t("page.agents.usageModal.weekSonnet"), claude.sevenDaySonnet);
      return out;
    }
    if (codex) {
      return [
        codexRow("primary", codex.primary),
        codexRow("secondary", codex.secondary),
      ].filter((row): row is UsageRow => row != null);
    }
    if (overviewRow) return overviewRows;
    return [];
  });

  const plan = $derived(claude?.plan ?? codex?.plan ?? overviewRow?.plan ?? null);
  /** Las filas del snapshot se presentan como consumido: cambia el subtítulo. */
  const showingOverview = $derived(usesOverview && overviewRow != null);

  async function load(silent = false) {
    if (!hasLiveQuota && !usesOverview) return;
    if (silent) refreshing = true;
    else loading = true;
    try {
      if (agent === "claude") claude = await agentClaudeUsage();
      else if (agent === "codex") codex = await agentCodexUsage();
      else if (usesOverview) {
        const row = quotaRows(await agentQuotaOverview()).find(
          (candidate) => candidate.agent === agent,
        );
        overviewRow = row ?? null;
      }
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
    if (!hasLiveQuota && !usesOverview) return;
    void load();
    const timer = window.setInterval(() => void load(true), POLL_MS);
    return () => window.clearInterval(timer);
  });
</script>

<div class="usage-modal">
  <Modal
    title={t("page.agents.usageModal.title", { provider })}
    subtitle={plan
      ? t("page.agents.usageModal.subtitlePlan", { plan })
      : showingOverview
        ? t("page.agents.usageModal.subtitleUsage")
        : t("page.agents.usageModal.subtitleFallback")}
    size="sm"
    contained
    {onClose}
  >
    <div class="usage-stack">
      {#if hasLiveQuota || (usesOverview && rows.length > 0)}
        <div class="provider-mark">
          <AgentLogo {agent} size={22} />
          <span aria-live="polite">
            {refreshing
              ? t("page.agents.usageModal.refreshing")
              : t("page.agents.usageModal.accountActive")}
          </span>
        </div>
      {/if}

      {#if loading}
        <div class="usage-state" aria-live="polite">
          <ProgressBar indeterminate label={t("page.agents.usageModal.loading")} />
        </div>
      {:else if error && rows.length === 0}
        <div class="usage-state is-error">
          <strong>{t("page.agents.usageModal.readFail")}</strong>
          <span>{error}</span>
          <button type="button" onclick={() => load()}>
            {t("page.agents.usageModal.retry")}
          </button>
        </div>
      {:else if rows.length > 0}
        {#if error}
          <p class="soft-error" role="status">{error}</p>
        {/if}
        <ul class="usage-list">
          {#each rows as row (row.key)}
            {@const shown =
              row.mode === "used"
                ? Math.min(100, Math.max(0, Math.round(row.used)))
                : Math.max(0, Math.round(100 - row.used))}
            <li>
              <div class="usage-head">
                <span>{row.label}</span>
                <span class="usage-value">
                  <strong data-numeric>{shown}%</strong>
                  {row.mode === "used"
                    ? t("page.agents.usageModal.used")
                    : t("page.agents.usageModal.remaining")}
                </span>
              </div>
              <ProgressBar
                value={shown / 100}
                ariaLabel={row.label}
                tone={row.used >= 85 ? "warn" : row.used >= 60 ? "accent" : "ok"}
              />
              {#if row.reset}<span class="reset">{row.reset}</span>{/if}
            </li>
          {/each}
        </ul>
        <p class="source">
          {agent === "claude"
            ? t("page.agents.usageModal.sourceClaude")
            : agent === "codex"
              ? t("page.agents.usageModal.sourceCodex")
              : t("page.agents.usageModal.sourcePill")}
        </p>
      {:else if agent === "opencode"}
        <div class="usage-state">
          <strong>{t("page.agents.usageModal.opencodeTitle")}</strong>
          <span>{t("page.agents.usageModal.opencodeBody")}</span>
          {#if onOpenConsole}
            <button
              type="button"
              onclick={() => {
                onOpenConsole();
                onClose();
              }}
            >
              {t("page.agents.usageModal.openConsole")}
            </button>
          {/if}
        </div>
      {:else if agent === "cursor-agent"}
        <div class="usage-state">
          <strong>{t("page.agents.usageModal.cursorTitle")}</strong>
          <span>{t("page.agents.usageModal.cursorBody")}</span>
          {#if onRunUsageCommand}
            <button
              type="button"
              onclick={() => {
                onRunUsageCommand();
                onClose();
              }}
            >
              {t("page.agents.usageModal.runUsage")}
            </button>
          {/if}
        </div>
      {:else}
        <div class="usage-state">
          <strong>{t("page.agents.usageModal.unavailableTitle")}</strong>
          <span>{t("page.agents.usageModal.unavailableBody")}</span>
        </div>
      {/if}
    </div>
  </Modal>
</div>

<style>
  .usage-stack {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .provider-mark {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    color: var(--text);
  }

  .provider-mark span {
    color: var(--muted);
    font-size: 0.6875rem;
    line-height: 1.45;
  }

  .usage-list {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .usage-list li {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  .usage-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
  }

  .usage-head > span:first-child {
    color: var(--muted);
    font-size: 0.75rem;
  }

  .usage-value {
    color: var(--muted);
    font-size: 0.6875rem;
    white-space: nowrap;
  }

  .usage-value strong {
    color: var(--text);
    font-size: 0.8125rem;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }

  .reset {
    align-self: flex-end;
    color: var(--faint);
    font-size: 0.6875rem;
    line-height: 1.45;
  }

  .source,
  .soft-error {
    margin: 0;
    color: var(--faint);
    font-size: 0.6875rem;
    line-height: 1.45;
  }

  .soft-error {
    color: var(--danger);
  }

  .usage-state {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    border-radius: var(--radius-sm);
    padding: 0.75rem;
    background: color-mix(in sRGB, var(--surface-2) 72%, transparent);
  }

  .usage-state strong {
    color: var(--text);
    font-size: 0.75rem;
    font-weight: 650;
  }

  .usage-state span {
    color: var(--muted);
    font-size: 0.6875rem;
    line-height: 1.45;
  }

  .usage-state.is-error strong {
    color: var(--danger);
  }

  .usage-state button {
    align-self: flex-start;
    border: 1px solid color-mix(in sRGB, var(--line) 84%, transparent);
    border-radius: var(--radius-xs);
    padding: 0.3rem 0.6rem;
    background: var(--surface);
    color: var(--text);
    font: inherit;
    font-size: 0.6875rem;
    cursor: pointer;
    transition:
      background-color var(--duration-quick) var(--ease-smooth-out),
      border-color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .usage-state button:hover {
    background: var(--surface-2);
    border-color: var(--line-strong);
  }

  .usage-state button:active {
    transform: scale(0.96);
  }

  .usage-state button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
