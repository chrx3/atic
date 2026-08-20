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
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Switch from "$ui/Switch.svelte";
  import { applyTheme, type UiTheme } from "$lib/theme";
  import { t } from "$domain/i18n.svelte";

  const cfg = $derived(config.current);
  const ui = useMainUi();

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
            value={cfg.ui_language === "en" ? "en" : "es"}
            label={t("settings.language.aria")}
            options={[
              { value: "es", label: t("settings.language.es") },
              { value: "en", label: t("settings.language.en") },
            ]}
            onchange={setLanguage}
            full
          />
        {/snippet}
      </SettingsRow>
      <SettingsRow label={t("settings.appearance.theme")} hint={t("settings.appearance.themeHint")}>
        {#snippet control()}
          <SegmentedControl
            value={(cfg.ui_theme || "system") as UiTheme}
            label={t("settings.appearance.themeAria")}
            options={[
              { value: "system" as UiTheme, label: t("settings.appearance.system") },
              { value: "light" as UiTheme, label: t("settings.appearance.light") },
              { value: "dark" as UiTheme, label: t("settings.appearance.dark") },
            ]}
            onchange={setTheme}
            full
          />
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
