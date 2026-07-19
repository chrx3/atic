<script lang="ts">
  import { onMount, untrack } from "svelte";
  import type { AppConfig, ModelStatus } from "$lib/types";
  import ModalShell from "$lib/ModalShell.svelte";
  import { formatMegabytes } from "$lib/format";
  import {
    downloadModelAndWait,
    listModels,
    onModelDownloadProgress,
  } from "$lib/api";

  let {
    config,
    onDone,
  }: {
    config: AppConfig;
    onDone: (cfg: AppConfig) => void | Promise<void>;
  } = $props();

  let step = $state(0);
  let busy = $state(false);
  let models = $state<ModelStatus[]>([]);
  let downloadingId = $state<string | null>(null);
  let downloadPct = $state(0);
  let downloadError = $state<string | null>(null);

  const initialConfig = untrack(() => config);
  let draft = $state({
    language: initialConfig.language,
    speakers_mode: initialConfig.speakers_mode,
    beep_on_start: initialConfig.beep_on_start,
    autostart: initialConfig.autostart,
    whisper_model: initialConfig.whisper_model || "base",
    dictation_whisper_model: initialConfig.dictation_whisper_model || "base",
  });

  const steps = ["Bienvenida", "Consentimiento", "Preferencias", "Modelos"];

  /** Modelos recomendados del onboarding (dictado + reuniones). */
  const recommendedIds = $derived([
    draft.dictation_whisper_model,
    draft.whisper_model,
  ].filter((id, i, arr) => arr.indexOf(id) === i));

  const recommendedModels = $derived(
    recommendedIds
      .map((id) => models.find((m) => m.id === id))
      .filter((m): m is ModelStatus => Boolean(m)),
  );

  function modelUseLabel(id: string): string {
    const uses: string[] = [];
    if (id === draft.dictation_whisper_model) uses.push("Dictado");
    if (id === draft.whisper_model) uses.push("Reuniones");
    return uses.join(" y ");
  }

  const totalBytes = $derived(
    recommendedModels.reduce((sum, m) => sum + (m.downloaded ? 0 : m.approx_size_bytes), 0),
  );

  const allReady = $derived(
    recommendedModels.length > 0 && recommendedModels.every((m) => m.downloaded),
  );

  async function refreshModels() {
    models = await listModels();
  }

  async function downloadMissing() {
    if (busy || downloadingId) return;
    downloadError = null;
    const missing = recommendedModels.filter((m) => !m.downloaded);
    for (const m of missing) {
      downloadingId = m.id;
      downloadPct = 0;
      try {
        await downloadModelAndWait(m.id);
        await refreshModels();
      } catch (e) {
        downloadError = String(e);
        downloadingId = null;
        return;
      }
    }
    downloadingId = null;
    downloadPct = 0;
  }

  async function finish(skipDownload: boolean) {
    if (busy) return;
    if (!skipDownload && !allReady) {
      await downloadMissing();
      await refreshModels();
      const ready = recommendedIds.every(
        (id) => models.find((m) => m.id === id)?.downloaded,
      );
      if (!ready) return;
    }
    busy = true;
    try {
      await onDone({
        ...config,
        language: draft.language,
        speakers_mode: draft.speakers_mode,
        beep_on_start: draft.beep_on_start,
        autostart: draft.autostart,
        whisper_model: draft.whisper_model,
        dictation_whisper_model: draft.dictation_whisper_model,
        dictation_backend: config.dictation_backend || "groq",
        onboarding_done: true,
      });
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    refreshModels().catch(() => {
      models = [];
    });

    const unProgress = onModelDownloadProgress((p) => {
      if (p.id === downloadingId) {
        downloadPct =
          p.total > 0 ? Math.round((p.downloaded / p.total) * 100) : 0;
      }
    });

    return () => {
      void unProgress.then((fn) => fn());
    };
  });
</script>

<ModalShell
  title="Atic"
  subtitle={`Primer uso · ${steps[step]}`}
  size="md"
  dismissible={false}
  onClose={() => {}}
