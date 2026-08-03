<script lang="ts">
  /**
   * El primer uso: consentimiento, preferencias, modelos y cómo se usa.
   *
   * No se puede cerrar. Es lo único de la app que bloquea, y bloquea porque el
   * paso de consentimiento no es decorativo: en muchas jurisdicciones grabar
   * una llamada sin avisar es ilegal, y Atic no puede dar por leído algo que el
   * usuario nunca vio.
   *
   * El tutorial va ÚLTIMO a propósito. Antes esto terminaba en «Modelos»:
   * salías configurado pero sin que nadie te hubiera mostrado la pill ni un
   * solo atajo, y toda la interfaz real de Atic vive fuera de esta ventana.
   */
  import { formatMegabytes } from "$core/format";
  import type { ModelStatus } from "$core/types";
  import { config } from "$domain/config.svelte";
  import { models } from "$domain/models.svelte";
  import { downloadModelAndWait } from "$ipc/models";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";
  import Field from "$ui/Field.svelte";
  import Modal from "$ui/Modal.svelte";
  import ProgressBar from "$ui/ProgressBar.svelte";
  import Select from "$ui/Select.svelte";
  import Switch from "$ui/Switch.svelte";
  import UsageGuide from "./UsageGuide.svelte";

  let { onDone }: { onDone: () => void } = $props();

  const STEPS = [
    "Bienvenida",
    "Consentimiento",
    "Preferencias",
    "Modelos",
    "Cómo se usa",
  ];

  /** Índice del paso de modelos; el tutorial va justo después. */
  const MODELS_STEP = 3;

  const cfg = $derived(config.current);

  let step = $state(0);
  let saving = $state(false);
  let downloadingId = $state<string | null>(null);
  let downloadError = $state<string | null>(null);

  /**
   * Los dos modelos que el onboarding ofrece: el de dictado y el de reuniones.
   * Pueden ser el mismo, y entonces se muestra una sola fila.
   */
  const recommended = $derived.by(() => {
    if (!cfg) return [] as ModelStatus[];
    const ids = [cfg.dictation_whisper_model, cfg.whisper_model].filter(
      (id, i, all) => all.indexOf(id) === i,
    );
    return ids
      .map((id) => models.items.find((m) => m.id === id))
      .filter((m): m is ModelStatus => Boolean(m));
  });

  const allReady = $derived(
    recommended.length > 0 && recommended.every((m) => m.downloaded),
  );

  const pendingBytes = $derived(
    recommended.reduce((sum, m) => sum + (m.downloaded ? 0 : m.approx_size_bytes), 0),
  );

  function useLabel(id: string): string {
    const uses: string[] = [];
    if (id === cfg?.dictation_whisper_model) uses.push("Dictado");
    if (id === cfg?.whisper_model) uses.push("Reuniones");
    return uses.join(" y ");
  }

  function patch(changes: Parameters<typeof config.patch>[0]) {
    // Se guarda paso a paso en vez de al final: si la app se cierra a mitad
    // del onboarding, lo elegido hasta ahí no se pierde. Lo único que espera
    // hasta el final es `onboarding_done`.
    void config.patch(changes).catch(() => {});
  }

  async function downloadMissing(): Promise<boolean> {
    downloadError = null;
    for (const model of recommended.filter((m) => !m.downloaded)) {
      downloadingId = model.id;
      try {
        await downloadModelAndWait(model.id);
        await models.hydrate();
      } catch (error) {
        downloadError = String(error);
        downloadingId = null;
        return false;
      }
    }
    downloadingId = null;
    return true;
  }

  /**
   * Sale del paso de modelos hacia el tutorial, descargando si se pidió.
   *
   * La descarga ya no termina el onboarding: bajar un modelo y quedar frente a
   * una app cuyos atajos nadie explicó era justamente el problema.
   */
  async function leaveModels(download: boolean) {
    if (downloadingId) return;
    // Si la descarga falló, quedarse acá: el error ya está en pantalla.
    if (download && !allReady && !(await downloadMissing())) return;
    step += 1;
  }

  async function finish() {
    saving = true;
    try {
      await config.patch({ onboarding_done: true });
      onDone();
    } catch {
      // Sin poder marcarlo, el onboarding volvería a salir al reabrir. Es
      // molesto pero no destructivo, así que igual se deja pasar.
      onDone();
    } finally {
      saving = false;
    }
  }
</script>

