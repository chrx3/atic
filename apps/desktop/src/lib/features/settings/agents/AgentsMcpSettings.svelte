<script lang="ts">
  /**
   * Ajustes de agentes → servidores MCP que reciben los agentes, y el hub de
   * orquestación para que agentes de otras apps usen los de Atic.
   */
  import { onMount } from "svelte";
  import { AGENTS_ENABLED } from "$core/tools";
  import { config } from "$domain/config.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import AgentMcpServersModal from "$features/settings/AgentMcpServersModal.svelte";
  import { hubSnippet, hubStatus } from "$ipc/agents";
  import type { HubStatus, McpServerConfig } from "$core/types";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Button from "$ui/Button.svelte";
  import { t } from "$domain/i18n.svelte";

  let mcpOpen = $state(false);

  /**
   * Cuántos servidores MCP se van a cargar de verdad.
   *
   * No alcanza con `enabled`: una entrada sin nombre o con el JSON roto se
   * saltea al arrancar (`mcp_servers.rs`), y contarla acá haría que el
   * subtítulo prometa más de lo que el agente va a tener.
   */
  const mcpCountLabel = $derived.by(() => {
    let total = 0;
    try {
      const lista = JSON.parse(
        config.current?.agent_mcp_servers ?? "[]",
      ) as McpServerConfig[];
      if (Array.isArray(lista)) {
        total = lista.filter(
          (s) => s?.enabled === true && !!s.name?.trim() && loadable(s.json),
        ).length;
      }
    } catch {
      /* Config rota: se muestra como ninguno, que es lo que va a cargar. */
    }
    if (total === 0) return t("settings.agents.mcpCountNone");
    if (total === 1) return t("settings.agents.mcpCountOne");
    return t("settings.agents.mcpCountMany", { n: total });
  });

  /** ¿La definición tiene la forma mínima para que el arranque la use? */
  function loadable(json: string | undefined): boolean {
    try {
      const parsed = JSON.parse(json ?? "");
      return typeof parsed === "object" && parsed !== null;
    } catch {
      return false;
    }
  }

  /**
   * Hub de orquestación MCP: un agente en otra app puede encargarle turnos a
   * los CLIs que Atic conoce. Los snippets se pegan a mano: Atic no escribe
   * `~/.codex/config.toml` ni los settings ajenos.
   */
  const HUB_HOSTS = [
    "claude-code",
    "cursor",
    "codex",
    "opencode",
    "grok",
    "antigravity",
  ] as const;
  const HUB_NAMES: Record<(typeof HUB_HOSTS)[number], string> = {
    "claude-code": "Claude Code",
    cursor: "Cursor",
    codex: "Codex",
    opencode: "OpenCode",
    grok: "Grok",
    antigravity: "Antigravity",
  };
  let hub = $state<HubStatus | null>(null);
  let hubError = $state<string | null>(null);
  let hubSnippets = $state<Record<string, string>>({});
  let hubCopied = $state<string | null>(null);

  onMount(() => {
    if (!AGENTS_ENABLED) return;
    void refreshHub();
  });

  async function refreshHub() {
    hubError = null;
    try {
      hub = await hubStatus();
      const pares = await Promise.all(
        HUB_HOSTS.map(async (h) => {
          try {
            return [h, await hubSnippet(h)] as const;
          } catch (err) {
            return [h, String(err)] as const;
          }
        }),
      );
      hubSnippets = Object.fromEntries(pares);
    } catch (err) {
      hubError = String(err);
    }
  }

  async function copyHubSnippet(host: string) {
    const texto = hubSnippets[host];
    if (!texto) return;
    try {
      await navigator.clipboard.writeText(texto);
      hubCopied = host;
      toasts.push(t("settings.agents.hubCopiedToast"));
    } catch (err) {
      toastError(err);
    }
  }
</script>

<div class="flex flex-col gap-5">
  {#if AGENTS_ENABLED}
    <SettingsGroup
      title={t("settings.agents.mcpTitle")}
      hint={t("settings.agents.mcpHint")}
    >
      <SettingsRow label={t("settings.agents.mcpTitle")} hint={mcpCountLabel}>
        {#snippet control()}
          <Button
            variant="soft"
            size="sm"
            full
            disabled={!config.current}
            onclick={() => (mcpOpen = true)}
          >
            {t("settings.agents.mcpEdit")}
          </Button>
        {/snippet}
      </SettingsRow>
    </SettingsGroup>
  {/if}

  {#if AGENTS_ENABLED}
    <SettingsGroup
      title={t("settings.agents.hubTitle")}
      hint={t("settings.agents.hubHint")}
    >
      <SettingsRow
        label={t("settings.agents.hubTitle")}
        hint={hub
          ? hub.running && hub.port !== null
            ? t("settings.agents.hubOn").replace("{port}", String(hub.port))
            : t("settings.agents.hubOff")
          : (hubError ?? "")}
      >
        {#snippet control()}
          <Button variant="soft" size="sm" full onclick={() => void refreshHub()}>
            {t("settings.agents.hubRefresh")}
          </Button>
        {/snippet}
      </SettingsRow>
      <SettingsRow label={t("settings.agents.hubPath")}>
        {#snippet control({ id })}
          <code {id} class="block text-[11px] text-faint break-all"
            >{hub?.mcpPath ?? t("settings.agents.hubPathMissing")}</code
          >
        {/snippet}
      </SettingsRow>
      {#each HUB_HOSTS as host (host)}
        <SettingsRow label={HUB_NAMES[host]}>
          {#snippet control()}
            <Button
              variant="soft"
              size="sm"
              full
              disabled={!hubSnippets[host]}
              onclick={() => void copyHubSnippet(host)}
            >
              {hubCopied === host
                ? t("settings.agents.copied")
                : t("settings.agents.copy")}
            </Button>
          {/snippet}
        </SettingsRow>
        {#if hubSnippets[host]}
          <pre
            class="m-0 max-h-48 overflow-auto rounded-sm border border-line bg-surface-2 p-2 text-[11px] leading-snug text-faint whitespace-pre-wrap break-all">{hubSnippets[
              host
            ]}</pre>
        {/if}
        {#if host === "codex"}
          <p class="text-[11px] text-faint">
            {t("settings.agents.hubCodexHelp")}
          </p>
        {/if}
      {/each}
    </SettingsGroup>
  {/if}
</div>

{#if mcpOpen}
  <AgentMcpServersModal onClose={() => (mcpOpen = false)} />
{/if}
