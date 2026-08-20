<script lang="ts">
  /** Qué se graba, con qué modelo y qué pasa al terminar. */
  import { formatMegabytes } from "$core/format";
  import { GROQ_WHISPER_MODELS } from "$core/groqWhisper";
  import { config } from "$domain/config.svelte";
  import { models } from "$domain/models.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { openDataDir } from "$ipc/config";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import ProgressBar from "$ui/ProgressBar.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Select from "$ui/Select.svelte";
  import Switch from "$ui/Switch.svelte";
  import GroqKeyField from "./GroqKeyField.svelte";
  import { t, whisperModelLabel } from "$domain/i18n.svelte";

  const cfg = $derived(config.current);

  function patch(changes: Parameters<typeof config.patch>[0]) {
    void config.patch(changes).catch(toastError);
  }

  /** El tamaño va en la etiqueta: es el dato que decide cuál elegir. */
  const modelOptions = $derived(
    models.items.map((m) => ({
      value: m.id,
      label: `${whisperModelLabel(m.id)} · ${formatMegabytes(m.approx_size_bytes)}${
        m.downloaded ? "" : ` · ${t("settings.meetings.notDownloaded")}`
      }`,
    })),
  );

  const chosen = $derived(models.items.find((m) => m.id === cfg?.whisper_model));
  const groq = $derived(cfg?.meeting_backend === "groq");
  const groqOptions = $derived(
    GROQ_WHISPER_MODELS.map((m) => ({
      value: m.value,
      label: t(`models.groq.${m.value}`),
    })),
  );
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    {#if !groq && chosen && !chosen.downloaded}
      <Banner tone="warn" title={t("settings.meetings.missingModel")}>
        {#snippet action()}
          <Button
            variant="soft"
            size="sm"
            loading={models.downloading !== null}
            onclick={() => void models.download(chosen.id).catch(toastError)}
          >
            {t("settings.meetings.download")}
          </Button>
        {/snippet}
        {t("settings.meetings.missingModelBody")}
      </Banner>
    {/if}

    {#if models.downloading}
      <ProgressBar
        value={models.downloading.downloaded / Math.max(models.downloading.total, 1)}
        label={t("settings.meetings.downloading")}
      />
    {/if}

    <SettingsGroup
      title={t("settings.meetings.transcription")}
      hint={t("settings.meetings.transcriptionHint")}
    >
      <SettingsRow label={t("settings.meetings.where")}>
        {#snippet control()}
          <SegmentedControl
            value={cfg.meeting_backend === "groq" ? "groq" : "local"}
            label={t("settings.meetings.whereAria")}
            options={[
              { value: "local", label: t("settings.meetings.local") },
              { value: "groq", label: t("settings.meetings.groq") },
            ]}
            onchange={(v) => patch({ meeting_backend: v })}
            full
          />
        {/snippet}
      </SettingsRow>

      {#if groq}
        <SettingsRow
          label={t("settings.meetings.groqModel")}
          hint={t("settings.meetings.groqModelHint")}
        >
          {#snippet control({ id })}
            <Select
              {id}
              value={cfg.meeting_groq_model}
              options={groqOptions}
              onchange={(e: Event) =>
                patch({
                  meeting_groq_model: (e.currentTarget as HTMLSelectElement).value,
                })}
            />
          {/snippet}
        </SettingsRow>
      {:else}
        <SettingsRow label={t("settings.meetings.model")} hint={t("settings.meetings.modelHint")}>
          {#snippet control({ id })}
            <Select
              {id}
              value={cfg.whisper_model}
              options={modelOptions}
              onchange={(e: Event) =>
                patch({ whisper_model: (e.currentTarget as HTMLSelectElement).value })}
            />
          {/snippet}
        </SettingsRow>
      {/if}

      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={cfg.auto_transcribe_after_recording}
            label={t("settings.meetings.autoTranscribe")}
            hint={groq
              ? t("settings.meetings.autoTranscribeGroq")
              : t("settings.meetings.autoTranscribeLocal")}
            onchange={(v) => patch({ auto_transcribe_after_recording: v })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={cfg.live_transcription}
            label={t("settings.meetings.live")}
            hint={t("settings.meetings.liveHint")}
            onchange={(v) => patch({ live_transcription: v })}
          />
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    {#if groq}
      <GroqKeyField missingHint={t("settings.meetings.groqKeyHint")} />
    {/if}

    <SettingsGroup title={t("settings.meetings.what")}>
      <SettingsRow
        label={t("settings.meetings.tracks")}
        hint={t("settings.meetings.tracksHint")}
      >
        {#snippet control()}
          <SegmentedControl
            value={cfg.record_tracks}
            label={t("settings.meetings.tracksAria")}
            options={[
              { value: "both", label: t("settings.meetings.both") },
              { value: "mic", label: t("settings.meetings.mic") },
              { value: "system", label: t("settings.meetings.pc") },
            ]}
            onchange={(v) => patch({ record_tracks: v })}
            full
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow
        label={t("settings.meetings.onDisk")}
        hint={t("settings.meetings.onDiskHint")}
      >
        {#snippet control()}
          <Button
            variant="soft"
            size="sm"
            full
            onclick={() => void openDataDir("recordings").catch(toastError)}
          >
            {t("settings.data.openFolder")}
          </Button>
        {/snippet}
      </SettingsRow>
    </SettingsGroup>
  </div>
{/if}
