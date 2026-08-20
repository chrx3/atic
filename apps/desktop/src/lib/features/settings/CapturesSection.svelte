<script lang="ts">
  /** Qué pasa después de sacar una captura. */
  import { captures } from "$domain/captures.svelte";
  import { config } from "$domain/config.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Button from "$ui/Button.svelte";
  import Input from "$ui/Input.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Switch from "$ui/Switch.svelte";
  import { t } from "$domain/i18n.svelte";

  const cfg = $derived(config.current);

  function patch(changes: Parameters<typeof config.patch>[0]) {
    void config.patch(changes).catch(toastError);
  }
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    <SettingsGroup title={t("settings.captures.shelf")} hint={t("settings.captures.shelfHint")}>
      <SettingsRow label={t("settings.captures.side")}>
        {#snippet control()}
          <SegmentedControl
            value={cfg.capture_shelf_side}
            label={t("settings.captures.sideAria")}
            options={[
              { value: "left", label: t("settings.captures.left") },
              { value: "right", label: t("settings.captures.right") },
            ]}
            onchange={(v) => patch({ capture_shelf_side: v })}
            full
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow label={t("settings.captures.timeout")} hint={t("settings.captures.timeoutHint")}>
        {#snippet control({ id })}
          <Input
            {id}
            type="number"
            min="0"
            value={String(cfg.capture_shelf_timeout_seconds)}
            oninput={(e: Event) =>
              patch({
                capture_shelf_timeout_seconds:
                  Number((e.currentTarget as HTMLInputElement).value) || 0,
              })}
          />
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    <SettingsGroup title={t("settings.captures.image")}>
      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={cfg.capture_include_cursor}
            label={t("settings.captures.cursor")}
            onchange={(v) => patch({ capture_include_cursor: v })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow label={t("settings.captures.keep")} hint={t("settings.captures.keepHint")}>
        {#snippet control({ id })}
          <Input
            {id}
            type="number"
            min="0"
            value={String(cfg.capture_retention_hours)}
            oninput={(e: Event) =>
              patch({
                capture_retention_hours:
                  Number((e.currentTarget as HTMLInputElement).value) || 0,
              })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow label={t("settings.captures.cleanup")} hint={t("settings.captures.cleanupHint")}>
        {#snippet control()}
          <Button
            variant="soft"
            size="sm"
            full
            onclick={() =>
              void captures
                .cleanup()
                .then((n) =>
                  toasts.push(
                    n > 0
                      ? t("settings.captures.cleaned", { count: n })
                      : t("settings.captures.nothingExpired"),
                  ),
                )
                .catch(toastError)}
          >
            {t("settings.captures.cleanupBtn")}
          </Button>
        {/snippet}
      </SettingsRow>
    </SettingsGroup>
  </div>
{/if}
