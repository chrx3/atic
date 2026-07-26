<script lang="ts">
  import { onMount } from "svelte";
  import type {
    AppConfig,
    InputDeviceInfo,
    ModelStatus,
    SecretsStatus,
    SummaryProvider,
    AudioTestResult,
    RetentionPreview,
  } from "$lib/types";
  import HotkeyCapture from "$lib/HotkeyCapture.svelte";
  import ModalShell from "$lib/ModalShell.svelte";
  import { formatMegabytes } from "$lib/format";
  import {
    getConfig,
    previewSound,
    setConfig,
    secretsStatus,
    setSecret,
    ollamaAvailable,
    listModels,
    listSummaryProviders,
    listInputDevices,
    listOutputDevices,
    debugListAudioDevices,
    downloadModelAndWait,
    onModelDownloadProgress,
    checkAppUpdate,
    installAppUpdateAndRelaunch,
    type AppUpdate,
    testAudio,
    previewRetention,
    cleanupRetention,
    cleanupCapturesNow,
    openDataDir,
  } from "$lib/api";
  import { looksLikeHeadset } from "$lib/audioHeadset";

  /** Modelos STT oficiales de Groq (mismo catálogo que el backend). */
  const GROQ_WHISPER_OPTIONS = [
    {
      id: "whisper-large-v3-turbo",
      label: "Whisper Large v3 Turbo (rápido)",
    },
    { id: "whisper-large-v3", label: "Whisper Large v3 (más preciso)" },
  ] as const;

  let {
    onClose,
    onSaved,
    onToast,
  }: {
    onClose: () => void;
    onSaved: (cfg: AppConfig) => void;
    onToast: (msg: string) => void;
  } = $props();

  let cfg = $state<AppConfig | null>(null);
  let secrets = $state<SecretsStatus | null>(null);
  let providers = $state<SummaryProvider[]>([]);
  let models = $state<ModelStatus[]>([]);
  let inputDevices = $state<InputDeviceInfo[]>([]);
  let outputDevices = $state<InputDeviceInfo[]>([]);
  let devicesLoading = $state(false);
  let devicesError = $state<string | null>(null);
  let audioTest = $state<AudioTestResult | null>(null);
  let audioTestError = $state<string | null>(null);
  let audioTestRunning = $state(false);
  let retentionPreviewData = $state<RetentionPreview | null>(null);
  let retentionBusy = $state(false);
  let retentionConfirming = $state(false);
  let retentionError = $state<string | null>(null);
  let ollamaOk = $state(false);
  let apiKeyInput = $state("");
  let groqDictationKey = $state("");
  let smtpPassword = $state("");
  let saving = $state(false);
  let downloading = $state<{
    id: string;
    downloaded: number;
    total: number;
  } | null>(null);

  type UpdateUiState =
    | { kind: "idle" }
    | { kind: "checking" }
    | { kind: "up_to_date" }
    | { kind: "available"; update: AppUpdate }
    | { kind: "downloading"; version: string; percent: number | null }
    | { kind: "error"; message: string };

  let updateUi = $state<UpdateUiState>({ kind: "idle" });
  let pendingUpdate = $state<AppUpdate | null>(null);

  type SettingsSectionId =
    | "general"
    | "shortcuts"
    | "audio"
    | "meetings"
    | "dictation"
    | "summary"
    | "captures";

  let activeSection = $state<SettingsSectionId>("general");

  let captureCleanupMsg = $state<string | null>(null);
  async function runCaptureCleanup() {
    try {
      const deleted = await cleanupCapturesNow();
      captureCleanupMsg =
        deleted > 0
          ? `Se eliminaron ${deleted} captura(s).`
          : "No había capturas vencidas.";
    } catch (error) {
      captureCleanupMsg = String(error);
    }
  }

  const SETTINGS_SECTIONS: { id: SettingsSectionId; label: string }[] = [
    { id: "general", label: "General" },
    { id: "shortcuts", label: "Atajos" },
    { id: "audio", label: "Audio" },
    { id: "meetings", label: "Reuniones" },
    { id: "dictation", label: "Dictado" },
    { id: "summary", label: "Resúmenes" },
    { id: "captures", label: "Capturas" },
  ];

  const activeSectionLabel = $derived(
    SETTINGS_SECTIONS.find((s) => s.id === activeSection)?.label ?? "Ajustes",
  );

  const showGroqKeyInTranscription = $derived(
    Boolean(
      cfg?.live_transcription &&
        cfg.live_engine === "groq" &&
        cfg.dictation_backend !== "groq",
    ),
  );

  const meetingModel = $derived(
    models.find((m) => m.id === (cfg?.whisper_model ?? "base")),
  );
  const dictationModel = $derived(
    models.find((m) => m.id === (cfg?.dictation_whisper_model ?? "base")),
  );
  const liveModel = $derived(
    models.find((m) => m.id === (cfg?.live_whisper_model ?? "small")),
  );
  const selectedProvider = $derived(
    providers.find((p) => p.id === (cfg?.summary_backend ?? "claude")),
  );
  const hasProviderKey = $derived(
    selectedProvider
      ? Boolean(secrets?.providers?.[selectedProvider.id])
      : false,
  );
  const dlPct = $derived(
    downloading && downloading.total > 0
      ? Math.round((downloading.downloaded / downloading.total) * 100)
      : 0,
  );

  function modelRow(m: ModelStatus | undefined) {
    if (!m) return null;
    return {
      model: m,
      downloading: downloading?.id === m.id,
      pct: downloading?.id === m.id ? dlPct : 0,
    };
  }

  function onProviderChange(id: string) {
    if (!cfg) return;
    const p = providers.find((x) => x.id === id);
    if (!p) return;
    const prevId = cfg.summary_backend;
    cfg.summary_backend = id;
    if (prevId !== id) {
      if (p.default_model) cfg.summary_model = p.default_model;
      cfg.summary_base_url = p.default_base_url;
    }
    apiKeyInput = "";
  }

  async function refreshModels() {
    models = await listModels();
  }

  const defaultInputName = $derived(
    inputDevices.find((d) => d.is_default)?.name ?? null,
  );
  const defaultOutputName = $derived(
    outputDevices.find((d) => d.is_default)?.name ?? null,
  );
  const micMissing = $derived(
    Boolean(
      cfg?.mic_device_id &&
        !inputDevices.some((d) => d.id === cfg?.mic_device_id),
    ),
  );
  const activeInput = $derived(
    cfg ? selectedDevice(cfg.mic_device_id, inputDevices) : undefined,
  );
  const bluetoothMicActive = $derived(
    Boolean(activeInput?.is_bluetooth),
  );
  const outputMissing = $derived(
    Boolean(
      cfg?.output_device_id &&
        !outputDevices.some((d) => d.id === cfg?.output_device_id),
    ),
  );
  const headsetInputCount = $derived(
    inputDevices.filter((d) => looksLikeHeadset(d.name)).length,
  );
  const fewMicsNoHeadset = $derived(
    inputDevices.length <= 1 && headsetInputCount === 0,
  );

  async function refreshDevices() {
    devicesLoading = true;
    devicesError = null;
    try {
      const [inputs, outputs] = await Promise.all([
        listInputDevices(),
        listOutputDevices(),
      ]);
      inputDevices = inputs;
      outputDevices = outputs;
      // Migra en memoria configuraciones antiguas que guardaban el nombre CPAL.
      if (cfg?.mic_device_id && !inputs.some((device) => device.id === cfg?.mic_device_id)) {
        const migrated = inputs.find((device) => device.name === cfg?.mic_device_id);
        if (migrated) cfg.mic_device_id = migrated.id;
      }
      if (
        cfg?.dictation_mic_device_id &&
        !inputs.some((device) => device.id === cfg?.dictation_mic_device_id)
      ) {
        const migrated = inputs.find(
          (device) => device.name === cfg?.dictation_mic_device_id,
        );
        if (migrated) cfg.dictation_mic_device_id = migrated.id;
      }
      if (
        cfg?.output_device_id &&
        !outputs.some((device) => device.id === cfg?.output_device_id)
      ) {
        const migrated = outputs.find((device) => device.name === cfg?.output_device_id);
        if (migrated) cfg.output_device_id = migrated.id;
      }
      try {
        const report = await debugListAudioDevices();
        console.info("[audio devices]\n" + report);
      } catch (diagErr) {
        console.warn("[audio devices] diagnóstico:", diagErr);
      }
    } catch (e) {
      inputDevices = [];
      outputDevices = [];
      devicesError = String(e);
    } finally {
      devicesLoading = false;
    }
  }

  function selectedDevice(
    id: string,
    devices: InputDeviceInfo[],
  ): InputDeviceInfo | undefined {
    return id
      ? devices.find((device) => device.id === id)
      : devices.find((device) => device.is_default) ?? devices[0];
  }

  function bluetoothOutput(): InputDeviceInfo | undefined {
    if (!cfg) return undefined;
    const selected = selectedDevice(cfg.output_device_id, outputDevices);
    if (selected?.is_bluetooth || (selected && looksLikeHeadset(selected.name))) {
      return selected;
    }
    return outputDevices.find(
      (device) => device.is_bluetooth || looksLikeHeadset(device.name),
    );
  }

  function applyQualityProfile() {
    if (!cfg) return;
    const mic =
      inputDevices.find(
        (device) =>
          device.is_default && !device.is_bluetooth && !device.may_not_open,
      ) ??
      inputDevices.find(
        (device) => !device.is_bluetooth && !device.may_not_open,
      );
    const output = bluetoothOutput();
    if (!mic) {
      onToast("No se encontró un micrófono interno o USB disponible.");
      return;
    }
    cfg.mic_device_id = mic.id;
    if (output) cfg.output_device_id = output.id;
    cfg.speakers_mode = false;
    cfg.record_tracks = "both";
    audioTest = null;
    onToast(`Calidad protegida: ${mic.name} + ${output?.name ?? "salida actual"}`);
  }

  function applyHeadsetProfile() {
    if (!cfg) return;
    const mic =
      inputDevices.find((device) => device.is_bluetooth) ??
      inputDevices.find((device) => looksLikeHeadset(device.name));
    const out = bluetoothOutput();
    if (mic) cfg.mic_device_id = mic.id;
    if (out) cfg.output_device_id = out.id;
    cfg.speakers_mode = false;
    cfg.record_tracks = "both";
    audioTest = null;
    if (mic || out) {
      onToast(
        mic && out
          ? `Auriculares: entrada «${mic.name}», salida «${out.name}»`
          : mic
            ? `Micrófono de auriculares: «${mic.name}»`
            : `Salida de auriculares: «${out!.name}»`,
      );
    } else {
      onToast(
        "No se detectó un mic/salida de auriculares. En Windows Sound elige Hands-Free, luego «Detectar de nuevo».",
      );
    }
  }

  function applySystemOnlyProfile() {
    if (!cfg) return;
    const output = bluetoothOutput();
    if (output) cfg.output_device_id = output.id;
    cfg.speakers_mode = true;
    audioTest = null;
    onToast("Solo otros: no se abrirá ningún micrófono.");
  }

  async function runAudioTest() {
    if (!cfg || audioTestRunning) return;
    audioTestRunning = true;
    audioTest = null;
    audioTestError = null;
    try {
      audioTest = await testAudio(cfg);
    } catch (error) {
      audioTestError = String(error);
    } finally {
      audioTestRunning = false;
    }
  }

  function describeTrack(name: string, track: AudioTestResult["mic"]): string {
    if (!track) return `${name}: no se capturó`;
    const quality = track.silent
      ? "sin señal útil"
      : track.clipped
        ? "señal saturada"
        : "señal correcta";
    return `${name}: ${quality} · ${track.sample_rate / 1000} kHz · ${track.channels === 1 ? "mono" : `${track.channels} canales`}`;
  }

  function deviceFormat(device: InputDeviceInfo): string {
    const parts: string[] = [];
    if (device.is_hands_free) parts.push("Hands-Free");
    else if (device.is_bluetooth) parts.push("Bluetooth");
    if (device.sample_rate) parts.push(`${device.sample_rate / 1000} kHz`);
    if (device.channels) {
      parts.push(device.channels === 1 ? "mono" : `${device.channels} canales`);
    }
    return parts.length ? ` · ${parts.join(" · ")}` : "";
  }

  async function reviewRetention() {
    if (!cfg || retentionBusy) return;
    retentionBusy = true;
    retentionError = null;
    retentionConfirming = false;
    try {
      retentionPreviewData = await previewRetention(cfg.retention_days);
    } catch (error) {
      retentionError = String(error);
    } finally {
      retentionBusy = false;
    }
  }

  async function runRetentionCleanup() {
    if (!cfg || retentionBusy || !retentionConfirming) return;
    retentionBusy = true;
    retentionError = null;
    try {
      const result = await cleanupRetention(cfg.retention_days);
      onToast(
        result.errors.length
          ? `Eliminadas ${result.deleted}; ${result.errors.length} con error`
          : `Eliminadas ${result.deleted} grabaciones (${formatMegabytes(result.bytes_freed)})`,
      );
      retentionPreviewData = await previewRetention(cfg.retention_days);
      retentionConfirming = false;
    } catch (error) {
      retentionError = String(error);
    } finally {
      retentionBusy = false;
    }
  }

  async function startDownload(id: string) {
    if (!cfg || downloading) return;
    downloading = { id, downloaded: 0, total: 1 };
    try {
      await downloadModelAndWait(id);
      downloading = null;
      await refreshModels();
      onToast("Modelo descargado");
    } catch (e) {
      downloading = null;
      onToast(String(e));
    }
  }

  /* ─── Sonidos ────────────────────────────────────────────────────────────
   *
   * Las claves coinciden con `SoundAction` en Rust y los ids de timbre con
   * `ToneProfile::parse`. Vacío significa "el timbre por defecto de esta
   * acción", que no es el mismo para todas: el dictado suena más contenido que
   * la grabación a propósito.
   */
  const SOUND_ACTIONS = [
    { key: "recording_start", label: "Empezar a grabar", field: "sound_recording_start" },
    { key: "recording_stop", label: "Terminar de grabar", field: "sound_recording_stop" },
    { key: "dictation_start", label: "Empezar a dictar", field: "sound_dictation_start" },
    { key: "dictation_done", label: "Texto pegado", field: "sound_dictation_done" },
    { key: "capture", label: "Captura de pantalla", field: "sound_capture" },
  ] as const;

  type SoundKey = (typeof SOUND_ACTIONS)[number]["key"];

  /** Ordenadas de grave a agudo: recorrer la lista es un barrido de registro,
   *  que es la diferencia que primero se oye entre dos timbres. */
  const SOUND_VOICES = [
    { id: "", label: "Por defecto" },
    { id: "grave", label: "Grave" },
    { id: "aire", label: "Aire" },
    { id: "pulso", label: "Pulso" },
    { id: "madera", label: "Madera" },
    { id: "cuerda", label: "Cuerda" },
    { id: "campana", label: "Campana" },
    { id: "digital", label: "Digital" },
    { id: "cristal", label: "Cristal" },
    { id: "ninguno", label: "Sin sonido" },
  ] as const;

  function soundField(key: SoundKey): string {
    return SOUND_ACTIONS.find((a) => a.key === key)!.field;
  }

  function soundValue(config: AppConfig, key: SoundKey): string {
    return (config as unknown as Record<string, string>)[soundField(key)] ?? "";
  }

  function setSound(config: AppConfig, key: SoundKey, value: string) {
    (config as unknown as Record<string, string>)[soundField(key)] = value;
  }

  onMount(() => {
    (async () => {
      cfg = await getConfig();
      if (cfg && !cfg.ui_theme) cfg.ui_theme = "system";
      if (cfg && typeof cfg.ui_sounds !== "boolean") cfg.ui_sounds = true;
      // Config vieja: sin estos campos los selects quedarían en `undefined` y
      // Svelte los mostraría vacíos en vez de en "Por defecto".
      if (cfg) {
        for (const a of SOUND_ACTIONS) {
          const bag = cfg as unknown as Record<string, string>;
          if (typeof bag[a.field] !== "string") bag[a.field] = "";
        }
      }
      secrets = await secretsStatus();
      providers = await listSummaryProviders();
      try {
        await refreshDevices();
      } catch {
        /* refreshDevices ya setea devicesError */
      }
      ollamaOk = await ollamaAvailable();
      await refreshModels();
    })();

    const unProgress = onModelDownloadProgress((p) => {
      downloading = {
        id: p.id,
        downloaded: p.downloaded,
        total: p.total,
      };
    });

    return () => {
      void unProgress.then((fn) => fn());
    };
  });

  async function save() {
    if (!cfg) return;
    saving = true;
    try {
      await setConfig(cfg);
      const secretKind = selectedProvider?.secret_kind;
      if (secretKind && apiKeyInput.trim()) {
        await setSecret(secretKind, apiKeyInput.trim());
        apiKeyInput = "";
      }
      if (groqDictationKey.trim()) {
        await setSecret("groq_api_key", groqDictationKey.trim());
        groqDictationKey = "";
      }
      if (smtpPassword.trim()) {
        await setSecret("smtp_password", smtpPassword.trim());
        smtpPassword = "";
      }
      secrets = await secretsStatus();
      ollamaOk = await ollamaAvailable();
      onSaved(cfg);
      onToast("Ajustes guardados");
      onClose();
    } catch (e) {
      onToast(String(e));
    } finally {
      saving = false;
    }
  }

  async function clearProviderKey() {
    const secretKind = selectedProvider?.secret_kind;
    if (!secretKind) return;
    await setSecret(secretKind, "");
    secrets = await secretsStatus();
    onToast("API key eliminada");
  }

  async function clearSmtpPassword() {
    await setSecret("smtp_password", "");
    secrets = await secretsStatus();
    onToast("Contraseña SMTP eliminada");
  }

  async function searchUpdates() {
    if (updateUi.kind === "checking" || updateUi.kind === "downloading") return;
    updateUi = { kind: "checking" };
    pendingUpdate = null;
    try {
      const update = await checkAppUpdate();
      if (!update) {
        updateUi = { kind: "up_to_date" };
        return;
      }
      pendingUpdate = update;
      updateUi = { kind: "available", update };
    } catch (e) {
      updateUi = {
        kind: "error",
        message: String(e),
      };
    }
  }

  async function installPendingUpdate() {
    const update = pendingUpdate;
    if (!update || updateUi.kind === "downloading") return;
    let downloaded = 0;
    let contentLength: number | null = null;
    updateUi = {
      kind: "downloading",
      version: update.version,
      percent: null,
    };
    try {
      await installAppUpdateAndRelaunch(update, (event) => {
        if (event.event === "Started") {
          contentLength = event.data.contentLength ?? null;
          updateUi = {
            kind: "downloading",
            version: update.version,
            percent: 0,
          };
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          const percent =
            contentLength && contentLength > 0
              ? Math.round((downloaded / contentLength) * 100)
              : null;
          updateUi = {
            kind: "downloading",
            version: update.version,
            percent,
          };
        } else if (event.event === "Finished") {
          updateUi = {
            kind: "downloading",
            version: update.version,
            percent: 100,
          };
        }
      });
    } catch (e) {
      updateUi = { kind: "error", message: String(e) };
    }
  }
