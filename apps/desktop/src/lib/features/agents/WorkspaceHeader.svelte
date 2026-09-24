<script lang="ts">
  /**
   * La barra de arriba de lo que está a la vista: de qué se trata, dónde
   * trabaja y qué se puede hacer con eso.
   *
   * El título es lo que el usuario pidió, no el nombre del agente: con tres
   * chats de Claude abiertos, «Claude Code» no distingue ninguno. Solo dibuja;
   * qué hace cada botón lo decide la ventana.
   */
  import { t } from "$domain/i18n.svelte";
  import Icon from "$ui/Icon.svelte";
  import { Copy, Folder, SquareTerminal, X } from "$lib/icons";
  import AgentLogo from "./AgentLogo.svelte";
  import type { ItemStatus } from "./agentWorkspace";

  let {
    title,
    agentCli,
    folder,
    status,
    onTerminalHere,
    onCopyPath,
    onClose,
  }: {
    title: string;
    agentCli: string | null;
    /** Ruta completa; vacía si no se sabe. */
    folder: string;
    status: ItemStatus;
    /** Sin esto no se ofrece: una terminal no abre otra terminal. */
    onTerminalHere?: () => void;
    onCopyPath: () => void;
    onClose: () => void;
  } = $props();

  /** Solo lo que pide atención se anuncia arriba; «lista» no dice nada. */
  const LOUD: ItemStatus[] = ["working", "waiting", "failed"];

  function folderName(path: string): string {
    return (
      path
        .replace(/[/\\]+$/, "")
        .split(/[/\\]/)
        .pop() || path
    );
  }
</script>

<header class="bar">
  <span class="who">
    <AgentLogo agent={agentCli} size={16} />
  </span>
  <h1 class="title" {title}>{title}</h1>

  {#if folder}
    <button
      type="button"
      class="chip"
      title={`${folder} · ${t("page.agents.window.copyPath")}`}
      onclick={onCopyPath}
    >
      <Icon icon={Folder} size={12} />
      <span class="chip-text">{folderName(folder)}</span>
      <span class="chip-copy" aria-hidden="true"><Icon icon={Copy} size={11} /></span>
    </button>
  {/if}

  {#if LOUD.includes(status)}
    <span class="state is-{status}">
      <span class="state-dot" aria-hidden="true"></span>
      {t(`page.agents.window.status.${status}`)}
    </span>
  {/if}

  <span class="actions">
    {#if onTerminalHere}
      <button
        type="button"
        class="icon"
        aria-label={t("page.agents.window.terminalHere")}
        title={t("page.agents.window.terminalHere")}
        onclick={onTerminalHere}
      >
        <Icon icon={SquareTerminal} size={15} />
      </button>
    {/if}
    <button
      type="button"
      class="icon"
      aria-label={t("page.agents.window.close", { name: title })}
      title={t("page.agents.window.close", { name: title })}
      onclick={onClose}
    >
      <Icon icon={X} size={15} />
    </button>
  </span>
</header>

<style>
  .bar {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 10px;
    min-width: 0;
    height: 46px;
    box-sizing: border-box;
    padding: 0 10px 0 16px;
    border-bottom: 1px solid color-mix(in sRGB, var(--rb-text) 7%, transparent);
  }

  .who {
    display: grid;
    flex-shrink: 0;
    place-items: center;
  }

  .title {
    min-width: 0;
    margin: 0;
    overflow: hidden;
    color: var(--rb-text);
    font-size: 13.5px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chip {
    display: inline-flex;
    flex-shrink: 1;
    align-items: center;
    gap: 5px;
    min-width: 0;
    max-width: 220px;
    height: 24px;
    border: 0;
    border-radius: 999px;
    padding: 0 9px;
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
    color: var(--rb-muted);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    transition:
      background-color var(--duration-fast) ease,
      color var(--duration-fast) ease;
  }

  .chip:hover {
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
    color: var(--rb-text);
  }

  .chip-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* El ícono de copiar aparece al pasar: en reposo la ficha solo informa. */
  .chip-copy {
    display: grid;
    width: 0;
    overflow: hidden;
    opacity: 0;
    transition:
      width var(--duration-fast) ease,
      opacity var(--duration-fast) ease;
  }

  .chip:hover .chip-copy {
    width: 11px;
    opacity: 1;
  }

  .state {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    gap: 6px;
    color: var(--rb-muted);
    font-size: 11.5px;
  }

  .state-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
  }

  .state.is-working .state-dot {
    animation: pulse 1.2s ease-in-out infinite;
  }

  .state.is-waiting {
    color: var(--rb-text);
  }

  .state.is-failed .state-dot {
    background: var(--rb-record);
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  .actions {
    display: flex;
    flex-shrink: 0;
    gap: 2px;
    margin-left: auto;
  }

  .icon {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      background-color var(--duration-fast) ease,
      color var(--duration-fast) ease;
  }

  .icon:hover {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
  }

  @media (prefers-reduced-motion: reduce) {
    .state.is-working .state-dot {
      animation: none;
    }
  }
</style>
