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
  import SshHostsPanel from "$features/agents/SshHostsPanel.svelte";
  import { agentPresenceHookSnippet } from "$ipc/agents";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Button from "$ui/Button.svelte";
  import { t } from "$domain/i18n.svelte";

  let snippet = $state("");
  let copied = $state(false);

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

  {#if AGENTS_ENABLED && config.current}
    <SshHostsPanel bind:config={config.current} {onToast} />
  {/if}
</div>
