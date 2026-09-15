<script lang="ts">
  /**
   * Reproductor de la grabación: la barra, y la pista cuando hay más de una.
   *
   * La pista es un `<select>` chico en la misma fila y no un segmentado de
   * tres tercios: elegir pista es secundario, y en macOS —donde no hay loopback
   * del sistema— solo existe una. El cabezal es el del controlador global, el
   * mismo que usa la transcripción.
   */
  import type { Recording } from "$core/types";
  import { t } from "$domain/i18n.svelte";
  import {
    defaultTrack,
    listenOptions,
    playback,
    type AudioTrack,
  } from "$domain/playback.svelte";
  import Select from "$ui/Select.svelte";
  import AudioPlayer from "./AudioPlayer.svelte";

  let { recording }: { recording: Recording } = $props();

  const options = $derived(
    listenOptions(recording).map((option) => ({
      ...option,
      label:
        option.value === "mix"
          ? t("page.meetings.all")
          : option.value === "mic"
            ? t("page.meetings.me")
            : t("page.meetings.others"),
    })),
  );
  const track = $derived(
    playback.recordingId === recording.id && playback.track
      ? playback.track
      : defaultTrack(recording),
  );

  function onTrack(event: Event) {
    const next = (event.currentTarget as HTMLSelectElement).value as AudioTrack;
    void playback.switchTrack(recording, next);
  }
</script>

<div class="flex items-center gap-2 rounded-md bg-surface-2 p-2">
  <AudioPlayer
    alwaysVisible
    dismissible={false}
    placeholder={t("page.meetings.playThis")}
    onEmptyPlay={() => playback.play(recording, track)}
  />

  {#if options.length > 1}
    <div class="w-24 shrink-0">
      <Select
        value={track}
        {options}
        onchange={onTrack}
        aria-label={t("page.meetings.track")}
      />
    </div>
  {/if}
</div>
