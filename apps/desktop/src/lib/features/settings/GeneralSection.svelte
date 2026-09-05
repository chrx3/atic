<script lang="ts">
  /** Tema, arranque, tutorial y retención. */
  import { config } from "$domain/config.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { openDataDir } from "$ipc/config";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import { useMainUi } from "$surfaces/main/mainUi.svelte";
  import Button from "$ui/Button.svelte";
  import Input from "$ui/Input.svelte";
  import Select from "$ui/Select.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Switch from "$ui/Switch.svelte";
  import ThemePicker from "./ThemePicker.svelte";
  import ThemeEditor from "./ThemeEditor.svelte";
  import {
    applyTheme,
    customKnobs,
    derivePalette,
    normalizeTheme,
    readPalette,
    UI_THEMES,
    type ThemeKnobs,
    type UiTheme,
  } from "$lib/theme";
  import { t } from "$domain/i18n.svelte";
  import AticMark from "$lib/AticMark.svelte";

  const cfg = $derived(config.current);
  const ui = useMainUi();

  const theme = $derived(normalizeTheme(cfg?.ui_theme));

  // Las perillas persistidas mandan; `customKnobs()` es el cache del webview y
  // cubre el primer render, antes de que llegue la config.
  const knobs = $derived<ThemeKnobs>(cfg?.ui_theme_custom ?? customKnobs());

  /** La muestra del personalizado: sus tokens, no los de una paleta del CSS. */
  const customColors = $derived.by(() => {
    if (typeof document === "undefined") return undefined;
    const palette = derivePalette(
      readPalette(knobs.base),
      { light: readPalette("light"), dark: readPalette("dark") },
      knobs,
    );
    return {
      bg: palette.bg,
      "surface-2": palette["surface-2"],
      accent: palette.accent,
      muted: palette.muted,
      line: palette.line,
    };
  });

  const themeOptions = $derived(
    UI_THEMES.map((value) => ({
      value,
      label: t(`settings.appearance.${value}`),
      colors: value === "custom" ? customColors : undefined,
    })),
  );

  const OVERLAY_SCALE_OPTIONS = $derived([
    { value: "1", label: t("settings.appearance.scaleNormal") },
    { value: "1.25", label: t("settings.appearance.scaleLarge") },
    { value: "1.5", label: t("settings.appearance.scaleHuge") },
  ]);

  function overlayScaleKey(n: number): string {
    const snapped = Math.round(n * 20) / 20;
    if (snapped <= 1.12) return "1";
    if (snapped <= 1.37) return "1.25";
    return "1.5";
  }

  const overlayScale = $derived(overlayScaleKey(cfg?.overlay_scale ?? 1));
  const overlayPreviewScale = $derived(Number(overlayScale));

  function patch(changes: Parameters<typeof config.patch>[0]) {
    void config.patch(changes).catch(toastError);
  }

  function setLanguage(language: string) {
    patch({ ui_language: language });
  }

  function setTheme(theme: UiTheme) {
    // Se aplica al DOM en el acto y se guarda después: esperar el viaje a Rust
    // para repintar hace que el tema se sienta pegajoso.
    applyTheme(theme);
    patch({ ui_theme: theme });
  }
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    <SettingsGroup title={t("settings.appearance.title")}>
      <SettingsRow label={t("settings.language.label")} hint={t("settings.language.hint")}>
        {#snippet control()}
          <SegmentedControl
            value={["system", "en", "es"].includes(cfg.ui_language) ? cfg.ui_language : "system"}
            label={t("settings.language.aria")}
            options={[
              { value: "system", label: t("settings.language.system") },
              { value: "es", label: t("settings.language.es") },
              { value: "en", label: t("settings.language.en") },
            ]}
            onchange={setLanguage}
            full
          />
        {/snippet}
      </SettingsRow>
      <SettingsRow label={t("settings.language.speech")} hint={t("settings.language.speechHint")}>
        {#snippet control()}
          <Select
            value={cfg.language}
            aria-label={t("settings.language.speechAria")}
            options={[
              { value: "system", label: t("onboarding.langSystem") },
              { value: "es", label: t("onboarding.langEs") },
              { value: "en", label: t("onboarding.langEn") },
              { value: "pt", label: t("onboarding.langPt") },
              { value: "auto", label: t("onboarding.langAuto") },
            ]}
            onchange={(event: Event) =>
              patch({ language: (event.currentTarget as HTMLSelectElement).value })}
          />
        {/snippet}
      </SettingsRow>
      <SettingsRow bare>
        {#snippet control()}
          <div class="flex flex-col gap-2 py-0.5">
            <div class="flex flex-col gap-0.5">
              <span class="text-sm text-text">{t("settings.appearance.theme")}</span>
              <p class="text-xs text-faint">{t("settings.appearance.themeHint")}</p>
            </div>
            <ThemePicker
              value={theme}
              label={t("settings.appearance.themeAria")}
              options={themeOptions}
              onchange={setTheme}
            />
            {#if theme === "custom"}
              <ThemeEditor {knobs} onchange={(next) => patch({ ui_theme_custom: next })} />
            {/if}
          </div>
        {/snippet}
      </SettingsRow>
      <SettingsRow bare>
        {#snippet control()}
          <div class="flex flex-col gap-3 py-0.5">
            <div class="flex items-center gap-3">
              <!-- Hueco fijo al máximo (150%): el disco escala por transform
                   y el texto de al lado no se reacomoda. -->
              <div class="grid h-12 w-12 shrink-0 place-items-center" aria-hidden="true">
                <div
                  class="grid size-8 place-items-center rounded-full
                         bg-[var(--skin)] text-[var(--text)]
                         shadow-[var(--shadow-goo)]
                         transition-transform duration-(--duration-fast) ease-calm"
                  style="transform: scale({overlayPreviewScale})"
                >
                  <AticMark size={17} strokeWidth={1.5} alive track="window" />
                </div>
              </div>
              <div class="flex min-w-0 flex-col gap-0.5">
                <span class="text-sm text-text">{t("settings.appearance.overlayScale")}</span>
                <p class="text-pretty text-xs text-faint">
                  {t("settings.appearance.overlayScaleHint")}
                </p>
              </div>
            </div>
            <SegmentedControl
              value={overlayScale}
              label={t("settings.appearance.overlayScaleAria")}
              options={OVERLAY_SCALE_OPTIONS}
              onchange={(value) => patch({ overlay_scale: Number(value) })}
              full
            />
          </div>
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    <SettingsGroup title={t("settings.startup.title")}>
      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={cfg.autostart}
            label={t("settings.startup.autostart")}
            hint={t("settings.startup.autostartHint")}
            onchange={(v) => patch({ autostart: v })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={cfg.show_pill}
            label={t("settings.startup.showPill")}
            hint={t("settings.startup.showPillHint")}
            onchange={(v) => patch({ show_pill: v })}
          />
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    <SettingsGroup title={t("settings.tutorial.title")} hint={t("settings.tutorial.hint")}>
      <SettingsRow label={t("settings.tutorial.firstUse")}>
        {#snippet control()}
          <Button
            variant="soft"
            size="sm"
            full
            onclick={() => void ui.replayOnboarding().catch(toastError)}
          >
            {t("settings.tutorial.replay")}
          </Button>
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    <SettingsGroup title={t("settings.data.title")} hint={t("settings.data.hint")}>
      <SettingsRow
        label={t("settings.data.keep")}
        hint={t("settings.data.keepHint")}
      >
        {#snippet control({ id })}
          <Input
            {id}
            type="number"
            min="0"
            value={String(cfg.retention_days)}
            oninput={(e: Event) =>
              patch({
                retention_days:
                  Number((e.currentTarget as HTMLInputElement).value) || 0,
              })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={cfg.retention_auto_cleanup}
            label={t("settings.data.autoCleanup")}
            onchange={(v) => patch({ retention_auto_cleanup: v })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow label={t("settings.data.folder")} hint={t("settings.data.folderHint")}>
        {#snippet control()}
          <Button
            variant="soft"
            size="sm"
            full
            onclick={() => void openDataDir("data").catch(toastError)}
          >
            {t("settings.data.openFolder")}
          </Button>
        {/snippet}
      </SettingsRow>
    </SettingsGroup>
  </div>
{/if}
