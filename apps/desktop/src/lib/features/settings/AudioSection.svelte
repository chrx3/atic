<script lang="ts">
  /**
   * Micrófonos, salida y tratamiento del sonido.
   *
   * Los dispositivos se piden una vez al abrir y no se cachean en un store:
   * cambian cuando enchufás algo, y una lista vieja es peor que esperar un
   * momento. El botón de recargar existe justamente para eso.
   */
  import type { AudioPreflight, InputDeviceInfo } from "$core/types";
  import { config } from "$domain/config.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { audioPreflight, listInputDevices, listOutputDevices } from "$ipc/config";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Select from "$ui/Select.svelte";
  import Switch from "$ui/Switch.svelte";
  import { t } from "$domain/i18n.svelte";

  const cfg = $derived(config.current);

  let mics = $state<InputDeviceInfo[]>([]);
  let outputs = $state<InputDeviceInfo[]>([]);
  let preflight = $state<AudioPreflight | null>(null);
  let loading = $state(true);

  async function load() {
    loading = true;
    try {
      [mics, outputs, preflight] = await Promise.all([
        listInputDevices(),
        listOutputDevices(),
        audioPreflight(),
      ]);
    } catch (error) {
      toastError(error);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void load();
  });

  function patch(changes: Parameters<typeof config.patch>[0]) {
    void config.patch(changes).catch(toastError);
  }

  /** "" = el que decida el sistema. Es la opción sana por defecto. */
  function options(devices: InputDeviceInfo[]) {
    return [
      { value: "", label: t("settings.audio.systemDevice") },
      ...devices.map((d) => ({
        value: d.id,
        // Los que cpal no supo describir pueden fallar al abrirse: mejor
        // saberlo antes de elegirlo que a mitad de una reunión.
        label: d.may_not_open ? `${d.name} · ${t("settings.audio.mayFail")}` : d.name,
      })),
    ];
  }

  const micOptions = $derived(options(mics));
  const outputOptions = $derived(options(outputs));
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    {#if preflight?.risk === "bluetooth_hands_free" && preflight.message}
      <Banner tone="warn" title={preflight.message}>
        {t("settings.audio.bluetooth")}
      </Banner>
    {/if}

    <SettingsGroup title={t("settings.audio.devices")}>
      <SettingsRow label={t("settings.audio.mic")} hint={t("settings.audio.micHint")}>
        {#snippet control({ id })}
          <Select
            {id}
            value={cfg.mic_device_id}
            options={micOptions}
            disabled={loading}
            onchange={(e: Event) =>
              patch({ mic_device_id: (e.currentTarget as HTMLSelectElement).value })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow label={t("settings.audio.dictationMic")} hint={t("settings.audio.dictationMicHint")}>
        {#snippet control({ id })}
          <Select
            {id}
            value={cfg.dictation_mic_device_id}
            options={micOptions}
            disabled={loading}
            onchange={(e: Event) =>
              patch({
                dictation_mic_device_id: (e.currentTarget as HTMLSelectElement).value,
              })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow label={t("settings.audio.output")} hint={t("settings.audio.outputHint")}>
        {#snippet control({ id })}
          <Select
            {id}
            value={cfg.output_device_id}
            options={outputOptions}
            disabled={loading}
            onchange={(e: Event) =>
              patch({ output_device_id: (e.currentTarget as HTMLSelectElement).value })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow label={t("settings.audio.refresh")} hint={t("settings.audio.refreshHint")}>
        {#snippet control()}
          <Button variant="soft" size="sm" full {loading} onclick={() => void load()}>
            {t("settings.audio.refreshBtn")}
          </Button>
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    <SettingsGroup title={t("settings.audio.treatment")}>
      <SettingsRow label={t("settings.audio.noise")} hint={t("settings.audio.noiseHint")}>
        {#snippet control()}
          <SegmentedControl
            value={cfg.noise_suppression || "off"}
            label={t("settings.audio.noiseAria")}
            options={[
              { value: "off", label: t("settings.audio.off") },
              { value: "low", label: t("settings.audio.low") },
              { value: "medium", label: t("settings.audio.medium") },
              { value: "high", label: t("settings.audio.high") },
            ]}
            onchange={(v) => patch({ noise_suppression: v })}
            full
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={cfg.speakers_mode}
            label={t("settings.audio.speakers")}
            hint={t("settings.audio.speakersHint")}
            onchange={(v) => patch({ speakers_mode: v })}
          />
        {/snippet}
      </SettingsRow>
    </SettingsGroup>
  </div>
{/if}
