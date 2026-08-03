<script lang="ts">
  /** Cómo se comporta el dictado. */
  import { config } from "$domain/config.svelte";
  import { models } from "$domain/models.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { formatMegabytes } from "$core/format";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Select from "$ui/Select.svelte";

  const cfg = $derived(config.current);

  function patch(changes: Parameters<typeof config.patch>[0]) {
    void config.patch(changes).catch(toastError);
  }

  const modelOptions = $derived(
    models.items.map((m) => ({
      value: m.id,
      label: `${m.display_name} · ${formatMegabytes(m.approx_size_bytes)}${
        m.downloaded ? "" : " · sin descargar"
      }`,
    })),
  );
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    <SettingsGroup title="Cómo se activa">
      <SettingsRow
        label="Modo"
        hint="Alternar es una tecla para empezar y otra para terminar; mantener dicta mientras la tengas apretada."
      >
        {#snippet control()}
          <SegmentedControl
            value={cfg.dictation_mode}
            label="Modo de dictado"
            options={[
              { value: "toggle", label: "Alternar" },
              { value: "push_to_talk", label: "Mantener" },
            ]}
            onchange={(v) => patch({ dictation_mode: v })}
            full
          />
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    <SettingsGroup
      title="Motor"
      hint="Local no sale de la máquina. Groq es más rápido pero manda el audio a su servidor."
    >
      <SettingsRow label="Dónde transcribe">
        {#snippet control()}
          <SegmentedControl
            value={cfg.dictation_backend}
            label="Motor de dictado"
            options={[
              { value: "local", label: "Local" },
              { value: "groq", label: "Groq" },
            ]}
            onchange={(v) => patch({ dictation_backend: v })}
            full
          />
        {/snippet}
      </SettingsRow>

      {#if cfg.dictation_backend !== "groq"}
        <SettingsRow
          label="Modelo"
          hint="Para dictado conviene uno chico: se nota la espera."
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
  </div>
{/if}
