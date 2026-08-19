<script lang="ts">
  /** Motor y modelo de transcripción de reuniones, sin ir a Ajustes. */
  import { formatMegabytes } from "$core/format";
  import { GROQ_WHISPER_MODELS } from "$core/groqWhisper";
  import { config } from "$domain/config.svelte";
  import { models } from "$domain/models.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Select from "$ui/Select.svelte";

  let { disabled = false }: { disabled?: boolean } = $props();

  const cfg = $derived(config.current);
  const backend = $derived(cfg?.meeting_backend === "groq" ? "groq" : "local");

  const localOptions = $derived(
    models.items.map((m) => ({
      value: m.id,
      label: `${m.display_name} · ${formatMegabytes(m.approx_size_bytes)}${
        m.downloaded ? "" : " · sin descargar"
      }`,
    })),
  );

  const groqOptions = GROQ_WHISPER_MODELS.map((m) => ({
    value: m.value,
    label: m.label,
  }));

  function setBackend(value: string) {
    void config.patch({ meeting_backend: value }).catch(toastError);
  }

  function onLocalModel(event: Event) {
    const id = (event.currentTarget as HTMLSelectElement).value;
    void config.patch({ whisper_model: id }).catch(toastError);
  }

  function onGroqModel(event: Event) {
    const id = (event.currentTarget as HTMLSelectElement).value;
    void config.patch({ meeting_groq_model: id }).catch(toastError);
  }
</script>

<div class="flex flex-col gap-1">
  <SegmentedControl
    value={backend}
    label="Motor de transcripción"
    options={[
      { value: "local", label: "Local", disabled },
      { value: "groq", label: "Groq", disabled },
    ]}
    onchange={setBackend}
    size="sm"
    full
  />
  {#if backend === "groq"}
    <Select
      value={cfg?.meeting_groq_model ?? "whisper-large-v3-turbo"}
      options={groqOptions}
      {disabled}
      onchange={onGroqModel}
      aria-label="Modelo Groq"
    />
  {:else}
    <Select
      value={cfg?.whisper_model ?? "base"}
      options={localOptions}
      {disabled}
      onchange={onLocalModel}
      aria-label="Modelo de transcripción"
    />
  {/if}
</div>
