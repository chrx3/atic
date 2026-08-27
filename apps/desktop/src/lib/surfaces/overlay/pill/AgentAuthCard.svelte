<script lang="ts">
  import { tip } from "$surfaces/overlay/tip.svelte";
  /**
   * Diálogo compacto de autorización junto a la pill.
   *
   * No usa `<dialog showModal>`: el overlay es click-through y un top-layer
   * a pantalla completa taparía el escritorio. Es tinta sobre la piel líquida
   * del overlay (sin fondo propio): el host publica la silueta al campo y
   * nace fundida a la pill.
   */
  import type { PendingPermission } from "$lib/agentSessions.svelte";
  import type { PermissionDecision } from "$core/types";

  let {
    permission,
    busy = false,
    onOpenConsole,
    onDecide,
  }: {
    permission: PendingPermission;
    busy?: boolean;
    onOpenConsole: () => void;
    onDecide: (decision: PermissionDecision) => void;
  } = $props();

  const summary = $derived.by(() => {
    const desc = permission.description?.trim();
    if (desc) return desc;
    return `El agente quiere usar «${permission.tool}».`;
  });

  const title = $derived(
    permission.description?.trim()
      ? `${permission.tool} · ${permission.description.trim()}`
      : permission.tool,
  );
</script>

<div
  class="auth"
  role="alertdialog"
  aria-modal="true"
  aria-labelledby="agent-auth-title"
  aria-describedby="agent-auth-body"
  data-no-drag
>
  <div class="auth-row">
    <p id="agent-auth-title" class="auth-tool" use:tip={title}>
      <strong>{permission.tool}</strong>
      {#if permission.description?.trim()}
        <span class="auth-w"> · {permission.description.trim()}</span>
      {/if}
    </p>
    <p id="agent-auth-body" class="sr-only">{summary}</p>
    <div class="auth-acts">
      <button
        type="button"
        class="auth-btn is-ghost"
        disabled={busy}
        onclick={onOpenConsole}
      >
        abrir consola
      </button>
      <button
        type="button"
        class="auth-btn is-danger"
        disabled={busy}
        onclick={() => onDecide("deny")}
      >
        rechazar
      </button>
      <button
        type="button"
        class="auth-btn is-warn"
        disabled={busy}
        onclick={() => onDecide("allow")}
      >
        aprobar
      </button>
      <button
        type="button"
        class="auth-btn is-ok"
        disabled={busy}
        onclick={() => onDecide("allowAlways")}
      >
        aprobar siempre
      </button>
    </div>
  </div>
</div>

<style>
  .auth {
    box-sizing: border-box;
    /* Radio host líquido = 12; padding ~6 → botones pill no compiten de radio. */
    width: min(22rem, 100%);
    padding: 0.375rem 0.4rem 0.375rem 0.55rem;
    border-radius: 12px;
    /* Sin fondo ni sombra: la piel líquida del overlay es la superficie. */
    color: var(--text);
    -webkit-font-smoothing: antialiased;
  }

  .auth-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.3rem 0.4rem;
  }

  .auth-tool {
    margin: 0;
    min-width: 0;
    flex: 1 1 8rem;
    color: var(--text);
    font-family: var(--font-sans);
    font-size: 0.6875rem;
    font-weight: 650;
    line-height: 1.25;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .auth-w {
    color: var(--muted);
    font-weight: 400;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    padding: 0;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  .auth-acts {
    display: flex;
    flex-shrink: 0;
    flex-wrap: nowrap;
    align-items: center;
    justify-content: flex-end;
    gap: 0.2rem;
    margin-left: auto;
  }

  .auth-btn {
    position: relative;
    display: inline-flex;
    min-height: 1.5rem;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 999px;
    padding: 0 0.52rem;
    background: color-mix(in sRGB, var(--text) 8%, transparent);
    color: var(--text);
    font-family: var(--font-sans);
    font-size: 0.5625rem;
    font-weight: 650;
    letter-spacing: 0.02em;
    line-height: 1;
    cursor: pointer;
    transition:
      transform var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      color var(--duration-quick) var(--ease-smooth-out),
      opacity var(--duration-quick) var(--ease-smooth-out),
      box-shadow var(--duration-quick) var(--ease-smooth-out);
  }

  .auth-btn::after {
    content: "";
    position: absolute;
    inset-block: 50%;
    inset-inline: 0;
    height: 40px;
    transform: translateY(-50%);
  }

  .auth-btn:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--text) 14%, transparent);
  }

  .auth-btn:active:not(:disabled) {
    transform: scale(0.96);
  }

  .auth-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .auth-btn:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 1.5px var(--accent);
  }

  .auth-btn.is-ghost {
    background: transparent;
    color: var(--muted);
    font-weight: 550;
  }

  .auth-btn.is-ghost:hover:not(:disabled) {
    color: var(--text);
    background: color-mix(in sRGB, var(--text) 8%, transparent);
  }

  /* Rechazar: más presencia que el resto (tinta + anillo), sin gritar. */
  .auth-btn.is-danger {
    padding-inline: 0.62rem;
    background: color-mix(in sRGB, var(--rec) 18%, transparent);
    color: var(--rec);
    font-weight: 700;
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--rec) 42%, transparent);
  }

  .auth-btn.is-danger:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--rec) 28%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--rec) 55%, transparent);
  }

  .auth-btn.is-danger:focus-visible {
    box-shadow: inset 0 0 0 1.5px var(--rec);
  }

  .auth-btn.is-warn {
    background: color-mix(in sRGB, var(--warn) 18%, transparent);
    color: var(--warn);
  }

  .auth-btn.is-warn:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--warn) 28%, transparent);
  }

  .auth-btn.is-warn:focus-visible {
    box-shadow: inset 0 0 0 1.5px var(--warn);
  }

  .auth-btn.is-ok {
    background: color-mix(in sRGB, var(--ok) 18%, transparent);
    color: var(--ok);
  }

  .auth-btn.is-ok:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--ok) 28%, transparent);
  }

  .auth-btn.is-ok:focus-visible {
    box-shadow: inset 0 0 0 1.5px var(--ok);
  }

  @media (prefers-reduced-motion: reduce) {
    .auth-btn {
      transition: none;
    }
  }
</style>