{#if cfg}
  <Modal
    title="Atic"
    subtitle="Primer uso · {STEPS[step]}"
    size="md"
    dismissible={false}
    onClose={() => {}}
  >
    <div class="flex flex-col gap-4">
      {#if step === 0}
        <p class="max-w-[60ch] text-sm leading-relaxed text-muted">
          Grabá el audio de tus reuniones, transcribí en local y generá resúmenes con la
          IA que vos configures.
        </p>
        <ul class="flex list-none flex-col gap-2">
          {#each ["Nunca graba sola: siempre decidís vos.", "El audio no sale del PC al transcribir.", "Podés borrar cualquier grabación cuando quieras."] as claim, i (i)}
            <li class="flex items-baseline gap-2 text-sm text-muted">
              <Chip tone="ok">{i + 1}</Chip>
              {claim}
            </li>
          {/each}
        </ul>
      {:else if step === 1}
        <p class="max-w-[60ch] text-sm leading-relaxed text-muted">
          En muchas jurisdicciones —Chile incluido— grabar una llamada requiere el
          consentimiento de los participantes. Usá Atic solo cuando esté permitido y, si
          hace falta, avisale a los demás.
        </p>
        <Switch
          checked={cfg.beep_on_start}
          label="Beep al empezar a grabar"
          hint="Un aviso audible para los demás."
          onchange={(checked) => patch({ beep_on_start: checked })}
        />
      {:else if step === 2}
        <Field label="Idioma de transcripción">
          {#snippet children({ id })}
            <Select
              {id}
              value={cfg.language}
              options={[
                { value: "es", label: "Español (recomendado)" },
                { value: "auto", label: "Autodetectar (puede fallar con ruido)" },
                { value: "en", label: "Inglés" },
                { value: "pt", label: "Portugués" },
              ]}
              onchange={(event: Event) =>
                patch({ language: (event.currentTarget as HTMLSelectElement).value })}
            />
          {/snippet}
        </Field>

        <Switch
          checked={cfg.speakers_mode}
          label="Modo parlantes"
          hint="Sin auriculares: graba solo «otros» para evitar el eco."
          onchange={(checked) => patch({ speakers_mode: checked })}
        />

        <Switch
          checked={cfg.autostart}
          label="Arrancar con el sistema"
          hint="Queda en la bandeja, sin abrir esta ventana."
          onchange={(checked) => patch({ autostart: checked })}
        />

        <p class="max-w-[60ch] text-xs leading-relaxed text-faint">
          Atic vive en la bandeja del sistema: cerrar la ventana con la X la esconde, no
          cierra la app ni corta una grabación en curso. Para salir del todo, usá
          «Salir» en el menú de la bandeja.
        </p>
      {:else if step === MODELS_STEP}
        <p class="max-w-[60ch] text-sm leading-relaxed text-muted">
          La transcripción corre en tu PC. Por defecto se baja un solo modelo rápido que
          sirve para dictado y para reuniones.
        </p>

        <ul class="flex list-none flex-col gap-1.5">
          {#each recommended as model (model.id)}
            <li
              class="flex items-center justify-between gap-3 rounded-sm border border-line
                     px-3 py-2"
            >
              <div class="flex min-w-0 flex-col gap-0.5">
                <span class="text-sm font-medium text-text">{useLabel(model.id)}</span>
                <span class="truncate text-xs text-faint">{model.display_name}</span>
              </div>
              {#if model.downloaded}
                <Chip tone="ok">Listo</Chip>
              {:else if downloadingId === model.id}
                <Chip tone="warn">Bajando</Chip>
              {:else}
                <span class="font-mono text-xs text-faint" data-numeric>
                  {formatMegabytes(model.approx_size_bytes)}
                </span>
              {/if}
            </li>
          {/each}
        </ul>

        {#if downloadingId}
          <ProgressBar
            value={models.percent / 100}
            label="Descargando el modelo"
            tone="ok"
          />
        {/if}

        {#if downloadError}
          <Banner tone="danger" title="No se pudo bajar el modelo">
            {downloadError}
          </Banner>
        {:else if !allReady && pendingBytes > 0}
          <p class="text-xs text-faint">
            Falta bajar ~{formatMegabytes(pendingBytes)}. Podés seguir y hacerlo después
            desde Ajustes.
          </p>
        {/if}
      {:else}
        <UsageGuide />
      {/if}
    </div>

    {#snippet actions()}
      <div class="flex w-full items-center justify-between gap-4">
        <div class="flex gap-1" aria-label="Paso {step + 1} de {STEPS.length}">
          {#each STEPS as _, i (i)}
            <span
              class="h-1 w-6 rounded-pill transition-colors duration-(--duration-quick)
                     {i === step
                ? 'bg-accent'
                : i < step
                  ? 'bg-muted'
                  : 'bg-line-strong'}"
            ></span>
          {/each}
        </div>

        <div class="flex gap-2">
          {#if step > 0}
            <Button
              variant="ghost"
              disabled={Boolean(downloadingId) || saving}
              onclick={() => (step -= 1)}
            >
              Atrás
            </Button>
          {/if}

          {#if step < MODELS_STEP}
            <Button variant="primary" onclick={() => (step += 1)}>Siguiente</Button>
          {:else if step === MODELS_STEP}
            {#if allReady}
              <Button variant="primary" onclick={() => void leaveModels(false)}>
                Siguiente
              </Button>
            {:else}
              <Button
                variant="ghost"
                disabled={Boolean(downloadingId)}
                onclick={() => void leaveModels(false)}
              >
                Más tarde
              </Button>
              <Button
                variant="primary"
                loading={Boolean(downloadingId)}
                onclick={() => void leaveModels(true)}
              >
                Descargar y seguir
              </Button>
            {/if}
          {:else}
            <Button variant="primary" loading={saving} onclick={() => void finish()}>
              Empezar
            </Button>
          {/if}
        </div>
      </div>
    {/snippet}
  </Modal>
{/if}
