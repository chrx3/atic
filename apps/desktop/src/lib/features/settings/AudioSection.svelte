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
      { value: "", label: "El del sistema" },
      ...devices.map((d) => ({
        value: d.id,
        // Los que cpal no supo describir pueden fallar al abrirse: mejor
        // saberlo antes de elegirlo que a mitad de una reunión.
        label: d.may_not_open ? `${d.name} · puede fallar` : d.name,
      })),
    ];
  }
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    {#if preflight?.risk === "bluetooth_hands_free" && preflight.message}
      <Banner tone="warn" title={preflight.message}>
        Los auriculares Bluetooth bajan la calidad del audio al usar su micrófono.
        Conviene grabar con otro.
      </Banner>
    {/if}

    <SettingsGroup title="Dispositivos">
      <SettingsRow label="Micrófono" hint="Para grabar reuniones.">
        {#snippet control({ id })}
          <Select
            {id}
            value={cfg.mic_device_id}
            options={options(mics)}
            disabled={loading}
            onchange={(e: Event) =>
              patch({ mic_device_id: (e.currentTarget as HTMLSelectElement).value })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow label="Micrófono de dictado" hint="Vacío reutiliza el de arriba.">
        {#snippet control({ id })}
          <Select
            {id}
            value={cfg.dictation_mic_device_id}
            options={options(mics)}
            disabled={loading}
            onchange={(e: Event) =>
              patch({
                dictation_mic_device_id: (e.currentTarget as HTMLSelectElement).value,
              })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow label="Salida" hint="De dónde se toma el audio del PC.">
        {#snippet control({ id })}
          <Select
            {id}
            value={cfg.output_device_id}
            options={options(outputs)}
            disabled={loading}
            onchange={(e: Event) =>
              patch({ output_device_id: (e.currentTarget as HTMLSelectElement).value })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow label="Volver a mirar" hint="Si enchufaste algo recién.">
        {#snippet control()}
          <Button variant="soft" size="sm" full {loading} onclick={() => void load()}>
            Recargar
          </Button>
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    <SettingsGroup title="Tratamiento">
      <SettingsRow label="Supresión de ruido" hint="Sobre el micrófono.">
        {#snippet control()}
          <SegmentedControl
            value={cfg.noise_suppression || "off"}
            label="Supresión de ruido"
            options={[
              { value: "off", label: "No" },
              { value: "low", label: "Baja" },
              { value: "medium", label: "Media" },
              { value: "high", label: "Alta" },
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
            label="Estoy con parlantes"
            hint="Prioriza el audio del PC para que el micrófono no capte el eco."
            onchange={(v) => patch({ speakers_mode: v })}
          />
        {/snippet}
      </SettingsRow>
    </SettingsGroup>
  </div>
{/if}
