<script lang="ts">
  /**
   * Un pedido de permiso, en el hilo y no en una barra aparte.
   *
   * Aparece donde el agente se detuvo, que es donde uno está mirando, y
   * muestra QUÉ quiere hacer —el comando o el cambio—: decidir sobre el
   * nombre de la herramienta sola es aprobar a ciegas.
   */
  import { editDiff } from "$lib/agentMarkdown";
  import type { PendingPermission } from "$lib/agentSessions.svelte";
  import type { PermissionDecision } from "$core/types";
  import { t } from "$domain/i18n.svelte";

  let {
    permission,
    agentName,
    busy = false,
    position = null,
    onMove,
    onDecide,
  }: {
    permission: PendingPermission;
    agentName: string;
    busy?: boolean;
    /** Con varios pendientes: cuál es este y cuántos hay. */
    position?: { index: number; total: number } | null;
    onMove?: (step: 1 | -1) => void;
    onDecide: (decision: PermissionDecision) => void;
  } = $props();

  /** Tope de líneas del adelanto: lo largo se lee en la herramienta ya hecha. */
  const PREVIEW_LINES = 14;

  const diff = $derived(editDiff(permission.input)?.slice(0, PREVIEW_LINES) ?? null);

  const command = $derived.by((): string | null => {
    const input = permission.input;
    if (!input || typeof input !== "object") return null;
    const value = (input as Record<string, unknown>).command;
    return typeof value === "string" ? value : null;
  });

  const raw = $derived.by((): string | null => {
    if (diff || command) return null;
    const input = permission.input;
    if (input == null) return null;
    const text = typeof input === "string" ? input : JSON.stringify(input, null, 2);
    return text.split("\n").slice(0, PREVIEW_LINES).join("\n");
  });
</script>

<section class="perm" aria-label={t("page.agents.permission.title")}>
  <div class="top">
    <p class="title">
      {t("page.agents.chat.permissionAsk", { name: agentName, tool: permission.tool })}
    </p>
    {#if position}
      <div class="pager">
        <button
          type="button"
          class="step"
          aria-label={t("page.agents.chat.previous")}
          onclick={() => onMove?.(-1)}>‹</button
        >
        <span>{position.index + 1} / {position.total}</span>
        <button
          type="button"
          class="step"
          aria-label={t("page.agents.chat.next")}
          onclick={() => onMove?.(1)}>›</button
        >
      </div>
    {/if}
  </div>
  {#if permission.description?.trim()}
    <p class="desc">{permission.description.trim()}</p>
  {/if}

  {#if diff}
    <div class="preview">
      {#each diff as line, i (i)}
        <div class="dl" data-sign={line.sign}>{line.sign} {line.text}</div>
      {/each}
    </div>
  {:else if command}
    <pre class="preview">$ {command}</pre>
  {:else if raw}
    <pre class="preview">{raw}</pre>
  {/if}

  <!-- La decisión más probable, al final: como una barra del sistema. -->
  <div class="actions">
    <button type="button" class="btn" disabled={busy} onclick={() => onDecide("deny")}>
      {t("page.agents.permission.deny")}
    </button>
    <button
      type="button"
      class="btn"
      disabled={busy}
      onclick={() => onDecide("allowAlways")}
    >
      {t("page.agents.permission.allowAlways")}
    </button>
    <button
      type="button"
      class="btn is-primary"
      disabled={busy}
      onclick={() => onDecide("allow")}
    >
      {t("page.agents.permission.allow")}
    </button>
  </div>
</section>

<style>
  .perm {
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-radius: 14px;
    padding: 12px;
    background: color-mix(in sRGB, var(--accent) 8%, var(--rb-surface));
    box-shadow:
      inset 0 0 0 1px color-mix(in sRGB, var(--accent) 30%, transparent),
      0 8px 24px -16px rgb(0 0 0 / 50%);
  }

  .top {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .pager {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 2px;
    margin-left: auto;
    color: var(--rb-faint);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .step {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 14px;
    cursor: pointer;
  }

  .step:hover {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .title {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }

  .desc {
    margin: 0;
    color: var(--rb-muted);
    font-size: 12.5px;
    text-wrap: pretty;
  }

  .preview {
    max-height: 220px;
    margin: 0;
    overflow: auto;
    border-radius: 8px;
    padding: 8px 10px;
    background: var(--rb-bg0);
    color: var(--rb-text);
    font-family: var(--rb-mono);
    font-size: 11.5px;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .dl[data-sign="+"] {
    color: var(--rb-ok);
  }

  .dl[data-sign="-"] {
    color: var(--rb-record);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }

  .btn {
    min-height: 30px;
    border: 0;
    border-radius: 8px;
    padding: 0 12px;
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition:
      background-color 120ms ease,
      scale 120ms ease;
  }

  .btn:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--rb-text) 13%, transparent);
  }

  .btn:active:not(:disabled) {
    scale: 0.96;
  }

  .btn.is-primary {
    background: var(--accent);
    color: var(--rb-on-accent, var(--rb-bg0));
  }

  .btn.is-primary:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--accent) 88%, var(--rb-text));
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
