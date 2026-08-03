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

  const cfg = $derived(config.current);

  function patch(changes: Parameters<typeof config.patch>[0]) {
    void config.patch(changes).catch(toastError);
  }
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    <SettingsGroup title="El shelf" hint="La tarjeta que aparece tras cada captura.">
      <SettingsRow label="De qué lado">
        {#snippet control()}
          <SegmentedControl
            value={cfg.capture_shelf_side}
            label="Lado del shelf"
            options={[
              { value: "left", label: "Izq." },
              { value: "right", label: "Der." },
            ]}
            onchange={(v) => patch({ capture_shelf_side: v })}
            full
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow label="Se va a los" hint="Segundos. 0 la deja hasta que la cierres.">
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

    <SettingsGroup title="La imagen">
      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={cfg.capture_include_cursor}
            label="Incluir el puntero"
            onchange={(v) => patch({ capture_include_cursor: v })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow label="Conservar" hint="En horas. 0 las guarda para siempre.">
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

      <SettingsRow label="Limpiar ahora" hint="Borra las que ya vencieron.">
        {#snippet control()}
          <Button
            variant="soft"
            size="sm"
            full
            onclick={() =>
              void captures
                .cleanup()
                .then((n) =>
                  toasts.push(n > 0 ? `Se borraron ${n}` : "No había nada vencido"),
                )
                .catch(toastError)}
          >
            Limpiar
          </Button>
        {/snippet}
      </SettingsRow>
    </SettingsGroup>
  </div>
{/if}
