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
  import { agentPresenceHookSnippet } from "$ipc/agents";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Button from "$ui/Button.svelte";
  import { t } from "$domain/i18n.svelte";

  let snippet = $state("");
  let copied = $state(false);

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
</div>