>
    <div class="space-y-4 text-sm" style="color: var(--rb-text)">
      {#if step === 0}
        <p class="leading-relaxed" style="color: var(--rb-muted)">
          Graba el audio de tus reuniones, transcribe en local y genera
          resúmenes con la IA que tú configures (BYOK).
        </p>
        <ul class="space-y-2" style="color: var(--rb-muted)">
          <li class="flex gap-2">
            <span class="rb-chip rb-chip-ok shrink-0">1</span>
            Nunca graba sola: siempre decides tú.
          </li>
          <li class="flex gap-2">
            <span class="rb-chip rb-chip-ok shrink-0">2</span>
            El audio no sale del PC al transcribir.
          </li>
          <li class="flex gap-2">
            <span class="rb-chip rb-chip-ok shrink-0">3</span>
            Puedes eliminar cualquier grabación cuando quieras.
          </li>
        </ul>
      {:else if step === 1}
        <p class="font-medium">Consentimiento</p>
        <p class="leading-relaxed" style="color: var(--rb-muted)">
          En muchas jurisdicciones (incluida Chile) grabar una llamada requiere
          el consentimiento de los participantes. Usa Atic solo cuando
          esté permitido y, si hace falta, avisa a los demás.
        </p>
        <label class="rb-check">
          <input type="checkbox" bind:checked={draft.beep_on_start} />
          <span>Beep al iniciar la grabación (aviso audible).</span>
        </label>
      {:else if step === 2}
        <label class="rb-label">
          Idioma de transcripción
          <select class="rb-field" bind:value={draft.language}>
            <option value="es">Español (recomendado)</option>
            <option value="auto">Autodetectar (puede fallar con ruido)</option>
            <option value="en">Inglés</option>
            <option value="pt">Portugués</option>
          </select>
        </label>
        <label class="rb-check">
          <input type="checkbox" bind:checked={draft.speakers_mode} />
          <span>
            <span class="font-medium">Modo parlantes</span>
            <span class="rb-hint mt-0.5 block"
              >Sin auriculares: graba solo «otros» para evitar eco.</span
            >
          </span>
        </label>
        <label class="rb-check">
          <input type="checkbox" bind:checked={draft.autostart} />
          Iniciar con el sistema (bandeja)
        </label>
      {:else}
        <p class="font-medium">Modelos locales</p>
        <p class="leading-relaxed" style="color: var(--rb-muted)">
          La transcripción corre en tu PC. Por defecto se descarga un único
          modelo rápido que sirve para dictado y reuniones.
        </p>

        <ul class="space-y-3">
          {#each recommendedModels as m (m.id)}
            <li
              class="flex items-start justify-between gap-3 rounded-lg border px-3 py-2.5"
              style="border-color: var(--rb-border)"
            >
              <div class="min-w-0">
                <p class="font-medium">
                  {modelUseLabel(m.id)}
                </p>
                <p class="rb-hint">{m.display_name}</p>
              </div>
              {#if m.downloaded}
                <span class="rb-chip rb-chip-ok shrink-0">Listo</span>
              {:else if downloadingId === m.id}
                <span class="rb-chip rb-chip-warn shrink-0"
                  >{downloadPct}%</span
                >
              {:else}
                <span class="rb-hint shrink-0"
                  >{formatMegabytes(m.approx_size_bytes)}</span
                >
              {/if}
            </li>
          {/each}
        </ul>

        {#if downloadingId}
          <div class="rb-level-track">
            <div
              class="rb-level-fill rb-level-mic"
              style="width: {downloadPct}%"
            ></div>
          </div>
        {/if}

        {#if downloadError}
          <p class="text-sm" style="color: var(--rb-warn)">{downloadError}</p>
        {/if}

        {#if !allReady && totalBytes > 0}
          <p class="rb-hint">
            Descarga pendiente: ~{formatMegabytes(totalBytes)}. Puedes
            continuar y descargar después desde Ajustes.
          </p>
        {/if}
      {/if}
    </div>

    {#snippet actions()}
      <div class="flex w-full items-center justify-between gap-4">
        <div class="flex gap-1.5" aria-label={`Paso ${step + 1} de ${steps.length}`}>
          {#each steps as _, i (i)}
            <span
              class="h-1.5 w-7 rounded-full transition-colors"
              style="background: {i === step
                ? 'var(--rb-record)'
                : i < step
                  ? 'var(--rb-accent)'
                  : 'var(--rb-border-strong)'}"
            ></span>
          {/each}
        </div>
        <div class="flex gap-2">
          {#if step > 0}
            <button
              class="rb-btn rb-btn-ghost"
              onclick={() => (step -= 1)}
              disabled={Boolean(downloadingId) || busy}
              >Atrás</button
            >
          {/if}
          {#if step < steps.length - 1}
            <button class="rb-btn rb-btn-primary" onclick={() => (step += 1)}
              >Siguiente</button
            >
          {:else if allReady}
            <button
              class="rb-btn rb-btn-primary"
              onclick={() => finish(true)}
              disabled={busy}
            >
              {busy ? "Guardando…" : "Empezar"}
            </button>
          {:else}
            <button
              class="rb-btn rb-btn-ghost"
              onclick={() => finish(true)}
              disabled={busy || Boolean(downloadingId)}
            >
              Más tarde
            </button>
            <button
              class="rb-btn rb-btn-primary"
              onclick={() => finish(false)}
              disabled={busy || Boolean(downloadingId)}
            >
              {downloadingId
                ? `Descargando… ${downloadPct}%`
                : busy
                  ? "Guardando…"
                  : "Descargar y empezar"}
            </button>
          {/if}
        </div>
      </div>
    {/snippet}
</ModalShell>
