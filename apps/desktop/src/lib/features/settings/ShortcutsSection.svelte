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
  import { resumeShortcuts, suspendShortcuts } from "$ipc/config";
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

  /** Los globales se apagan mientras HotkeyCapture captura (y vuelven al salir). */
  function suspendGlobals() {
    void suspendShortcuts().catch(() => {});
  }

  function resumeGlobals() {
    void resumeShortcuts().catch(() => {});
  }

  const ALL_SHORTCUTS = $derived([
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
      key: "color_shortcut" as const,
      label: t("settings.shortcuts.color"),
      hint: t("settings.shortcuts.colorHint"),
      fallback: "CmdOrCtrl+Shift+C",
    },
    {
      key: "window_flip_shortcut" as const,
      label: t("settings.shortcuts.flip"),
      hint: t("settings.shortcuts.flipHint"),
      fallback: "CmdOrCtrl+Shift+B",
    },
    {
      key: "launcher_shortcut" as const,
      label: t("settings.shortcuts.launcher"),
      hint: t("settings.shortcuts.launcherHint"),
      fallback: "CmdOrCtrl+Space",
    },
  ] as const);

  const SHORTCUTS = $derived(
    AGENTS_ENABLED
      ? ALL_SHORTCUTS
      : ALL_SHORTCUTS.filter((item) => item.key !== "agents_shortcut"),
  );

  /** Rust manda los nombres tal como los registró. */
  const conflicts = $derived(new Set(config.conflicts));

  /** Qué otros comandos comparten el atajo de cada clave de config. */
  const sharedWith = $derived.by(() => {
    const map: Record<string, string[]> = {};
    for (const group of config.shared) {
      for (const key of group) {
        map[key] = group.filter((other) => other !== key);
      }
    }
    return map;
  });

  /** La etiqueta visible de cada clave, para nombrar a los otros comandos. */
  const labelsByKey = $derived.by(() => {
    const map: Record<string, string> = {};
    for (const item of ALL_SHORTCUTS) map[item.key] = item.label;
    return map;
  });

  function sharedLabels(key: string): string {
    return (sharedWith[key] ?? [])
      .map((other) => labelsByKey[other] ?? other)
      .join(", ");
  }

  function rowLabel(item: { key: string; label: string }): string {
    const marks: string[] = [];
    if (conflicts.has(item.key)) marks.push(t("settings.shortcuts.conflictSuffix"));
    if (sharedWith[item.key]) marks.push(t("settings.shortcuts.sharedSuffix"));
    return marks.length > 0 ? `${item.label} · ${marks.join(", ")}` : item.label;
  }
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

    {#if config.shared.length > 0}
      <Banner
        tone="danger"
        title={config.shared.length === 1
          ? t("settings.shortcuts.sharedOne")
          : t("settings.shortcuts.sharedMany", { count: config.shared.length })}
      >
        {t("settings.shortcuts.sharedBody")}
      </Banner>
    {/if}

    <SettingsGroup
      title={t("settings.shortcuts.title")}
      hint={t("settings.shortcuts.hint")}
    >
      {#each SHORTCUTS as item (item.key)}
        <SettingsRow
          label={rowLabel(item)}
          hint={"hint" in item ? item.hint : undefined}
        >
          {#snippet control()}
            <div class="flex flex-col gap-1">
              <HotkeyCapture
                value={cfg[item.key]}
                defaultValue={item.fallback}
                ariaLabel={t("settings.shortcuts.changeAria", { label: item.label })}
                onCaptureStart={suspendGlobals}
                onCaptureEnd={resumeGlobals}
                onChange={(sc) => patch({ [item.key]: sc })}
              />
              {#if sharedWith[item.key]}
                <p class="text-xs text-danger">
                  {t("settings.shortcuts.sharedWith", {
                    labels: sharedLabels(item.key),
                  })}
                </p>
              {/if}
            </div>
          {/snippet}
        </SettingsRow>
      {/each}
    </SettingsGroup>
  </div>
{/if}
