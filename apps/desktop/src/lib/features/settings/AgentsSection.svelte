<script lang="ts">
  /**
   * Ajustes → Agentes.
   *
   * El pager (hooks de Claude Code) es independiente del chat de Atic. Los
   * hosts SSH solo aparecen si la consola está habilitada.
   */
  import { onMount } from "svelte";
  import { AGENTS_ENABLED, AGENT_PAGER_ENABLED } from "$core/tools";
  import { config } from "$domain/config.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import { AGENTS, shownAgents } from "$features/agents/agentCatalog";
  import SshHostsPanel from "$features/agents/SshHostsPanel.svelte";
  import Switch from "$ui/Switch.svelte";
  import { agentPresenceHookSnippet, hubSnippet, hubStatus } from "$ipc/agents";
  import type { HubStatus } from "$core/types";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Button from "$ui/Button.svelte";
  import { t } from "$domain/i18n.svelte";

  let snippet = $state("");
  let copied = $state(false);

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

  /**
   * Los agentes marcados. Vacio en la config = todos, y por eso la vista
   * arranca con todas las casillas puestas: es lo que se ve.
   */
  const shown = $derived(shownAgents(config.current?.agents_shown ?? []).map((a) => a.cli));

  /**
   * Marcar y desmarcar, guardando el orden del catalogo.
   *
   * Quedarse sin ninguno no se guarda como lista vacia —eso significa «sin
   * configurar», o sea todos—: se ignora el ultimo desmarcado. Sin agentes no
   * hay nada que lanzar ni cupo que mirar, y la pantalla no daria forma de
   * volver.
   */
  function toggleAgent(cli: string, on: boolean) {
    const next = AGENTS.map((a) => a.cli).filter((id) =>
      id === cli ? on : shown.includes(id),
    );
    if (next.length === 0) return;
    void config
      .patch({ agents_shown: next.length === AGENTS.length ? [] : next })
      .catch(toastError);
  }

  onMount(() => {
    if (!AGENT_PAGER_ENABLED) return;
    void agentPresenceHookSnippet()
      .then((raw) => {
        try {
          snippet = JSON.stringify(JSON.parse(raw), null, 2);
        } catch {
          snippet = raw;
        }
      })
      .catch(toastError);
  });

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

  async function copySnippet() {
    if (!snippet) return;
    try {
      await navigator.clipboard.writeText(snippet);
      copied = true;
      toasts.push(t("settings.agents.copiedToast"));
    } catch (err) {
      toastError(err);
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

  function onToast(msg: string) {
    toasts.push(msg);
  }
</script>

<div class="flex flex-col gap-5">
  {#if AGENT_PAGER_ENABLED}
    <SettingsGroup
      title={t("settings.agents.title")}
      hint={t("settings.agents.hint")}
    >
      <SettingsRow
        label={t("settings.agents.hooks")}
        hint={t("settings.agents.hooksHint")}
      >
        {#snippet control()}
          <Button
            variant="soft"
            size="sm"
            full
            disabled={!snippet}
            onclick={() => void copySnippet()}
          >
            {copied ? t("settings.agents.copied") : t("settings.agents.copy")}
          </Button>
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    {#if snippet}
      <pre
        class="m-0 max-h-48 overflow-auto rounded-sm border border-line bg-surface-2 p-2 text-[11px] leading-snug text-faint whitespace-pre-wrap break-all"
      >{snippet}</pre>
    {/if}
  {/if}

  {#if AGENTS_ENABLED}
    <SettingsGroup
      title={t("settings.agents.shown")}
      hint={t("settings.agents.shownHint")}
    >
      {#each AGENTS as agent (agent.cli)}
        <SettingsRow bare>
          {#snippet control()}
            <Switch
              checked={shown.includes(agent.cli)}
              label={agent.name}
              onchange={(v) => toggleAgent(agent.cli, v)}
            />
          {/snippet}
        </SettingsRow>
      {/each}
    </SettingsGroup>
  {/if}

  {#if AGENTS_ENABLED && config.current}
    <SshHostsPanel bind:config={config.current} {onToast} />
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
          <Button
            variant="soft"
            size="sm"
            full
            onclick={() => void refreshHub()}
          >
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
            class="m-0 max-h-48 overflow-auto rounded-sm border border-line bg-surface-2 p-2 text-[11px] leading-snug text-faint whitespace-pre-wrap break-all"
          >{hubSnippets[host]}</pre>
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
