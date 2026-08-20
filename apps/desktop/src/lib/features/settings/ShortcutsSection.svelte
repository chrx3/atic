<script lang="ts">
  /**
   * Los atajos globales.
   *
   * Se registran en el SO, así que otra app puede tenerlos tomados. Cuando eso
   * pasa Rust avisa y acá se marca cuál: un atajo que no funciona y no dice por
   * qué es de las cosas más frustrantes que puede hacer una app de escritorio.
   */
  import { config } from "$domain/config.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Banner from "$ui/Banner.svelte";
  import HotkeyCapture from "$ui/HotkeyCapture.svelte";
  import { AGENTS_ENABLED } from "$core/tools";
  import { t } from "$domain/i18n.svelte";

  const cfg = $derived(config.current);

  function patch(changes: Parameters<typeof config.patch>[0]) {
    void config.patch(changes).catch(toastError);
  }

  const ALL_SHORTCUTS = $derived(
    [
      {
        key: "global_shortcut" as const,
        label: t("settings.shortcuts.record"),
        hint: t("settings.shortcuts.recordHint"),
        fallback: "CmdOrCtrl+Shift+R",
      },
      {
        key: "dictation_shortcut" as const,
        label: t("settings.shortcuts.dictate"),
        hint: t("settings.shortcuts.dictateHint"),
        fallback: "CmdOrCtrl+Shift+D",
      },
      {
        key: "summon_pill_shortcut" as const,
        label: t("settings.shortcuts.summon"),
        hint: t("settings.shortcuts.summonHint"),
        fallback: "CmdOrCtrl+Shift+P",
      },
      {
        key: "pill_radial_shortcut" as const,
        label: t("settings.shortcuts.wheel"),
        hint: t("settings.shortcuts.wheelHint"),
        fallback: "CmdOrCtrl+Shift+Space",
      },
      {
        key: "clipboard_shortcut" as const,
        label: t("settings.shortcuts.clipboard"),
        fallback: "CmdOrCtrl+Shift+V",
      },
      {
        key: "snippets_shortcut" as const,
        label: t("settings.shortcuts.snippets"),
        fallback: "CmdOrCtrl+Shift+S",
      },
      {
        key: "agents_shortcut" as const,
        label: t("settings.shortcuts.agents"),
        hint: t("settings.shortcuts.agentsHint"),
        fallback: "CmdOrCtrl+Shift+A",
      },
      {
        key: "screenshot_shortcut" as const,
        label: t("settings.shortcuts.screenshot"),
        fallback: "CmdOrCtrl+Shift+4",
      },
      {
        key: "board_shortcut" as const,
        label: t("settings.shortcuts.board"),
        hint: t("settings.shortcuts.boardHint"),
        fallback: "CmdOrCtrl+Shift+X",
      },
      {
        key: "launcher_shortcut" as const,
        label: t("settings.shortcuts.launcher"),
        hint: t("settings.shortcuts.launcherHint"),
        fallback: "CmdOrCtrl+Space",
      },
    ] as const,
  );

  const SHORTCUTS = $derived(
    AGENTS_ENABLED
      ? ALL_SHORTCUTS
      : ALL_SHORTCUTS.filter((item) => item.key !== "agents_shortcut"),
  );

  /** Rust manda los nombres tal como los registró. */
  const conflicts = $derived(new Set(config.conflicts));
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    {#if config.conflicts.length > 0}
      <Banner
        tone="warn"
        title={config.conflicts.length === 1
          ? t("settings.shortcuts.conflictOne")
          : t("settings.shortcuts.conflictMany", { count: config.conflicts.length })}
      >
        {t("settings.shortcuts.conflictBody")}
      </Banner>
    {/if}

    <SettingsGroup
      title={t("settings.shortcuts.title")}
      hint={t("settings.shortcuts.hint")}
    >
      {#each SHORTCUTS as item (item.key)}
        <SettingsRow
          label={conflicts.has(item.key) ? `${item.label} · ${t("settings.shortcuts.conflictSuffix")}` : item.label}
          hint={"hint" in item ? item.hint : undefined}
        >
          {#snippet control()}
            <HotkeyCapture
              value={cfg[item.key]}
              defaultValue={item.fallback}
              ariaLabel={t("settings.shortcuts.changeAria", { label: item.label })}
              onChange={(sc) => patch({ [item.key]: sc })}
            />
          {/snippet}
        </SettingsRow>
      {/each}
    </SettingsGroup>
  </div>
{/if}
