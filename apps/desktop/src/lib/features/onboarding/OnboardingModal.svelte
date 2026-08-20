<script lang="ts">
  /**
   * El primer uso: consentimiento, Groq o local, modelos, atajos y práctica.
   *
   * El primer uso no se cierra: el consentimiento no es decorativo. Si se
   * pidió repetir el tutorial (birrete o Ajustes), sí: Esc, la X o «Cerrar».
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
  import { t, whisperModelLabel } from "$domain/i18n.svelte";

  let {
    onDone,
    replay = false,
  }: {
    onDone: () => void;
    /** Pedido a propósito después del primer uso: se puede abandonar. */
    replay?: boolean;
  } = $props();

  const STEPS = $derived([
    t("onboarding.steps.welcome"),
    t("onboarding.steps.consent"),
    t("onboarding.steps.prefs"),
    t("onboarding.steps.dictation"),
    t("onboarding.steps.models"),
    t("onboarding.steps.shortcuts"),
  ]);

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
      (item) => t(`onboarding.setup.${item.id}.label`),
    ),
  );

  function useLabel(id: string): string {
    const uses: string[] = [];
    if (id === cfg?.dictation_whisper_model) uses.push(t("onboarding.useDictation"));
    if (id === cfg?.whisper_model) uses.push(t("onboarding.useMeetings"));
    return uses.join(t("onboarding.useJoin"));
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

  async function dismissReplay() {
    if (!replay || saving) return;
    saving = true;
    try {
      await config.patch({ onboarding_done: true, onboarding_practice_done: true });
    } catch {
      /* el modal sigue abierto si no se pudo persistir */
    } finally {
      saving = false;
    }
  }
</script>

{#if cfg}
  <Modal
    title={t("onboarding.title")}
    subtitle={t("onboarding.subtitle", {
      kind: replay ? t("onboarding.replay") : t("onboarding.firstUse"),
      step: STEPS[step],
    })}
    size="md"
    dismissible={replay}
    onClose={() => void dismissReplay()}
  >
    <div class="flex flex-col gap-4">
      {#if step === 0}
        <p class="max-w-[60ch] text-sm leading-relaxed text-muted">
          {t("onboarding.welcomeBody")}
        </p>
        <ul class="flex list-none flex-col gap-2">
          {#each [t("onboarding.claim1"), t("onboarding.claim2"), t("onboarding.claim3")] as claim, i (i)}
            <li class="flex items-baseline gap-2 text-sm text-muted">
              <Chip tone="ok">{i + 1}</Chip>
              {claim}
            </li>
          {/each}
        </ul>
      {:else if step === 1}
        <p class="max-w-[60ch] text-sm leading-relaxed text-muted">
          {t("onboarding.consentBody")}
        </p>
        <Switch
          checked={cfg.beep_on_start}
          label={t("onboarding.beep")}
          hint={t("onboarding.beepHint")}
          onchange={(checked) => patch({ beep_on_start: checked })}
        />
      {:else if step === 2}
        <Field label={t("onboarding.transcribeLang")}>
          {#snippet children({ id })}
            <Select
              {id}
              value={cfg.language}
              options={[
                { value: "es", label: t("onboarding.langEs") },
                { value: "auto", label: t("onboarding.langAuto") },
                { value: "en", label: t("onboarding.langEn") },
                { value: "pt", label: t("onboarding.langPt") },
              ]}
              onchange={(event: Event) =>
                patch({ language: (event.currentTarget as HTMLSelectElement).value })}
            />
          {/snippet}
        </Field>

        <Switch
          checked={cfg.speakers_mode}
          label={t("onboarding.speakers")}
          hint={t("onboarding.speakersHint")}
          onchange={(checked) => patch({ speakers_mode: checked })}
        />

        <Switch
          checked={cfg.autostart}
          label={t("onboarding.autostart")}
          hint={t("onboarding.autostartHint")}
          onchange={(checked) => patch({ autostart: checked })}
        />

        <p class="max-w-[60ch] text-xs leading-relaxed text-faint">
          {t("onboarding.trayNote")}
        </p>
      {:else if step === DICTATION_STEP}
        <p class="max-w-[60ch] text-sm leading-relaxed text-muted">
          {t("onboarding.dictationBody")}
        </p>

        <SegmentedControl
          value={cfg.dictation_backend === "groq" ? "groq" : "local"}
          label={t("onboarding.dictationEngine")}
          options={[
            { value: "groq", label: t("settings.meetings.groq") },
            { value: "local", label: t("settings.meetings.local") },
          ]}
          onchange={setDictationBackend}
          full
        />

        {#if cfg.dictation_backend === "groq"}
          <GroqKeyField />
        {:else}
          <p class="max-w-[60ch] text-xs leading-relaxed text-faint">
            {t("onboarding.whisperNote")}
          </p>
        {/if}
      {:else if step === MODELS_STEP}
        <p class="max-w-[60ch] text-sm leading-relaxed text-muted">
          {#if cfg.dictation_backend === "groq"}
            {t("onboarding.modelsGroq")}
          {:else}
            {t("onboarding.modelsLocal")}
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
                <span class="truncate text-xs text-faint">{whisperModelLabel(model.id)}</span>
              </div>
              {#if model.downloaded}
                <Chip tone="ok">{t("onboarding.ready")}</Chip>
              {:else if downloadingId === model.id}
                <Chip tone="warn">{t("onboarding.downloadingChip")}</Chip>
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
            label={t("onboarding.downloadingModel")}
            tone="ok"
          />
        {/if}

        {#if downloadError}
          <Banner tone="danger" title={t("onboarding.downloadFailed")}>
            {downloadError}
          </Banner>
        {:else if !allReady && pendingBytes > 0}
          <p class="text-xs text-faint">
            {t("onboarding.pendingBytes", { size: formatMegabytes(pendingBytes) })}
          </p>
        {/if}
      {:else if step === SHORTCUTS_STEP}
        <p class="max-w-[60ch] text-sm leading-relaxed text-muted">
          {t("onboarding.shortcutsBody")}
        </p>

        {#if coreConflicts.length > 0}
          <Banner
            tone="warn"
            title={coreConflicts.length === 1
              ? t("settings.shortcuts.conflictOne")
              : t("settings.shortcuts.conflictMany", { count: coreConflicts.length })}
          >
            {t("onboarding.conflictBody", { names: coreConflicts.join(", ") })}
          </Banner>
        {/if}

        <ul class="flex list-none flex-col gap-3">
          {#each SETUP_SHORTCUTS as item (item.key)}
            <li class="flex items-start justify-between gap-3">
              <div class="flex min-w-0 flex-col gap-0.5">
                <span class="text-sm font-medium text-text">{t(`onboarding.setup.${item.id}.label`)}</span>
                <span class="text-xs text-faint">{t(`onboarding.setup.${item.id}.hint`)}</span>
              </div>
              <HotkeyCapture
                value={cfg[item.key]}
                defaultValue={item.fallback}
                ariaLabel={t("settings.shortcuts.changeAria", {
                  label: t(`onboarding.setup.${item.id}.label`),
                })}
                onChange={(sc) => patch({ [item.key]: sc })}
              />
            </li>
          {/each}
        </ul>

        <p class="text-xs text-faint">
          {t("onboarding.restInSettings")}
        </p>
      {/if}
    </div>

    {#snippet actions()}
      <div class="flex w-full items-center justify-between gap-4">
        <div class="flex gap-1" aria-label={t("onboarding.stepAria", { current: step + 1, total: STEPS.length })}>
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
          {#if replay}
            <Button
              variant="ghost"
              disabled={Boolean(downloadingId) || saving}
              onclick={() => void dismissReplay()}
            >
              {t("onboarding.close")}
            </Button>
          {/if}
          {#if step > 0}
            <Button
              variant="ghost"
              disabled={Boolean(downloadingId) || saving}
              onclick={() => (step -= 1)}
            >
              {t("onboarding.back")}
            </Button>
          {/if}

          {#if step < MODELS_STEP}
            <Button variant="primary" onclick={() => (step += 1)}>{t("onboarding.next")}</Button>
          {:else if step === MODELS_STEP}
            {#if allReady}
              <Button variant="primary" onclick={() => void leaveModels(false)}>
                {t("onboarding.next")}
              </Button>
            {:else}
              <Button
                variant="ghost"
                disabled={Boolean(downloadingId)}
                onclick={() => void leaveModels(false)}
              >
                {t("onboarding.later")}
              </Button>
              <Button
                variant="primary"
                loading={Boolean(downloadingId)}
                onclick={() => void leaveModels(true)}
              >
                {t("onboarding.downloadNext")}
              </Button>
            {/if}
          {:else}
            <Button
              variant="primary"
              loading={saving}
              disabled={coreConflicts.length > 0}
              onclick={() => void startPractice()}
            >
              {t("onboarding.practice")}
            </Button>
          {/if}
        </div>
      </div>
    {/snippet}
  </Modal>
{/if}
