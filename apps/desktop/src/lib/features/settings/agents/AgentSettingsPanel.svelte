<script lang="ts">
  /**
   * Ajustes de agentes: el mismo contenido en los dos lugares donde se abre.
   *
   * En la ventana de agentes va como modal con navegación propia (`nav`). En
   * los Ajustes principales, que ya tienen su navegación, las secciones van
   * una debajo de otra (`stacked`): una segunda columna de pestañas dentro de
   * otra se pierde.
   */
  import { untrack } from "svelte";
  import { tabPanel } from "$lib/motion";
  import type { IconId } from "$lib/ToolIcon.svelte";
  import SettingsNav from "$patterns/SettingsNav.svelte";
  import SshHostsPanel from "$features/agents/SshHostsPanel.svelte";
  import { AGENTS_ENABLED } from "$core/tools";
  import { config } from "$domain/config.svelte";
  import { toasts } from "$domain/toasts.svelte";
  import { t } from "$domain/i18n.svelte";
  import AgentsMcpSettings from "./AgentsMcpSettings.svelte";
  import AgentsShortcutsSettings from "./AgentsShortcutsSettings.svelte";
  import AgentsShownSettings from "./AgentsShownSettings.svelte";

  type SectionId = "agents" | "mcp" | "ssh" | "shortcuts";

  let {
    layout,
    initialSection = "agents",
  }: {
    layout: "nav" | "stacked";
    initialSection?: SectionId;
  } = $props();

  let section = $state<SectionId>(untrack(() => initialSection));

  const sections = $derived([
    {
      value: "agents" as const,
      label: t("settings.agents.navAgents"),
      icon: "agents" as IconId,
    },
    ...(AGENTS_ENABLED
      ? [
          {
            value: "mcp" as const,
            label: t("settings.agents.navMcp"),
            icon: "settings" as IconId,
          },
          {
            value: "ssh" as const,
            label: t("settings.agents.navSsh"),
            icon: "window" as IconId,
          },
          {
            value: "shortcuts" as const,
            label: t("settings.agents.navShortcuts"),
            icon: "shortcuts" as IconId,
          },
        ]
      : []),
  ]);
</script>

{#snippet body(id: SectionId)}
  {#if id === "agents"}
    <AgentsShownSettings />
  {:else if id === "mcp"}
    <AgentsMcpSettings />
  {:else if id === "ssh" && config.current}
    <SshHostsPanel bind:config={config.current} onToast={(msg) => toasts.push(msg)} />
  {:else if id === "shortcuts"}
    <AgentsShortcutsSettings />
  {/if}
{/snippet}

{#if layout === "nav"}
  <div class="@container/settings flex h-full min-h-0 overflow-hidden">
    <SettingsNav bind:value={section} {sections} />
    <div class="stage min-h-0 flex-1 overflow-y-auto p-4">
      {#key section}
        <div class="pane" in:tabPanel|local out:tabPanel|local>
          {@render body(section)}
        </div>
      {/key}
    </div>
  </div>
{:else}
  <div class="flex flex-col gap-5">
    {#each sections as entry (entry.value)}
      {@render body(entry.value)}
    {/each}
  </div>
{/if}

<style>
  .stage {
    position: relative;
    height: 100%;
  }

  .pane {
    transform-origin: 50% 0;
  }
</style>
