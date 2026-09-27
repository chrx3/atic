<script lang="ts">
  /**
   * Ajustes de agentes → el servidor `atic` en la config de cada CLI.
   *
   * No confundir con `AgentsMcpSettings`: esa es la de los servidores MCP que
   * Atic les pasa a los agentes y el hub para otras apps.
   *
   * Las sesiones que abre el hub ya reciben el MCP al arrancar; las consolas
   * que abre el usuario son su CLI de siempre, y sin esto no ven las
   * herramientas. Antes solo se conectaba desde el enchufe de una consola de
   * ese mismo agente, de a uno.
   */
  import { onMount } from "svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { t } from "$domain/i18n.svelte";
  import { AGENTS } from "$features/agents/agentCatalog";
  import { agentBackends, agentMcpStatus, agentMcpToggle } from "$ipc/agents";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Button from "$ui/Button.svelte";
  import Switch from "$ui/Switch.svelte";

  /** `null` = todavía preguntando. */
  type McpState = "missing" | "signedOut" | "on" | "off" | null;

  let states = $state<Record<string, McpState>>({});
  let busy = $state<string[]>([]);
  let connectingAll = $state(false);

  const pending = $derived(AGENTS.filter((a) => states[a.cli] === "off"));
  const anyInstalled = $derived(
    AGENTS.some((a) => states[a.cli] === "on" || states[a.cli] === "off"),
  );

  onMount(() => {
    void agentBackends()
      .then((backends) =>
        Promise.all(
          AGENTS.map(async (agent) => {
            const info = backends.find((b) => b.id === agent.backend);
            let next: McpState;
            if (!info?.available) next = "missing";
            else if (info.signedIn === false) next = "signedOut";
            else
              next = (await agentMcpStatus(agent.cli).catch(() => false))
                ? "on"
                : "off";
            states = { ...states, [agent.cli]: next };
          }),
        ),
      )
      .catch(toastError);
  });

  async function setMcp(cli: string, on: boolean): Promise<void> {
    busy = [...busy, cli];
    try {
      const now = await agentMcpToggle(cli, on);
      states = { ...states, [cli]: now ? "on" : "off" };
    } catch (err) {
      toastError(err);
    } finally {
      busy = busy.filter((id) => id !== cli);
    }
  }

  /** En serie: cada uno corre el CLI de su agente, y en paralelo se pisan. */
  async function connectAll(): Promise<void> {
    connectingAll = true;
    try {
      for (const agent of pending) await setMcp(agent.cli, true);
    } finally {
      connectingAll = false;
    }
  }

  function hintFor(state: McpState): string | undefined {
    if (state === "missing") return t("settings.agents.mcpMissing");
    if (state === "signedOut") return t("settings.agents.mcpSignedOut");
    if (state === "on") return t("settings.agents.mcpConnected");
    if (state === "off") return t("settings.agents.mcpDisconnected");
    return undefined;
  }
</script>

<SettingsGroup
  title={t("settings.agents.aticMcpTitle")}
  hint={t("settings.agents.aticMcpHint")}
>
  {#each AGENTS as agent (agent.cli)}
    {@const state = states[agent.cli] ?? null}
    <SettingsRow bare>
      {#snippet control()}
        <Switch
          checked={state === "on"}
          label={agent.name}
          hint={hintFor(state)}
          disabled={(state !== "on" && state !== "off") ||
            busy.includes(agent.cli) ||
            connectingAll}
          onchange={(v) => void setMcp(agent.cli, v)}
        />
      {/snippet}
    </SettingsRow>
  {/each}
  {#if anyInstalled}
    <div class="flex flex-col gap-1">
      <Button
        variant="soft"
        size="sm"
        full
        disabled={pending.length === 0 || connectingAll}
        onclick={() => void connectAll()}
      >
        {connectingAll
          ? t("settings.agents.mcpConnecting")
          : pending.length === 0
            ? t("settings.agents.mcpAllConnected")
            : t("settings.agents.mcpConnectAll")}
      </Button>
      <p class="m-0 text-xs text-faint">{t("settings.agents.mcpRestartHint")}</p>
    </div>
  {/if}
</SettingsGroup>
