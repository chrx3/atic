<script lang="ts">
  /** Motor y modelo de transcripción de reuniones, sin ir a Ajustes. */
  import { formatMegabytes } from "$core/format";
  import { GROQ_WHISPER_MODELS } from "$core/groqWhisper";
  import { config } from "$domain/config.svelte";
  import { models } from "$domain/models.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Select from "$ui/Select.svelte";
  import { t, whisperModelLabel } from "$domain/i18n.svelte";

  let { disabled = false }: { disabled?: boolean } = $props();

  const cfg = $derived(config.current);
  const backend = $derived(cfg?.meeting_backend === "groq" ? "groq" : "local");

  const localOptions = $derived(
    models.items.map((m) => ({
      value: m.id,
      label: `${whisperModelLabel(m.id)} · ${formatMegabytes(m.approx_size_bytes)}${
        m.downloaded ? "" : ` · ${t("settings.meetings.notDownloaded")}`
      }`,
    })),
  );

  const groqOptions = $derived(
    GROQ_WHISPER_MODELS.map((m) => ({
      value: m.value,
      label: t(`models.groq.${m.value}`),
    })),
  );

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

  const engineOptions = $derived([
    { value: "local", label: t("settings.meetings.local"), disabled },
    { value: "groq", label: t("settings.meetings.groq"), disabled },
  ]);
</script>

<div class="flex flex-col gap-1">
  <SegmentedControl
    value={backend}
    label={t("settings.meetings.whereAria")}
    options={engineOptions}
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
      aria-label={t("page.meetings.groqModel")}
    />
  {:else}
    <Select
      value={cfg?.whisper_model ?? "base"}
      options={localOptions}
      {disabled}
      onchange={onLocalModel}
      aria-label={t("page.meetings.localModel")}
    />
  {/if}
</div>
