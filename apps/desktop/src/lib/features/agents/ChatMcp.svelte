<script lang="ts">
  /**
   * Los servidores MCP de la sesión, como los informó el agente.
   *
   * El de Atic se inyecta solo al arrancar un chat (`bridge.rs`): con él, el
   * agente puede abrir y delegar en otros agentes, y esas sesiones aparecen
   * como fichas de sólo lectura en el rail. Sin esto había que adivinar si
   * estaba enchufado.
   */
  import type { McpServerState } from "$lib/types";
  import { t } from "$domain/i18n.svelte";
  import Icon from "$ui/Icon.svelte";
  import { Plug } from "$lib/icons";
  import ChatPopover from "./ChatPopover.svelte";

  let {
    servers,
    tools,
    open,
    onToggle,
  }: {
    servers: McpServerState[];
    /** Herramientas de la sesión: de acá sale cuántas aporta cada servidor. */
    tools: string[];
    open: boolean;
    onToggle: (open: boolean) => void;
  } = $props();

  const ATIC = "atic";

  const isUp = (status: string) => status === "connected" || status === "ready";
  const connected = $derived(servers.filter((s) => isUp(s.status)).length);
  const failing = $derived(servers.some((s) => s.status === "failed"));

  function toolCount(name: string): number {
    const prefix = `mcp__${name}__`;
    return tools.filter((tool) => tool.startsWith(prefix)).length;
  }
</script>

<ChatPopover
  {open}
  {onToggle}
  label={t("page.agents.chat.mcp")}
  width={300}
  align="right"
>
  {#snippet trigger()}
    <Icon icon={Plug} size={13} />
    <span class="count">{connected}</span>
    {#if failing}
      <!-- Los conectados en color normal: que alguno falle no es que falle todo. -->
      <span class="bad-dot" aria-hidden="true"></span>
    {/if}
  {/snippet}

  <p class="head">{t("page.agents.chat.mcp")}</p>
  {#each servers as server (server.name)}
    {@const count = toolCount(server.name)}
    <div class="server">
      <span
        class="dot"
        class:is-up={isUp(server.status)}
        class:is-bad={server.status === "failed"}
        aria-hidden="true"
      ></span>
      <span class="name">{server.name}</span>
      <span class="status">
        {count > 0 ? t("page.agents.chat.mcpTools", { n: count }) : server.status}
      </span>
    </div>
    {#if server.name === ATIC}
      <p class="note">{t("page.agents.chat.mcpAtic")}</p>
    {/if}
  {/each}
</ChatPopover>

<style>
  .count {
    font-variant-numeric: tabular-nums;
  }

  .bad-dot {
    width: 5px;
    height: 5px;
    margin-left: -2px;
    border-radius: 999px;
    background: var(--rb-record);
  }

  .head {
    margin: 2px 8px 6px;
    color: var(--rb-faint);
    font-size: 11px;
    font-weight: 600;
  }

  .server {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 8px;
    font-size: 12.5px;
  }

  .dot {
    flex-shrink: 0;
    width: 6px;
    height: 6px;
    border-radius: 999px;
    background: var(--rb-faint);
  }

  .dot.is-up {
    background: var(--rb-ok);
  }

  .dot.is-bad {
    background: var(--rb-record);
  }

  .name {
    min-width: 0;
    overflow: hidden;
    font-weight: 560;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .status {
    flex-shrink: 0;
    margin-left: auto;
    color: var(--rb-faint);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .note {
    margin: 0 8px 4px 22px;
    color: var(--rb-muted);
    font-size: 11.5px;
    line-height: 1.45;
    text-wrap: pretty;
  }
</style>
