<script lang="ts">
  /**
   * Pista a escuchar y barra de reproducción.
   *
   * «Todos» mezcla mic y sistema. «Yo» / «Otros» aíslan una. El cabezal es el
   * del controlador global, el mismo que usa la transcripción.
   */
  import type { Recording } from "$core/types";
  import {
    defaultTrack,
    listenOptions,
    playback,
    type AudioTrack,
  } from "$domain/playback.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import AudioPlayer from "./AudioPlayer.svelte";
  import { t } from "$domain/i18n.svelte";

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

  function onTrack(next: AudioTrack) {
    void playback.switchTrack(recording, next);
  }
</script>

<div class="flex flex-col gap-2">
  {#if options.length > 1}
    <SegmentedControl
      value={track}
      {options}
      size="sm"
      label={t("page.meetings.track")}
      onchange={onTrack}
    />
  {/if}
  <AudioPlayer
    alwaysVisible
    dismissible={false}
    placeholder={t("page.meetings.playThis")}
    onEmptyPlay={() => playback.play(recording, track)}
  />
</div>
