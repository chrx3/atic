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

  const cfg = $derived(config.current);

  function patch(changes: Parameters<typeof config.patch>[0]) {
    void config.patch(changes).catch(toastError);
  }

  /** El tamaño va en la etiqueta: es el dato que decide cuál elegir. */
  const modelOptions = $derived(
    models.items.map((m) => ({
      value: m.id,
      label: `${m.display_name} · ${formatMegabytes(m.approx_size_bytes)}${
        m.downloaded ? "" : " · sin descargar"
      }`,
    })),
  );

  const chosen = $derived(models.items.find((m) => m.id === cfg?.whisper_model));
  const groq = $derived(cfg?.meeting_backend === "groq");
  const groqOptions = GROQ_WHISPER_MODELS.map((m) => ({
    value: m.value,
    label: m.label,
  }));
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    {#if !groq && chosen && !chosen.downloaded}
      <Banner tone="warn" title="El modelo elegido no está descargado">
        {#snippet action()}
          <Button
            variant="soft"
            size="sm"
            loading={models.downloading !== null}
            onclick={() => void models.download(chosen.id).catch(toastError)}
          >
            Descargar
          </Button>
        {/snippet}
        Sin él no se puede transcribir.
      </Banner>
    {/if}

    {#if models.downloading}
      <ProgressBar
        value={models.downloading.downloaded / Math.max(models.downloading.total, 1)}
        label="Descargando"
      />
    {/if}

    <SettingsGroup
      title="Transcripción"
      hint="Local no sale de la máquina. Groq es más rápido y manda el audio a su API."
    >
      <SettingsRow label="Dónde transcribe">
        {#snippet control()}
          <SegmentedControl
            value={cfg.meeting_backend === "groq" ? "groq" : "local"}
            label="Motor de transcripción de reuniones"
            options={[
              { value: "local", label: "Local" },
              { value: "groq", label: "Groq" },
            ]}
            onchange={(v) => patch({ meeting_backend: v })}
            full
          />
        {/snippet}
      </SettingsRow>

      {#if groq}
        <SettingsRow
          label="Modelo Groq"
          hint="Turbo llega antes; Large v3 acierta un poco más."
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
        <SettingsRow label="Modelo" hint="Más grande transcribe mejor y tarda más.">
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
            label="Transcribir al terminar de grabar"
            hint={groq
              ? "Al terminar se envía el audio a Groq."
              : "Corre local. En una reunión larga puede tardar."}
            onchange={(v) => patch({ auto_transcribe_after_recording: v })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={cfg.live_transcription}
            label="Vista en vivo mientras grabás"
            hint="Experimental. Consume bastante más CPU."
            onchange={(v) => patch({ live_transcription: v })}
          />
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    {#if groq}
      <GroqKeyField missingHint="Sin ella no se puede transcribir la reunión en Groq." />
    {/if}

    <SettingsGroup title="Qué se graba">
      <SettingsRow
        label="Pistas"
        hint="El micrófono, lo que suena en el PC, o las dos."
      >
        {#snippet control()}
          <SegmentedControl
            value={cfg.record_tracks}
            label="Pistas a grabar"
            options={[
              { value: "both", label: "Ambas" },
              { value: "mic", label: "Mic" },
              { value: "system", label: "PC" },
            ]}
            onchange={(v) => patch({ record_tracks: v })}
            full
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow
        label="En disco"
        hint="WAV y transcripciones. Para una reunión concreta, usá Carpeta en Reuniones."
      >
        {#snippet control()}
          <Button
            variant="soft"
            size="sm"
            full
            onclick={() => void openDataDir("recordings").catch(toastError)}
          >
            Abrir carpeta
          </Button>
        {/snippet}
      </SettingsRow>
    </SettingsGroup>
  </div>
{/if}
