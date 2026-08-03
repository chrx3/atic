<script lang="ts">
  /** Qué se graba, con qué modelo y qué pasa al terminar. */
  import { formatMegabytes } from "$core/format";
  import { config } from "$domain/config.svelte";
  import { models } from "$domain/models.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import ProgressBar from "$ui/ProgressBar.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Select from "$ui/Select.svelte";
  import Switch from "$ui/Switch.svelte";

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
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    {#if chosen && !chosen.downloaded}
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

    <SettingsGroup title="Transcripción">
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

      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={cfg.auto_transcribe_after_recording}
            label="Transcribir al terminar de grabar"
            hint="Corre local. En una reunión larga puede tardar."
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

      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={cfg.detect_meetings}
            label="Avisar cuando detecte una reunión"
            hint="Mira si hay una llamada abierta; no lee su contenido."
            onchange={(v) => patch({ detect_meetings: v })}
          />
        {/snippet}
      </SettingsRow>
    </SettingsGroup>
  </div>
{/if}
