<script lang="ts">
  /** Cómo se comporta el dictado. */
  import { config } from "$domain/config.svelte";
  import { models } from "$domain/models.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { formatMegabytes } from "$core/format";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import GroqKeyField from "./GroqKeyField.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Select from "$ui/Select.svelte";
  import { t, whisperModelLabel } from "$domain/i18n.svelte";

  const cfg = $derived(config.current);

  function patch(changes: Parameters<typeof config.patch>[0]) {
    void config.patch(changes).catch(toastError);
  }

  const modelOptions = $derived(
    models.items.map((m) => ({
      value: m.id,
      label: `${whisperModelLabel(m.id)} · ${formatMegabytes(m.approx_size_bytes)}${
        m.downloaded ? "" : ` · ${t("settings.meetings.notDownloaded")}`
      }`,
    })),
  );
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    <SettingsGroup title={t("settings.dictation.how")}>
      <SettingsRow
        label={t("settings.dictation.mode")}
        hint={t("settings.dictation.modeHint")}
      >
        {#snippet control()}
          <SegmentedControl
            value={cfg.dictation_mode}
            label={t("settings.dictation.modeAria")}
            options={[
              { value: "toggle", label: t("settings.dictation.toggle") },
              { value: "push_to_talk", label: t("settings.dictation.hold") },
            ]}
            onchange={(v) => patch({ dictation_mode: v })}
            full
          />
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    <SettingsGroup
      title={t("settings.dictation.engine")}
      hint={t("settings.dictation.engineHint")}
    >
      <SettingsRow label={t("settings.dictation.where")}>
        {#snippet control()}
          <SegmentedControl
            value={cfg.dictation_backend}
            label={t("settings.dictation.whereAria")}
            options={[
              { value: "groq", label: t("settings.meetings.groq") },
              { value: "local", label: t("settings.meetings.local") },
            ]}
            onchange={(v) =>
              patch({
                dictation_backend: v,
                live_engine: v === "groq" ? "groq" : "local",
              })}
            full
          />
        {/snippet}
      </SettingsRow>

      {#if cfg.dictation_backend !== "groq"}
        <SettingsRow
          label={t("settings.dictation.model")}
          hint={t("settings.dictation.modelHint")}
        >
          {#snippet control({ id })}
            <Select
              {id}
              value={cfg.dictation_whisper_model}
              options={modelOptions}
              onchange={(e: Event) =>
                patch({
                  dictation_whisper_model: (e.currentTarget as HTMLSelectElement).value,
                })}
            />
          {/snippet}
        </SettingsRow>
      {/if}
    </SettingsGroup>

    {#if cfg.dictation_backend === "groq"}
      <GroqKeyField missingHint={t("settings.dictation.groqKeyHint")} />
    {/if}
  </div>
{/if}
