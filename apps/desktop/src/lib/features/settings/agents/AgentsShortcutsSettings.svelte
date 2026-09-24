<script lang="ts">
  /**
   * Ajustes de agentes → atajos. Solo se leen: los de la ventana están fijos
   * (`AgentsWorkspace.onKey`) y el global se cambia en Ajustes → Atajos, donde
   * se detectan los choques con los demás.
   */
  import { config } from "$domain/config.svelte";
  import { shortcutParts } from "$core/hotkeys";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Kbd from "$ui/Kbd.svelte";
  import { t } from "$domain/i18n.svelte";

  /** Los de `AgentsWorkspace.onKey`: si cambian allá, cambian acá. */
  const WINDOW_KEYS = [
    { label: "settings.agents.shortcutNew", combo: "CommandOrControl+N" },
    { label: "settings.agents.shortcutClose", combo: "CommandOrControl+W" },
    { label: "settings.agents.shortcutNext", combo: "CommandOrControl+Tab" },
    { label: "settings.agents.shortcutPrev", combo: "CommandOrControl+Shift+Tab" },
    { label: "settings.agents.shortcutGoTo", combo: "CommandOrControl+1" },
  ] as const;

  const toggle = $derived(config.current?.agents_shortcut ?? "");
</script>

<div class="flex flex-col gap-5">
  <SettingsGroup
    title={t("settings.agents.shortcutsTitle")}
    hint={t("settings.agents.shortcutsHint")}
  >
    <SettingsRow
      label={t("settings.agents.shortcutToggle")}
      hint={t("settings.agents.shortcutToggleHint")}
    >
      {#snippet control()}
        {#if toggle}
          <Kbd combo={shortcutParts(toggle).join("+")} />
        {:else}
          <span class="text-xs text-faint">{t("settings.agents.shortcutNone")}</span>
        {/if}
      {/snippet}
    </SettingsRow>
    {#each WINDOW_KEYS as key (key.combo)}
      <SettingsRow label={t(key.label)}>
        {#snippet control()}
          <Kbd combo={shortcutParts(key.combo).join("+")} />
        {/snippet}
      </SettingsRow>
    {/each}
  </SettingsGroup>
</div>
