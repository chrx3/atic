<script lang="ts">
  import { onMount } from "svelte";
  import type {
    ClaudeAccountUsage,
    CodexAccountUsage,
    CodexUsageWindow,
  } from "$lib/types";
  import { agentClaudeUsage, agentCodexUsage } from "$ipc/agents";
  import { t } from "$domain/i18n.svelte";
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
            : t("page.agents.usageModal.providerFallback"),
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
    title={t("page.agents.usageModal.title", { provider })}
    subtitle={plan
      ? t("page.agents.usageModal.subtitlePlan", { plan })
      : t("page.agents.usageModal.subtitleFallback")}
    size="sm"
    contained
    {onClose}
  >
    <div class="usage-stack">
      {#if hasLiveQuota}
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
            {@const remaining = Math.max(0, Math.round(100 - row.used))}
            <li>
              <div class="usage-head">
                <span>{row.label}</span>
                <span class="usage-value">
                  <strong data-numeric>{remaining}%</strong>
                  {t("page.agents.usageModal.remaining")}
                </span>
              </div>
              <ProgressBar
                value={remaining / 100}
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
            : t("page.agents.usageModal.sourceCodex")}
        </p>
      {:else if agent === "opencode"}
        <div class="usage-state">
          <strong>{t("page.agents.usageModal.opencodeTitle")}</strong>
          <span>{t("page.agents.usageModal.opencodeBody")}</span>
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