</script>

<ModalShell
  title="Ajustes"
  subtitle="Preferencias esenciales"
  size="xl"
  onClose={onClose}
>
  {#if !cfg || !secrets}
    <p class="p-8 text-center text-sm rb-text-muted">Cargando…</p>
  {:else}
    {#snippet general(c: AppConfig, s: SecretsStatus)}
      <div class="rb-settings-group">
        <h4 class="rb-settings-group-title">Interfaz</h4>

        <div class="rb-settings-row">
          <div class="rb-settings-row-copy">
            <span class="rb-settings-row-label">Tema</span>
            <p class="rb-hint">También en el botón sol/luna de la barra.</p>
          </div>
          <div class="rb-settings-row-control">
            <select class="rb-field" bind:value={c.ui_theme}>
              <option value="light">Claro</option>
              <option value="dark">Oscuro</option>
              <option value="system">Sistema</option>
            </select>
          </div>
        </div>

        <label class="rb-settings-row rb-check">
          <span class="rb-settings-row-copy">
            <span class="rb-settings-row-label">Sonidos de interfaz</span>
            <span class="rb-hint">
              Toques al capturar y al dictar. Desactivarlo silencia todo.
            </span>
          </span>
          <span class="rb-settings-row-control">
            <input type="checkbox" bind:checked={c.ui_sounds} />
          </span>
        </label>

        <!-- Cada acción elige su timbre. El botón de prueba no es un extra:
             sin escucharlo no se puede elegir, y la alternativa sería guardar y
             provocar la acción real para comparar. -->
        {#if c.ui_sounds}
          <div class="rb-settings-group snd-group">
            <p class="rb-hint snd-intro">
              Cada acción suena distinto para reconocerla sin mirar. El timbre lo
              elegís vos; el gesto —sube al empezar, baja al terminar— no cambia.
            </p>
            {#each SOUND_ACTIONS as action (action.key)}
              <div class="rb-settings-row snd-row">
                <span class="rb-settings-row-copy">
                  <span class="rb-settings-row-label">{action.label}</span>
                </span>
                <span class="rb-settings-row-control snd-control">
                  <select
                    class="rb-field snd-pick"
                    value={soundValue(c, action.key)}
                    onchange={(e) =>
                      setSound(c, action.key, e.currentTarget.value)}
                    aria-label={`Timbre de ${action.label}`}
                  >
                    {#each SOUND_VOICES as voice (voice.id)}
                      <option value={voice.id}>{voice.label}</option>
                    {/each}
                  </select>
                  <button
                    type="button"
                    class="rb-btn rb-btn-ghost snd-try"
                    onclick={() =>
                      void previewSound(action.key, soundValue(c, action.key))}
                    disabled={soundValue(c, action.key) === "ninguno"}
                  >
                    Probar
                  </button>
                </span>
              </div>
            {/each}
          </div>
        {/if}
      </div>

      <div class="rb-settings-group">
        <h4 class="rb-settings-group-title">Arranque y pill</h4>

        <label class="rb-settings-row rb-check">
          <span class="rb-settings-row-copy">
            <span class="rb-settings-row-label">Iniciar con el sistema</span>
            <span class="rb-hint">Abre Atic en la bandeja al iniciar sesión.</span>
          </span>
          <span class="rb-settings-row-control">
            <input type="checkbox" bind:checked={c.autostart} />
          </span>
        </label>

        <label class="rb-settings-row rb-check">
          <span class="rb-settings-row-copy">
            <span class="rb-settings-row-label">Mostrar pill flotante</span>
            <span class="rb-hint">La barra compacta junto al cursor.</span>
          </span>
          <span class="rb-settings-row-control">
            <input type="checkbox" bind:checked={c.show_pill} />
          </span>
        </label>

        <div class="rb-settings-row">
          <div class="rb-settings-row-copy">
            <span class="rb-settings-row-label">Carpeta de datos</span>
            <p class="rb-hint">Grabaciones, capturas y archivos locales de Atic.</p>
          </div>
          <div class="rb-settings-row-control">
            <button
              type="button"
              class="rb-btn rb-btn-soft text-xs"
              onclick={() => void openDataDir("data")}
            >
              Abrir carpeta
            </button>
          </div>
        </div>
      </div>

      <div class="rb-settings-group">
        <h4 class="rb-settings-group-title">Versión y actualizaciones</h4>
        <p class="rb-hint">
          Busca versiones nuevas en GitHub Releases (build firmada).
        </p>
        <div class="rb-settings-actions">
          <button
            type="button"
            class="rb-btn rb-btn-primary"
            onclick={searchUpdates}
            disabled={updateUi.kind === "checking" ||
              updateUi.kind === "downloading"}
          >
            {#if updateUi.kind === "checking"}
              Buscando…
            {:else}
              Buscar actualizaciones
            {/if}
          </button>
          {#if updateUi.kind === "available"}
            <button
              type="button"
              class="rb-btn rb-btn-soft"
              onclick={installPendingUpdate}
            >
              Descargar e instalar {updateUi.update.version}
            </button>
          {/if}
          {#if updateUi.kind === "up_to_date"}
            <span class="rb-chip rb-chip-ok">Estás al día</span>
          {:else if updateUi.kind === "available"}
            <span class="rb-chip rb-chip-warn"
              >Actualización disponible: {updateUi.update.version}</span
            >
          {:else if updateUi.kind === "downloading"}
            <span class="rb-chip rb-chip-warn">
              Descargando {updateUi.version}{#if updateUi.percent !== null}
                … {updateUi.percent}%{/if}
            </span>
          {:else if updateUi.kind === "error"}
            <span class="rb-chip rb-chip-warn">{updateUi.message}</span>
          {/if}
        </div>
        {#if updateUi.kind === "downloading" && updateUi.percent !== null}
          <div class="rb-level-track">
            <div
              class="rb-level-fill rb-level-mic"
              style="width: {updateUi.percent}%"
            ></div>
          </div>
        {/if}
      </div>
    {/snippet}

    {#snippet shortcuts(c: AppConfig, s: SecretsStatus)}
      <div class="rb-settings-group">
        <h4 class="rb-settings-group-title">Atajos globales</h4>

        <div class="rb-settings-hotkey">
          <p class="rb-settings-hotkey-label">Grabar reunión</p>
          <HotkeyCapture
            value={c.global_shortcut}
            defaultValue="CmdOrCtrl+Shift+R"
            ariaLabel="Cambiar atajo de grabación"
            onChange={(sc) => {
              c.global_shortcut = sc;
            }}
          />
        </div>

        <div class="rb-settings-hotkey">
          <p class="rb-settings-hotkey-label">Dictado</p>
          <HotkeyCapture
            value={c.dictation_shortcut}
            defaultValue="CmdOrCtrl+Shift+D"
            ariaLabel="Cambiar atajo de dictado"
            onChange={(sc) => {
              c.dictation_shortcut = sc;
            }}
          />
          <p class="rb-hint">
            {#if c.dictation_mode === "push_to_talk"}
              Mantén para hablar; al soltar, transcribe y pega.
            {:else}
              Pulsa para empezar o parar (transcribe y pega).
            {/if}
            El modo se configura en Dictado.
          </p>
        </div>

        <div class="rb-settings-hotkey">
          <p class="rb-settings-hotkey-label">Captura de pantalla</p>
          <HotkeyCapture
            value={c.screenshot_shortcut}
            defaultValue="CmdOrCtrl+Shift+4"
            ariaLabel="Cambiar atajo para abrir la selección de captura"
            onChange={(sc) => {
              c.screenshot_shortcut = sc;
            }}
          />
          <p class="rb-hint">
            Abre la selección de ventana, región o monitor. Esc cancela.
          </p>
        </div>

        <div class="rb-settings-hotkey">
          <p class="rb-settings-hotkey-label">Traer pill al cursor</p>
          <HotkeyCapture
            value={c.summon_pill_shortcut}
            defaultValue="CmdOrCtrl+Shift+P"
            ariaLabel="Cambiar atajo para traer la pill al cursor"
            onChange={(sc) => {
              c.summon_pill_shortcut = sc;
            }}
          />
          <p class="rb-hint">Muestra la pill y la acerca al puntero.</p>
        </div>

        <div class="rb-settings-hotkey">
          <p class="rb-settings-hotkey-label">Rueda de herramientas</p>
          <HotkeyCapture
            value={c.pill_radial_shortcut}
            defaultValue="Alt+Z"
            ariaLabel="Cambiar atajo de la rueda de herramientas"
            onChange={(sc) => {
              c.pill_radial_shortcut = sc;
            }}
          />
          <p class="rb-hint">
            Abre el selector en la pill. Rueda del ratón para elegir, clic para
            activar.
          </p>
        </div>

        <div class="rb-settings-hotkey">
          <p class="rb-settings-hotkey-label">Historial de clipboard</p>
          <HotkeyCapture
            value={c.clipboard_shortcut}
            defaultValue="CmdOrCtrl+Shift+V"
            ariaLabel="Cambiar atajo del historial de clipboard"
            onChange={(sc) => {
              c.clipboard_shortcut = sc;
            }}
          />
          <p class="rb-hint">Abre el historial local junto a la pill.</p>
        </div>

        <div class="rb-settings-hotkey">
          <p class="rb-settings-hotkey-label">Textos guardados</p>
          <HotkeyCapture
            value={c.snippets_shortcut}
            defaultValue="CmdOrCtrl+Shift+S"
            ariaLabel="Cambiar atajo del panel de textos"
            onChange={(sc) => {
              c.snippets_shortcut = sc;
            }}
          />
          <p class="rb-hint">Abre plantillas y el bloc de notas.</p>
        </div>
      </div>
    {/snippet}

    {#snippet audio(c: AppConfig, s: SecretsStatus)}
      <div class="rb-settings-group">
        <div class="rb-settings-group-toolbar">
          <h4 class="rb-settings-group-title">Entrada y salida</h4>
          <div class="rb-settings-actions">
            <button
              type="button"
              class="rb-btn rb-btn-ghost text-xs"
              onclick={refreshDevices}
              disabled={devicesLoading}
            >
              {devicesLoading ? "Detectando…" : "Detectar de nuevo"}
            </button>
            <button
              type="button"
              class="rb-btn rb-btn-ghost text-xs"
              onclick={runAudioTest}
              disabled={devicesLoading || audioTestRunning || !cfg}
            >
              {audioTestRunning ? "Probando 5 s…" : "Probar audio"}
            </button>
          </div>
        </div>

        <div class="rb-settings-block">
          <p class="rb-settings-row-label">Perfil para reuniones</p>
          <p class="rb-hint mt-1">
            Equilibra calidad, comodidad y las pistas que se graban.
          </p>
          <div
            class="mt-3 flex flex-wrap gap-2"
            role="group"
            aria-label="Perfil de audio"
          >
            <button
              type="button"
              class="rb-btn rb-btn-primary text-xs"
              onclick={applyQualityProfile}
              disabled={devicesLoading || inputDevices.length === 0}
            >
              Proteger calidad
            </button>
            <button
              type="button"
              class="rb-btn rb-btn-soft text-xs"
              onclick={applyHeadsetProfile}
              disabled={devicesLoading || inputDevices.length === 0}
            >
              Usar mic Bluetooth
            </button>
            <button
              type="button"
              class="rb-btn rb-btn-soft text-xs"
              onclick={applySystemOnlyProfile}
              disabled={devicesLoading || outputDevices.length === 0}
            >
              Solo otros
            </button>
          </div>
          <p class="rb-hint mt-2">
            Recomendado: micrófono interno o USB + salida Bluetooth (mantiene
            A2DP estéreo).
          </p>
        </div>

        <label class="rb-label">
          Micrófono (entrada)
          <select class="rb-field" bind:value={c.mic_device_id}>
            <option value="">
              Por defecto del sistema{defaultInputName
                ? ` · ${defaultInputName}`
                : ""}
            </option>
            {#each inputDevices as device (device.id)}
              <option value={device.id}>
                {device.name}{device.is_default
                  ? " · predeterminado"
                  : ""}{deviceFormat(device)}{device.may_not_open
                  ? " · puede no abrir"
                  : ""}
              </option>
            {/each}
          </select>
        </label>
        {#if micMissing}
          <span class="rb-chip rb-chip-warn"
            >El micrófono guardado no está conectado. Detecta de nuevo o elige
            otro.</span
          >
        {:else if bluetoothMicActive && !c.speakers_mode}
          <div class="rb-banner rb-banner-warn" role="status">
            <p class="text-xs font-medium">
              Esta entrada puede activar Hands-Free
            </p>
            <p class="rb-hint mt-1">
              Al grabar te pediremos confirmación. Usa «Proteger calidad» para
              mantener el audio estéreo.
            </p>
          </div>
        {/if}

        <label class="rb-label">
          Altavoces / auriculares (salida)
          <select class="rb-field" bind:value={c.output_device_id}>
            <option value="">
              Por defecto del sistema{defaultOutputName
                ? ` · ${defaultOutputName}`
                : ""}
            </option>
            {#each outputDevices as device (device.id)}
              <option value={device.id}>
                {device.name}{device.is_default
                  ? " · predeterminado"
                  : ""}{deviceFormat(device)}
              </option>
            {/each}
          </select>
        </label>
        {#if outputMissing}
          <span class="rb-chip rb-chip-warn"
            >La salida guardada no está conectada. Detecta de nuevo o elige
            otra.</span
          >
        {/if}

        {#if audioTestError}
          <div class="rb-banner rb-banner-warn" role="alert">
            <p class="text-xs font-medium">La prueba no pudo completarse</p>
            <p class="rb-hint mt-1">{audioTestError}</p>
          </div>
        {:else if audioTest}
          <div
            class="rb-banner {audioTest.mic?.silent || audioTest.system?.silent
              ? 'rb-banner-warn'
              : 'rb-banner-info'}"
            role="status"
            aria-live="polite"
          >
            <p class="text-xs font-medium">Resultado de la prueba</p>
            <p class="rb-hint mt-1">
              {describeTrack("Micrófono", audioTest.mic)}
            </p>
            <p class="rb-hint">
              {describeTrack("Otros", audioTest.system)}
            </p>
            {#if audioTest.preflight.risk === "bluetooth_hands_free"}
              <p class="rb-hint mt-2">
                Puede activar Hands-Free. Aplica «Proteger calidad» y repite la
                prueba.
              </p>
            {/if}
          </div>
        {/if}

        {#if devicesError}
          <span class="rb-chip rb-chip-warn">{devicesError}</span>
        {:else}
          <p class="rb-hint">
            {inputDevices.length} entrada(s), {outputDevices.length} salida(s).
            Entrada: grabación y dictado. Salida: «Otros» y beep.
          </p>
          {#if fewMicsNoHeadset}
            <span class="rb-chip rb-chip-warn">
              No aparece un micrófono Bluetooth. En Sonido de Windows elige el
              perfil Hands-Free del auricular (no solo A2DP) y pulsa «Detectar
              de nuevo».
            </span>
          {:else}
            <p class="rb-hint">
              Bluetooth: el micrófono suele estar en Hands-Free; A2DP es solo
              salida. Grabar con mic del auricular baja la calidad de lo que
              escuchas hasta detener. Evítalo con mic interno + salida BT, o
              Modo parlantes.
            </p>
          {/if}
        {/if}
      </div>
    {/snippet}

    {#snippet meetings(c: AppConfig, s: SecretsStatus)}
      <div class="rb-settings-group">
        <h4 class="rb-settings-group-title">Idioma y transcripción</h4>

        <label class="rb-label">
          Idioma de la reunión
          <select class="rb-field" bind:value={c.language}>
            <option value="es">Español (recomendado)</option>
            <option value="auto">Autodetectar (puede fallar con ruido)</option>
            <option value="en">Inglés</option>
            <option value="pt">Portugués</option>
            <option value="fr">Francés</option>
          </select>
        </label>
        <p class="rb-hint">
          Autodetectar a menudo inventa inglés con audio corto o ruidoso.
        </p>

        <label class="rb-label">
          Modelo para reuniones
          <select class="rb-field" bind:value={c.whisper_model}>
            {#each models as m (m.id)}
              <option value={m.id}>
                {m.display_name}{m.downloaded ? " · listo" : ""}
              </option>
            {/each}
          </select>
        </label>
        <p class="rb-hint">
          Base es rápido; Small prioriza precisión si hay más memoria.
        </p>
        {#if meetingModel}
          {@const row = modelRow(meetingModel)}
          {#if row}
            <div class="flex items-center justify-between gap-3">
              {#if row.model.downloaded}
                <span class="rb-chip rb-chip-ok">Listo</span>
              {:else if row.downloading}
                <span class="rb-chip rb-chip-warn">Descargando… {row.pct}%</span>
              {:else}
                <span class="rb-hint"
                  >No descargado · {formatMegabytes(row.model.approx_size_bytes)}</span
                >
                <button
                  type="button"
                  class="rb-btn rb-btn-primary"
                  onclick={() => startDownload(row.model.id)}
                  disabled={Boolean(downloading)}>Descargar</button
                >
              {/if}
            </div>
            {#if row.downloading}
              <div class="rb-level-track">
                <div
                  class="rb-level-fill rb-level-mic"
                  style="width: {row.pct}%"
                ></div>
              </div>
            {/if}
          {/if}
        {/if}

        <label class="rb-settings-row rb-check">
          <span class="rb-settings-row-copy">
            <span class="rb-settings-row-label">Al terminar una reunión</span>
            <span class="rb-hint">
              Transcribe en segundo plano. Si falla, el audio queda y puedes
              reintentar.
            </span>
          </span>
          <span class="rb-settings-row-control">
            <input
              type="checkbox"
              bind:checked={c.auto_transcribe_after_recording}
            />
          </span>
        </label>
      </div>

      <div class="rb-settings-group">
        <h4 class="rb-settings-group-title">Grabación</h4>

        <div class="rb-settings-row">
          <div class="rb-settings-row-copy">
            <span class="rb-settings-row-label">Carpeta de grabaciones</span>
          </div>
          <div class="rb-settings-row-control">
            <button
              type="button"
              class="rb-btn rb-btn-soft text-xs"
              onclick={() => void openDataDir("recordings")}
            >
              Abrir carpeta
            </button>
          </div>
        </div>

        <label class="rb-settings-row rb-check">
          <span class="rb-settings-row-copy">
            <span class="rb-settings-row-label">Modo parlantes</span>
            <span class="rb-hint">
              Sin auriculares el mic captura eco. Actívalo para grabar solo
              «otros».
            </span>
          </span>
          <span class="rb-settings-row-control">
            <input type="checkbox" bind:checked={c.speakers_mode} />
          </span>
        </label>

        <label class="rb-label" class:opacity-50={c.speakers_mode}>
          Qué grabar
          <select
            class="rb-field"
            bind:value={c.record_tracks}
            disabled={c.speakers_mode}
          >
            <option value="both">Yo + otros</option>
            <option value="mic">Solo yo</option>
            <option value="system">Solo otros</option>
          </select>
        </label>

        <label class="rb-label" class:opacity-50={c.speakers_mode}>
          Qué transcribir
          <select
            class="rb-field"
            bind:value={c.transcribe_tracks}
            disabled={c.speakers_mode}
          >
            <option value="both">Ambas pistas</option>
            <option value="mic">Solo yo</option>
            <option value="system">Solo otros</option>
          </select>
          <span class="rb-hint"
            >Útil si ya grabaste ambas y solo quieres procesar una.</span
          >
        </label>

        {#if c.speakers_mode}
          <span class="rb-chip rb-chip-warn">Modo parlantes: solo «otros»</span>
        {/if}

        <label class="rb-settings-row rb-check">
          <span class="rb-settings-row-copy">
            <span class="rb-settings-row-label"
              >Sonido al grabar (aviso de consentimiento)</span
            >
          </span>
          <span class="rb-settings-row-control">
            <input type="checkbox" bind:checked={c.beep_on_start} />
          </span>
        </label>

        <label class="rb-settings-row rb-check">
          <span class="rb-settings-row-copy">
            <span class="rb-settings-row-label">Detectar reuniones abiertas</span>
            <span class="rb-hint">
              Revisa localmente Teams, Zoom, Meet y Webex. Solo ofrece grabar.
            </span>
          </span>
          <span class="rb-settings-row-control">
            <input type="checkbox" bind:checked={c.detect_meetings} />
          </span>
        </label>
      </div>

      <div class="rb-settings-group">
        <h4 class="rb-settings-group-title">Conservación</h4>
        <div class="rb-settings-block">
          <label class="rb-label">
            Eliminar grabaciones con más de
            <select
              class="rb-field"
              bind:value={c.retention_days}
              onchange={() => {
                retentionPreviewData = null;
                retentionConfirming = false;
              }}
            >
              <option value={0}>Nunca</option>
              <option value={30}>30 días</option>
              <option value={90}>90 días</option>
              <option value={180}>180 días</option>
              <option value={365}>1 año</option>
            </select>
          </label>
          <label
            class="rb-check mt-3"
            class:opacity-50={c.retention_days === 0}
          >
            <input
              type="checkbox"
              bind:checked={c.retention_auto_cleanup}
              disabled={c.retention_days === 0}
            />
            Limpiar automáticamente al iniciar
          </label>
          <p class="rb-hint mt-2">
            Incluye audio, transcripción y resumen. Los exportados fuera de Atic
            no se eliminan.
          </p>
          <div class="mt-3 flex flex-wrap gap-2">
            <button
              type="button"
              class="rb-btn rb-btn-soft"
              onclick={reviewRetention}
              disabled={retentionBusy || c.retention_days === 0}
            >
              {retentionBusy ? "Revisando…" : "Revisar vencidos"}
            </button>
            {#if retentionPreviewData?.count && !retentionConfirming}
              <button
                type="button"
                class="rb-btn rb-btn-danger"
                onclick={() => (retentionConfirming = true)}
              >
                Preparar eliminación
              </button>
            {/if}
            {#if retentionConfirming}
              <button
                type="button"
                class="rb-btn rb-btn-danger-solid"
                onclick={runRetentionCleanup}
                disabled={retentionBusy}
              >
                Confirmar eliminación permanente
              </button>
              <button
                type="button"
                class="rb-btn rb-btn-ghost"
                onclick={() => (retentionConfirming = false)}
                disabled={retentionBusy}
              >
                Cancelar
              </button>
            {/if}
          </div>
          {#if retentionPreviewData}
            <p class="rb-hint mt-2" aria-live="polite">
              {retentionPreviewData.count === 0
                ? "No hay grabaciones vencidas."
                : `${retentionPreviewData.count} grabación(es), ${formatMegabytes(retentionPreviewData.bytes)}.`}
            </p>
          {/if}
          {#if retentionError}
            <span class="rb-chip rb-chip-warn" role="alert">{retentionError}</span>
          {/if}
        </div>
      </div>

      <details class="rb-settings-details">
        <summary>Avanzado</summary>
        <div class="rb-settings-details-body">
          <label class="rb-check">
            <input type="checkbox" bind:checked={c.live_transcription} />
            <span>
              <span class="font-medium">Vista previa en vivo (experimental)</span>
              <span class="rb-hint mt-0.5 block">
                Subtítulos provisionales mientras grabas. Puede tener retraso o
                frases incompletas; nunca se guarda ni reemplaza la
                transcripción final.
              </span>
            </span>
          </label>

          {#if c.live_transcription}
            <label class="rb-label">
              Motor en vivo
              <select class="rb-field" bind:value={c.live_engine}>
                <option value="local">Local (Whisper en el PC)</option>
                <option value="groq">Groq Whisper (nube)</option>
              </select>
            </label>
            {#if c.live_engine === "groq"}
              <p class="rb-hint">
                Con Groq el audio de la reunión se envía a la nube. En local no
                sale del PC.
              </p>
              <label class="rb-label">
                Modelo Groq (live)
                <select class="rb-field" bind:value={c.live_groq_model}>
                  {#each GROQ_WHISPER_OPTIONS as opt (opt.id)}
                    <option value={opt.id}>{opt.label}</option>
                  {/each}
                </select>
              </label>
              <span
                class="rb-chip {s.providers?.groq
                  ? 'rb-chip-ok'
                  : 'rb-chip-warn'}"
              >
                {s.providers?.groq
                  ? "Key configurada en el llavero"
                  : "Sin key — se usará Whisper local"}
              </span>
            {:else}
              <p class="rb-hint">
                Local procesa en tu PC. Groq es más rápido pero envía audio a la
                nube.
              </p>
            {/if}
            {#if c.live_engine === "local" || !s.providers?.groq}
              <label class="rb-label">
                Modelo para transcripción en vivo
                <select class="rb-field" bind:value={c.live_whisper_model}>
                  {#each models as m (m.id)}
                    <option value={m.id}>
                      {m.display_name}{m.downloaded ? " · listo" : ""}
                    </option>
                  {/each}
                </select>
              </label>
              <p class="rb-hint">
                Small equilibra latencia y precisión. Base es más ligero. También
                se usa si eliges Groq sin API key.
              </p>
              {#if liveModel}
                {@const row = modelRow(liveModel)}
                {#if row}
                  <div class="flex items-center justify-between gap-3">
                    {#if row.model.downloaded}
                      <span class="rb-chip rb-chip-ok">Listo</span>
                    {:else if row.downloading}
                      <span class="rb-chip rb-chip-warn"
                        >Descargando… {row.pct}%</span
                      >
                    {:else}
                      <span class="rb-hint"
                        >No descargado · {formatMegabytes(
                          row.model.approx_size_bytes,
                        )}</span
                      >
                      <button
                        type="button"
                        class="rb-btn rb-btn-primary"
                        onclick={() => startDownload(row.model.id)}
                        disabled={Boolean(downloading)}>Descargar</button
                      >
                    {/if}
                  </div>
                  {#if row.downloading}
                    <div class="rb-level-track">
                      <div
                        class="rb-level-fill rb-level-mic"
                        style="width: {row.pct}%"
                      ></div>
                    </div>
                  {/if}
                {/if}
              {/if}
            {/if}

            {#if showGroqKeyInTranscription}
              <label class="rb-label">
                API key Groq (transcripción en vivo)
                <input
                  type="password"
                  class="rb-field"
                  placeholder={s.providers?.groq
                    ? "•••••••• (llavero — escribe para reemplazar)"
                    : "Pega tu API key de Groq…"}
                  bind:value={groqDictationKey}
                  autocomplete="off"
                />
              </label>
              <p class="rb-hint">Crea tu cuenta y API key en groq.com</p>
            {/if}
          {/if}

          <label class="rb-label">
            Supresión de ruido (mic)
            <select class="rb-field" bind:value={c.noise_suppression}>
              <option value="off">Desactivada (reuniones)</option>
              <option value="low">Baja</option>
              <option value="medium">Media (recomendado en notebook)</option>
              <option value="high">Alta (puede silenciar la voz)</option>
            </select>
          </label>
          <p class="rb-hint">
            Solo afecta a «Yo». El dictado usa al menos media. Alta puede dejar
            la pista casi en silencio. No afecta a «Otros».
          </p>
        </div>
      </details>
    {/snippet}

    {#snippet dictation(c: AppConfig, s: SecretsStatus)}
      <div class="rb-settings-group">
        <h4 class="rb-settings-group-title">Modo</h4>
        <div class="rb-settings-row">
          <div class="rb-settings-row-copy">
            <span class="rb-settings-row-label">Cómo activar</span>
            <span class="rb-hint">El atajo está en Atajos.</span>
          </div>
          <div class="rb-settings-row-control">
            <select class="rb-field" bind:value={c.dictation_mode}>
              <option value="push_to_talk">Push-to-talk (mantener)</option>
              <option value="toggle">Toggle (pulsar para iniciar/parar)</option>
            </select>
          </div>
        </div>
      </div>

      <div class="rb-settings-group">
        <h4 class="rb-settings-group-title">Motor y micrófono</h4>
        <label class="rb-label">
          Motor de dictado
          <select class="rb-field" bind:value={c.dictation_backend}>
            <option value="local">Local (Whisper en el PC)</option>
            <option value="groq">Groq Whisper (nube, más rápido)</option>
          </select>
        </label>
        <p class="rb-hint">
          Groq es más rápido y requiere API key; sin ella se usa local. El audio
          solo sale del PC con Groq.
        </p>

        <label class="rb-label">
          Micrófono para dictado
          <select class="rb-field" bind:value={c.dictation_mic_device_id}>
            <option value="">Usar el micrófono de reuniones</option>
            {#each inputDevices as device (device.id)}
              <option value={device.id}>
                {device.name}{deviceFormat(device)}
              </option>
            {/each}
          </select>
        </label>
        <p class="rb-hint">
          Puedes usar Bluetooth solo para dictados y un mic interno o USB para
          reuniones.
        </p>

        {#if c.dictation_backend === "groq"}
          <label class="rb-label">
            Modelo Groq (dictado)
            <select class="rb-field" bind:value={c.dictation_groq_model}>
              {#each GROQ_WHISPER_OPTIONS as opt (opt.id)}
                <option value={opt.id}>{opt.label}</option>
              {/each}
            </select>
          </label>
          <p class="rb-hint">
            Turbo es más rápido y barato; Large v3 prioriza precisión.
          </p>
          <label class="rb-label">
            API key Groq (necesaria para nube)
            <input
              type="password"
              class="rb-field"
              placeholder={s.providers?.groq
                ? "•••••••• (llavero — escribe para reemplazar)"
                : "Pega tu API key de Groq…"}
              bind:value={groqDictationKey}
              autocomplete="off"
            />
          </label>
          <p class="rb-hint">Crea tu cuenta y API key en groq.com</p>
          <span
            class="rb-chip {s.providers?.groq
              ? 'rb-chip-ok'
              : 'rb-chip-warn'}"
          >
            {s.providers?.groq
              ? "Key configurada en el llavero"
              : "Sin key — se usará Whisper local"}
          </span>
        {/if}

        {#if c.dictation_backend !== "groq"}
          <label class="rb-label">
            Modelo local para dictado
            <select class="rb-field" bind:value={c.dictation_whisper_model}>
              {#each models as m (m.id)}
                <option value={m.id}>
                  {m.display_name}{m.downloaded ? " · listo" : ""}
                </option>
              {/each}
            </select>
          </label>
          <p class="rb-hint">
            Base es más rápido; Small si necesitas más precisión.
          </p>
          {#if dictationModel}
            {@const row = modelRow(dictationModel)}
            {#if row}
              <div class="flex items-center justify-between gap-3">
                {#if row.model.downloaded}
                  <span class="rb-chip rb-chip-ok">Listo</span>
                {:else if row.downloading}
                  <span class="rb-chip rb-chip-warn"
                    >Descargando… {row.pct}%</span
                  >
                {:else}
                  <span class="rb-hint"
                    >No descargado · {formatMegabytes(
                      row.model.approx_size_bytes,
                    )}</span
                  >
                  <button
                    type="button"
                    class="rb-btn rb-btn-primary"
                    onclick={() => startDownload(row.model.id)}
                    disabled={Boolean(downloading)}>Descargar</button
                  >
                {/if}
              </div>
              {#if row.downloading}
                <div class="rb-level-track">
                  <div
                    class="rb-level-fill rb-level-mic"
                    style="width: {row.pct}%"
                  ></div>
                </div>
              {/if}
            {/if}
          {/if}
        {/if}
      </div>
    {/snippet}

    {#snippet summary(c: AppConfig, s: SecretsStatus)}
      <div class="rb-settings-group">
        <h4 class="rb-settings-group-title">Proveedor</h4>
        <label class="rb-label">
          Proveedor
          <select
            class="rb-field"
            value={c.summary_backend}
            onchange={(e) =>
              onProviderChange((e.currentTarget as HTMLSelectElement).value)}
          >
            {#each providers as p (p.id)}
              <option value={p.id}>{p.display_name}</option>
            {/each}
          </select>
        </label>

        {#if selectedProvider}
          <label class="rb-label">
            Modelo
            {#if (selectedProvider.suggested_models?.length ?? 0) > 0}
              <select class="rb-field" bind:value={c.summary_model}>
                {#each selectedProvider.suggested_models as m (m)}
                  <option value={m}>{m}</option>
                {/each}
                {#if c.summary_model &&
                  !selectedProvider.suggested_models.includes(c.summary_model)}
                  <option value={c.summary_model}>{c.summary_model}</option>
                {/if}
              </select>
            {:else}
              <input
                class="rb-field"
                bind:value={c.summary_model}
                placeholder={selectedProvider.default_model ||
                  "nombre-del-modelo…"}
              />
            {/if}
          </label>

          {#if selectedProvider.kind !== "claude"}
            <label class="rb-label">
              URL base
              <input
                class="rb-field"
                bind:value={c.summary_base_url}
                placeholder={selectedProvider.default_base_url || "https://…/v1"}
                disabled={!selectedProvider.base_url_editable}
              />
            </label>
          {/if}

          {#if selectedProvider.needs_api_key && selectedProvider.secret_kind}
            <label class="rb-label">
              API key
              <input
                type="password"
                class="rb-field"
                placeholder={hasProviderKey
                  ? "•••••••• (ya guardada — escribe para reemplazar)"
                  : "Pega tu API key…"}
                bind:value={apiKeyInput}
                autocomplete="off"
              />
            </label>
            {#if hasProviderKey}
              <button
                type="button"
                class="rb-btn rb-btn-danger"
                onclick={clearProviderKey}
              >
                Eliminar API key
              </button>
            {:else}
              <span class="rb-chip rb-chip-warn"
                >Necesitas una API key para este proveedor.</span
              >
            {/if}
          {/if}

          {#if selectedProvider.id === "ollama"}
            <span class="rb-chip {ollamaOk ? 'rb-chip-ok' : 'rb-chip-warn'}">
              {ollamaOk
                ? "Ollama responde"
                : "Ollama no responde — ¿está en marcha?"}
            </span>
          {/if}

          {#if selectedProvider.id === "custom"}
            <p class="rb-hint">
              Endpoint compatible con OpenAI Chat Completions (Together,
              Fireworks, LM Studio, vLLM, etc.).
            </p>
          {/if}
        {/if}
      </div>

      <div class="rb-settings-group">
        <h4 class="rb-settings-group-title">Enviar</h4>
        <div class="rb-settings-row">
          <div class="rb-settings-row-copy">
            <span class="rb-settings-row-label">Cómo enviar</span>
            <span class="rb-hint">Mailto abre el cliente; SMTP envía desde Atic.</span>
          </div>
          <div class="rb-settings-row-control">
            <select class="rb-field" bind:value={c.mail_backend}>
              <option value="mailto">Abrir cliente de correo (mailto)</option>
              <option value="smtp">SMTP directo</option>
            </select>
          </div>
        </div>

        {#if c.mail_backend === "smtp"}
          <details class="rb-settings-details">
            <summary>Configuración SMTP</summary>
            <div class="rb-settings-details-body">
              <label class="rb-label">
                Host
                <input
                  class="rb-field"
                  bind:value={c.smtp_host}
                  placeholder="smtp.ejemplo.com"
                />
              </label>
              <div class="rb-settings-two-col">
                <label class="rb-label">
                  Puerto
                  <input
                    type="number"
                    class="rb-field"
                    bind:value={c.smtp_port}
                  />
                </label>
                <label class="rb-check items-end pb-2">
                  <input type="checkbox" bind:checked={c.smtp_use_tls} />
                  STARTTLS
                </label>
              </div>
              <label class="rb-label">
                Usuario
                <input class="rb-field" bind:value={c.smtp_username} />
              </label>
              <label class="rb-label">
                Remitente (From)
                <input
                  class="rb-field"
                  bind:value={c.smtp_from}
                  placeholder="opcional"
                />
              </label>
              <label class="rb-label">
                Contraseña
                <input
                  type="password"
                  class="rb-field"
                  placeholder={s.has_smtp_password
                    ? "•••••••• (ya guardada)"
                    : ""}
                  bind:value={smtpPassword}
                  autocomplete="off"
                />
              </label>
              {#if s.has_smtp_password}
                <button
                  type="button"
                  class="rb-btn rb-btn-danger"
                  onclick={clearSmtpPassword}
                >
                  Eliminar contraseña
                </button>
              {/if}
            </div>
          </details>
        {/if}
      </div>
    {/snippet}

    {#snippet captures(c: AppConfig, s: SecretsStatus)}
      <div class="rb-settings-group">
        <h4 class="rb-settings-group-title">Atajo</h4>
        <p class="rb-hint">Cambia el atajo en Atajos.</p>
      </div>

      <div class="rb-settings-group">
        <h4 class="rb-settings-group-title">Shelf y retención</h4>

        <label class="rb-label">
          Lado del shelf
          <select class="rb-field" bind:value={c.capture_shelf_side}>
            <option value="right">Derecha</option>
            <option value="left">Izquierda</option>
          </select>
        </label>

        <label class="rb-label">
          Retracción del shelf (segundos)
          <input
            class="rb-field"
            type="number"
            min="5"
            max="120"
            bind:value={c.capture_shelf_timeout_seconds}
          />
        </label>

        <label class="rb-label">
          Conservar capturas (horas, 0 = sin límite)
          <input
            class="rb-field"
            type="number"
            min="0"
            max="720"
            bind:value={c.capture_retention_hours}
          />
        </label>
      </div>

      <div class="rb-settings-group">
        <h4 class="rb-settings-group-title">Captura</h4>

        <label class="rb-settings-row rb-check">
          <span class="rb-settings-row-copy">
            <span class="rb-settings-row-label">Incluir el cursor</span>
          </span>
          <span class="rb-settings-row-control">
            <input type="checkbox" bind:checked={c.capture_include_cursor} />
          </span>
        </label>

        <label class="rb-label">
          Al hacer clic en la miniatura
          <select class="rb-field" bind:value={c.capture_click_action}>
            <option value="preview">Abrir vista previa</option>
            <option value="location">Abrir ubicación</option>
          </select>
        </label>

        <div class="rb-settings-actions">
          <button
            type="button"
            class="rb-btn rb-btn-soft text-xs"
            onclick={() => void openDataDir("captures")}
          >
            Abrir carpeta
          </button>
          <button
            type="button"
            class="rb-btn rb-btn-soft text-xs"
            onclick={runCaptureCleanup}
          >
            Limpiar capturas ahora
          </button>
        </div>
        {#if captureCleanupMsg}
          <p class="rb-hint mt-1">{captureCleanupMsg}</p>
        {/if}
      </div>
    {/snippet}

    <div class="rb-settings-layout text-sm">
      <nav class="rb-settings-sidebar" aria-label="Secciones de ajustes">
        {#each SETTINGS_SECTIONS as section (section.id)}
          <button
            type="button"
            class="rb-settings-nav-item"
            class:active={activeSection === section.id}
            aria-current={activeSection === section.id ? "page" : undefined}
            onclick={() => {
              activeSection = section.id;
            }}
          >
            {section.label}
          </button>
        {/each}
      </nav>

      <div class="rb-settings-content">
        <div class="rb-settings-panel">
          <h3 class="rb-settings-panel-title">{activeSectionLabel}</h3>

          {#if activeSection === "general"}
            {@render general(cfg, secrets)}
          {:else if activeSection === "shortcuts"}
            {@render shortcuts(cfg, secrets)}
          {:else if activeSection === "audio"}
            {@render audio(cfg, secrets)}
          {:else if activeSection === "meetings"}
            {@render meetings(cfg, secrets)}
          {:else if activeSection === "dictation"}
            {@render dictation(cfg, secrets)}
          {:else if activeSection === "summary"}
            {@render summary(cfg, secrets)}
          {:else if activeSection === "captures"}
            {@render captures(cfg, secrets)}
          {/if}
        </div>
      </div>
    </div>
  {/if}

  {#snippet actions()}
    {#if cfg && secrets}
      <button type="button" class="rb-btn rb-btn-ghost" onclick={onClose}
        >Cancelar</button
      >
      <button
        type="button"
        class="rb-btn rb-btn-primary"
        onclick={save}
        disabled={saving}
      >
        {saving ? "Guardando…" : "Guardar cambios"}
      </button>
    {/if}
  {/snippet}
</ModalShell>
