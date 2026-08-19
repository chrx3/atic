<script lang="ts">
  /**
   * El primer uso: consentimiento, Groq o local, modelos, atajos y práctica.
   *
   * No se puede cerrar. El consentimiento no es decorativo: en muchas
   * jurisdicciones grabar una llamada sin avisar es ilegal.
   *
   * La práctica no vive acá. Esta ventana atrapa el foco; los atajos reales
   * corren en el escritorio, junto a la pill. Al terminar el setup se cierra
   * el modal y el overlay toma el coach.
   */
  import { formatMegabytes } from "$core/format";
  import type { ModelStatus } from "$core/types";
  import { config } from "$domain/config.svelte";
  import { models } from "$domain/models.svelte";
  import { downloadModelAndWait } from "$ipc/models";
  import { minimizeWindow } from "$ipc/windows";
  import GroqKeyField from "$features/settings/GroqKeyField.svelte";
  import { SETUP_SHORTCUTS } from "./practice";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";
  import Field from "$ui/Field.svelte";
  import HotkeyCapture from "$ui/HotkeyCapture.svelte";
  import Modal from "$ui/Modal.svelte";
  import ProgressBar from "$ui/ProgressBar.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Select from "$ui/Select.svelte";
  import Switch from "$ui/Switch.svelte";

  let { onDone }: { onDone: () => void } = $props();

  const STEPS = [
    "Bienvenida",
    "Consentimiento",
    "Preferencias",
    "Dictado",
    "Modelos",
    "Atajos",
  ];

  const DICTATION_STEP = 3;
  const MODELS_STEP = 4;
  const SHORTCUTS_STEP = 5;

  const cfg = $derived(config.current);

  let step = $state(0);
  let saving = $state(false);
  let downloadingId = $state<string | null>(null);
  let downloadError = $state<string | null>(null);

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

  const coreConflicts = $derived(
    SETUP_SHORTCUTS.filter((item) => config.conflicts.includes(item.conflict)).map(
      (item) => item.label,
    ),
  );

  function useLabel(id: string): string {
    const uses: string[] = [];
    if (id === cfg?.dictation_whisper_model) uses.push("Dictado");
    if (id === cfg?.whisper_model) uses.push("Reuniones");
    return uses.join(" y ");
  }

  function patch(changes: Parameters<typeof config.patch>[0]) {
    void config.patch(changes).catch(() => {});
  }

  function setDictationBackend(value: string) {
    patch({
      dictation_backend: value,
      live_engine: value === "groq" ? "groq" : "local",
    });
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

  async function leaveModels(download: boolean) {
    if (downloadingId) return;
    if (download && !allReady && !(await downloadMissing())) return;
    step += 1;
  }

  async function startPractice() {
    if (coreConflicts.length > 0) return;
    saving = true;
    try {
      await config.patch({ onboarding_done: true, onboarding_practice_done: false });
      onDone();
      void minimizeWindow().catch(() => {});
    } catch {
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
          {#each ["Nunca graba sola: siempre decidís vos.", "El audio no sale del PC al transcribir en local.", "Podés borrar cualquier grabación cuando quieras."] as claim, i (i)}
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
      {:else if step === DICTATION_STEP}
        <p class="max-w-[60ch] text-sm leading-relaxed text-muted">
          Si no tenés gráfica, Groq dicta casi al instante. El audio de esa frase corta
          sale de tu PC. Las reuniones se transcriben en tu máquina por defecto; en
          Reuniones o Ajustes podés pasarlas a Groq.
        </p>

        <SegmentedControl
          value={cfg.dictation_backend === "groq" ? "groq" : "local"}
          label="Motor de dictado"
          options={[
            { value: "groq", label: "Groq" },
            { value: "local", label: "Local" },
          ]}
          onchange={setDictationBackend}
          full
        />

        {#if cfg.dictation_backend === "groq"}
          <GroqKeyField />
        {:else}
          <p class="max-w-[60ch] text-xs leading-relaxed text-faint">
            Whisper corre en el CPU. En el paso siguiente se baja un modelo chico.
          </p>
        {/if}
      {:else if step === MODELS_STEP}
        <p class="max-w-[60ch] text-sm leading-relaxed text-muted">
          {#if cfg.dictation_backend === "groq"}
            Las reuniones se transcriben en tu PC por defecto. Este modelo también sirve
            de reserva si Groq no responde.
          {:else}
            La transcripción corre en tu PC. Por defecto se baja un solo modelo rápido
            que sirve para dictado y para reuniones.
          {/if}
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
      {:else if step === SHORTCUTS_STEP}
        <p class="max-w-[60ch] text-sm leading-relaxed text-muted">
          Tres atajos, no diez. Confirmalos o cambialos ahora: después vas a tener que
          usarlos.
        </p>

        {#if coreConflicts.length > 0}
          <Banner
            tone="warn"
            title={coreConflicts.length === 1
              ? "Un atajo ya lo tenía tomado otra app"
              : `${coreConflicts.length} atajos ya los tenía tomados otra app`}
          >
            Elegí otra combinación para {coreConflicts.join(", ")}.
          </Banner>
        {/if}

        <ul class="flex list-none flex-col gap-3">
          {#each SETUP_SHORTCUTS as item (item.key)}
            <li class="flex items-start justify-between gap-3">
              <div class="flex min-w-0 flex-col gap-0.5">
                <span class="text-sm font-medium text-text">{item.label}</span>
                <span class="text-xs text-faint">{item.hint}</span>
              </div>
              <HotkeyCapture
                value={cfg[item.key]}
                defaultValue={item.fallback}
                ariaLabel="Cambiar el atajo de {item.label}"
                onChange={(sc) => patch({ [item.key]: sc })}
              />
            </li>
          {/each}
        </ul>

        <p class="text-xs text-faint">
          El resto está en Ajustes → Atajos.
        </p>
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
            <Button
              variant="primary"
              loading={saving}
              disabled={coreConflicts.length > 0}
              onclick={() => void startPractice()}
            >
              Practicar
            </Button>
          {/if}
        </div>
      </div>
    {/snippet}
  </Modal>
{/if}
