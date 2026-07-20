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
    openCapturesDir,
  } from "$lib/api";
  import { looksLikeHeadset } from "$lib/audioHeadset";

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
    | "appearance"
    | "shortcuts"
    | "audio"
    | "transcription"
    | "dictation"
    | "summary"
    | "recording"
    | "capturas"
    | "mail"
    | "updates";

  let activeSection = $state<SettingsSectionId>("appearance");

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
    { id: "appearance", label: "Apariencia" },
    { id: "shortcuts", label: "Atajos" },
    { id: "audio", label: "Audio" },
    { id: "transcription", label: "Transcripción" },
    { id: "dictation", label: "Dictado" },
    { id: "summary", label: "Resumen" },
    { id: "recording", label: "Grabación" },
    { id: "capturas", label: "Capturas" },
    { id: "mail", label: "Correo" },
    { id: "updates", label: "Actualizaciones" },
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

  onMount(() => {
    (async () => {
      cfg = await getConfig();
      if (cfg && !cfg.ui_theme) cfg.ui_theme = "system";
      if (cfg && typeof cfg.ui_sounds !== "boolean") cfg.ui_sounds = true;
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

          {#if activeSection === "appearance"}
            <div class="rb-settings-group">
              <label class="rb-label">
                Tema
                <select class="rb-field" bind:value={cfg.ui_theme}>
                  <option value="light">Claro</option>
                  <option value="dark">Oscuro</option>
                  <option value="system">Sistema</option>
                </select>
              </label>
              <p class="rb-hint">
                También puedes cambiarlo con el botón de sol/luna/sistema en la
                barra superior.
              </p>

              <label class="rb-check">
                <input type="checkbox" bind:checked={cfg.ui_sounds} />
                Sonidos de interfaz
              </label>
              <p class="rb-hint">
                Toques graves al capturar y al dictar (tipo vibración suave).
              </p>
            </div>
          {:else if activeSection === "shortcuts"}
            <div class="rb-settings-group">
              <div>
                <p class="mb-2 text-xs font-medium" style="color: var(--rb-muted)">
                  Atajo de grabación
                </p>
                <HotkeyCapture
                  value={cfg.global_shortcut}
                  defaultValue="CmdOrCtrl+Shift+R"
                  ariaLabel="Cambiar atajo de grabación"
                  onChange={(sc) => {
                    if (cfg) cfg.global_shortcut = sc;
                  }}
                />
              </div>

              <label class="rb-label">
                Modo de dictado
                <select class="rb-field" bind:value={cfg.dictation_mode}>
                  <option value="push_to_talk">Push-to-talk (mantener)</option>
                  <option value="toggle">Toggle (pulsar para iniciar/parar)</option>
                </select>
              </label>

              <div>
                <p class="mb-2 text-xs font-medium" style="color: var(--rb-muted)">
                  Atajo de dictado
                </p>
                <HotkeyCapture
                  value={cfg.dictation_shortcut}
                  defaultValue="CmdOrCtrl+Shift+D"
                  ariaLabel="Cambiar atajo de dictado"
                  onChange={(sc) => {
                    if (cfg) cfg.dictation_shortcut = sc;
                  }}
                />
                <p class="rb-hint mt-1.5">
                  {#if cfg.dictation_mode === "push_to_talk"}
                    Mantén el atajo para hablar; al soltar, transcribe y pega.
                  {:else}
                    Pulsa para empezar, pulsa otra vez para transcribir y pegar.
                  {/if}
                  El botón de la pill siempre funciona en modo toggle.
                </p>
              </div>

              <div>
                <p class="mb-2 text-xs font-medium" style="color: var(--rb-muted)">
                  Traer pill al cursor
                </p>
                <HotkeyCapture
                  value={cfg.summon_pill_shortcut}
                  defaultValue="CmdOrCtrl+Shift+P"
                  ariaLabel="Cambiar atajo para traer la pill al cursor"
                  onChange={(sc) => {
                    if (cfg) cfg.summon_pill_shortcut = sc;
                  }}
                />
                <p class="rb-hint mt-1.5">
                  Muestra la pill (si estaba oculta) y la anima hasta el puntero del
                  mouse.
                </p>
              </div>

              <div>
                <p class="mb-2 text-xs font-medium" style="color: var(--rb-muted)">
                  Historial de clipboard
                </p>
                <HotkeyCapture
                  value={cfg.clipboard_shortcut}
                  defaultValue="CmdOrCtrl+Shift+V"
                  ariaLabel="Cambiar atajo del historial de clipboard"
                  onChange={(sc) => {
                    if (cfg) cfg.clipboard_shortcut = sc;
                  }}
                />
                <p class="rb-hint mt-1.5">
                  Trae la pill al cursor y abre el panel con el historial local
                  (texto e imágenes).
                </p>
              </div>
            </div>
          {:else if activeSection === "audio"}
            <div class="rb-settings-group">
              <div class="flex flex-wrap items-end justify-between gap-2">
                <p class="text-xs font-medium" style="color: var(--rb-muted)">
                  Entrada y salida
                </p>
                <div class="flex flex-wrap gap-2">
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
                <p class="text-xs font-medium" style="color: var(--rb-text)">
                  Perfil para reuniones
                </p>
                <p class="rb-hint mt-1">
                  Elige cómo equilibrar calidad, comodidad y las pistas que se
                  graban.
                </p>
                <div class="mt-3 flex flex-wrap gap-2" role="group" aria-label="Perfil de audio">
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
                  Recomendado: micrófono interno o USB + salida Bluetooth. Así el
                  auricular conserva el perfil estéreo A2DP.
                </p>
              </div>

              <label class="rb-label">
                Micrófono (entrada)
                <select class="rb-field" bind:value={cfg.mic_device_id}>
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
              {:else if bluetoothMicActive && !cfg.speakers_mode}
                <div class="rb-banner rb-banner-warn" role="status">
                  <p class="text-xs font-medium">
                    Esta entrada puede activar Hands-Free
                  </p>
                  <p class="rb-hint mt-1">
                    Al iniciar una grabación te pediremos confirmación. Usa
                    «Proteger calidad» para mantener el audio estéreo del auricular.
                  </p>
                </div>
              {/if}

              <label class="rb-label">
                Altavoces / auriculares (salida)
                <select class="rb-field" bind:value={cfg.output_device_id}>
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
                      Windows está usando una combinación que puede activar
                      Hands-Free. Aplica «Proteger calidad» y repite la prueba.
                    </p>
                  {/if}
                </div>
              {/if}

              {#if devicesError}
                <span class="rb-chip rb-chip-warn">{devicesError}</span>
              {:else}
                <p class="rb-hint">
                  {inputDevices.length} entrada(s), {outputDevices.length} salida(s)
                  detectadas. Entrada: grabación y dictado. Salida: audio del
                  sistema («Otros») y beep.
                </p>
                {#if fewMicsNoHeadset}
                  <span class="rb-chip rb-chip-warn">
                    No aparece un micrófono Bluetooth. En Windows: Configuración →
                    Sistema → Sonido → Entrada, elige el perfil Hands-Free / Headset
                    Microphone del auricular (no solo «Auriciculares» A2DP, que es
                    solo salida). Luego «Detectar de nuevo».
                  </span>
                {:else}
                  <p class="rb-hint">
                    Si conectas auriculares Bluetooth, Windows suele exponer el
                    micrófono solo en Hands-Free («Headset Microphone» / «Hands-Free
                    AG Audio»). A2DP es solo salida. Guarda Ajustes para aplicar.
                  </p>
                  <p class="rb-hint">
                    Al grabar con el micrófono del auricular, Windows cambia a perfil
                    Hands-Free y baja la calidad del audio que escuchas (mono, ~16 kHz).
                    Al detener la grabación vuelve a la normalidad. Para evitarlo: usa
                    micrófono interno + salida Bluetooth, o activa Modo parlantes (solo
                    «otros»).
                  </p>
                {/if}
              {/if}
            </div>
          {:else if activeSection === "transcription"}
            <div class="rb-settings-group">
              <label class="rb-label">
                Idioma
                <select class="rb-field" bind:value={cfg.language}>
                  <option value="es">Español (recomendado)</option>
                  <option value="auto">Autodetectar (puede fallar con ruido)</option>
                  <option value="en">Inglés</option>
                  <option value="pt">Portugués</option>
                  <option value="fr">Francés</option>
                </select>
              </label>
              <p class="rb-hint">
                Si la reunión es en español, deja «Español». Autodetectar a menudo
                inventa inglés con audio corto o ruidoso.
              </p>
              <label class="rb-label">
                Modelo para reuniones
                <select class="rb-field" bind:value={cfg.whisper_model}>
                  {#each models as m (m.id)}
                    <option value={m.id}>
                      {m.display_name}{m.downloaded ? " · listo" : ""}
                    </option>
                  {/each}
                </select>
              </label>
              <p class="rb-hint">
                Base es el perfil local rápido. Elige Small si priorizas precisión
                en reuniones largas y tienes más memoria disponible.
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
                        class="rb-btn rb-btn-primary"
                        onclick={() => startDownload(row.model.id)}
                        disabled={Boolean(downloading)}
                        >Descargar</button
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

              <label class="rb-check">
                <input
                  type="checkbox"
                  bind:checked={cfg.auto_transcribe_after_recording}
                />
                Transcribir automáticamente al terminar
              </label>
              <p class="rb-hint">
                Usa las pistas completas para obtener frases más coherentes. Se
                ejecuta en segundo plano; si falla, el audio queda guardado y puedes
                reintentar.
              </p>
            </div>

            <div class="rb-settings-group">
              <label class="rb-check">
                <input type="checkbox" bind:checked={cfg.live_transcription} />
                Vista previa en vivo (experimental)
              </label>
              <p class="rb-hint">
                Muestra subtítulos provisionales mientras grabas. Puede tener retraso
                o frases incompletas; nunca se guarda ni reemplaza la transcripción
                final.
              </p>

              {#if cfg.live_transcription}
                <label class="rb-label">
                  Motor en vivo
                  <select class="rb-field" bind:value={cfg.live_engine}>
                    <option value="local">Local (Whisper en el PC)</option>
                    <option value="groq">Groq Whisper (nube)</option>
                  </select>
                </label>
                {#if cfg.live_engine === "groq"}
                  <p class="rb-hint">
                    Aviso de privacidad: al usar Groq, el audio de la reunión se
                    envía a la nube de Groq para transcribir. En modo local el audio
                    no sale de tu PC.
                  </p>
                  <span
                    class="rb-chip {secrets?.providers?.groq
                      ? 'rb-chip-ok'
                      : 'rb-chip-warn'}"
                  >
                    {secrets?.providers?.groq
                      ? "Key configurada en el llavero"
                      : "Sin key — se usará Whisper local"}
                  </span>
                {:else}
                  <p class="rb-hint">
                    El motor local procesa el audio en tu PC. Groq es más rápido pero
                    envía audio a la nube con tu API key.
                  </p>
                {/if}
                {#if cfg.live_engine === "local" || !secrets?.providers?.groq}
                  <label class="rb-label">
                    Modelo para transcripción en vivo
                    <select class="rb-field" bind:value={cfg.live_whisper_model}>
                      {#each models as m (m.id)}
                        <option value={m.id}>
                          {m.display_name}{m.downloaded ? " · listo" : ""}
                        </option>
                      {/each}
                    </select>
                  </label>
                  <p class="rb-hint">
                    Small equilibra latencia y precisión en ventanas cortas. Base es
                    más ligero si la máquina va justa de memoria. También se usa si
                    eliges Groq sin API key.
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
                            class="rb-btn rb-btn-primary"
                            onclick={() => startDownload(row.model.id)}
                            disabled={Boolean(downloading)}
                            >Descargar</button
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
                      placeholder={secrets?.providers?.groq
                        ? "•••••••• (llavero — escribe para reemplazar)"
                        : "Pega tu API key de Groq"}
                      bind:value={groqDictationKey}
                      autocomplete="off"
                    />
                  </label>
                  <p class="rb-hint">Crea tu cuenta y API key en groq.com</p>
                {/if}
              {/if}
            </div>
          {:else if activeSection === "dictation"}
            <div class="rb-settings-group">
              <label class="rb-label">
                Motor de dictado
                <select class="rb-field" bind:value={cfg.dictation_backend}>
                  <option value="local">Local (Whisper en el PC)</option>
                  <option value="groq">Groq Whisper (nube, más rápido)</option>
                </select>
              </label>
              <p class="rb-hint">
                Groq es el modo rápido (nube) y requiere tu propia API key. Sin ella
                se usa Whisper local. El audio sale del PC solo en modo Groq.
              </p>

              <label class="rb-label">
                Micrófono para dictado
                <select class="rb-field" bind:value={cfg.dictation_mic_device_id}>
                  <option value="">Usar el micrófono de reuniones</option>
                  {#each inputDevices as device (device.id)}
                    <option value={device.id}>
                      {device.name}{deviceFormat(device)}
                    </option>
                  {/each}
                </select>
              </label>
              <p class="rb-hint">
                Puedes usar el micrófono Bluetooth solo para dictados cortos y
                mantener un micrófono interno o USB para reuniones.
              </p>

              {#if cfg.dictation_backend === "groq"}
                <label class="rb-label">
                  API key Groq (necesaria para nube)
                  <input
                    type="password"
                    class="rb-field"
                    placeholder={secrets?.providers?.groq
                      ? "•••••••• (llavero — escribe para reemplazar)"
                      : "Pega tu API key de Groq"}
                    bind:value={groqDictationKey}
                    autocomplete="off"
                  />
                </label>
                <p class="rb-hint">Crea tu cuenta y API key en groq.com</p>
                <span
                  class="rb-chip {secrets?.providers?.groq
                    ? 'rb-chip-ok'
                    : 'rb-chip-warn'}"
                >
                  {secrets?.providers?.groq
                    ? "Key configurada en el llavero"
                    : "Sin key — se usará Whisper local"}
                </span>
              {/if}

              {#if cfg.dictation_backend !== "groq"}
                <label class="rb-label">
                  Modelo local para dictado
                  <select class="rb-field" bind:value={cfg.dictation_whisper_model}>
                    {#each models as m (m.id)}
                      <option value={m.id}>
                        {m.display_name}{m.downloaded ? " · listo" : ""}
                      </option>
                    {/each}
                  </select>
                </label>
                <p class="rb-hint">
                  Base es más rápido para frases cortas; puedes subir a Small si
                  necesitas más precisión.
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
                          class="rb-btn rb-btn-primary"
                          onclick={() => startDownload(row.model.id)}
                          disabled={Boolean(downloading)}
                          >Descargar</button
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
          {:else if activeSection === "summary"}
            <div class="rb-settings-group">
          <label class="rb-label">
            Proveedor
            <select
              class="rb-field"
              value={cfg.summary_backend}
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
              <input
                class="rb-field"
                bind:value={cfg.summary_model}
                placeholder={selectedProvider.default_model ||
                  "nombre-del-modelo"}
              />
            </label>

            {#if selectedProvider.kind !== "claude"}
              <label class="rb-label">
                URL base
                <input
                  class="rb-field"
                  bind:value={cfg.summary_base_url}
                  placeholder={selectedProvider.default_base_url ||
                    "https://…/v1"}
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
                    : "pega tu API key"}
                  bind:value={apiKeyInput}
                  autocomplete="off"
                />
              </label>
              {#if hasProviderKey}
                <button class="rb-btn rb-btn-danger" onclick={clearProviderKey}
                  >Eliminar API key</button
                >
              {:else}
                <p class="rb-hint" style="color: var(--rb-warn)">
                  Necesitas una API key para este proveedor.
                </p>
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
          {:else if activeSection === "recording"}
            <div class="rb-settings-group">
              <label class="rb-check">
                <input type="checkbox" bind:checked={cfg.speakers_mode} />
                <span>
                  <span class="font-medium">Modo parlantes</span>
                  <span class="rb-hint mt-0.5 block">
                    Sin auriculares, el mic captura eco. Activa esto para grabar solo
                    «otros» (audio del sistema).
                  </span>
                </span>
              </label>

              <label class="rb-label">
                Supresión de ruido (mic)
                <select class="rb-field" bind:value={cfg.noise_suppression}>
                  <option value="off">Desactivada (reuniones)</option>
                  <option value="low">Baja</option>
                  <option value="medium">Media (recomendado en notebook)</option>
                  <option value="high">Alta (puede silenciar la voz)</option>
                </select>
              </label>
              <p class="rb-hint">
                Solo afecta a «Yo» en grabaciones. El dictado usa al menos media
                (ventiladores / ruido de fondo). Alta puede dejar la pista casi en
                silencio. No afecta a «Otros».
              </p>

              <label class="rb-label" class:opacity-50={cfg.speakers_mode}>
                Qué grabar
                <select
                  class="rb-field"
                  bind:value={cfg.record_tracks}
                  disabled={cfg.speakers_mode}
                >
                  <option value="both">Yo + otros</option>
                  <option value="mic">Solo yo</option>
                  <option value="system">Solo otros</option>
                </select>
              </label>

              <label class="rb-label" class:opacity-50={cfg.speakers_mode}>
                Qué transcribir
                <select
                  class="rb-field"
                  bind:value={cfg.transcribe_tracks}
                  disabled={cfg.speakers_mode}
                >
                  <option value="both">Ambas pistas</option>
                  <option value="mic">Solo yo</option>
                  <option value="system">Solo otros</option>
                </select>
                <span class="rb-hint"
                  >Útil si ya grabaste ambas y solo quieres procesar una.</span
                >
              </label>

              {#if cfg.speakers_mode}
                <span class="rb-chip rb-chip-warn"
                  >Modo parlantes: solo «otros»</span
                >
              {/if}

              <div class="space-y-2.5 pt-1">
                <label class="rb-check">
                  <input type="checkbox" bind:checked={cfg.beep_on_start} />
                  Sonido al grabar (aviso de consentimiento)
                </label>
                <label class="rb-check">
                  <input type="checkbox" bind:checked={cfg.autostart} />
                  Iniciar con el sistema (bandeja)
                </label>
                <label class="rb-check">
                  <input type="checkbox" bind:checked={cfg.show_pill} />
                  Mostrar pill flotante
                </label>
                <label class="rb-check">
                  <input type="checkbox" bind:checked={cfg.detect_meetings} />
                  <span>
                    <span class="font-medium">Detectar reuniones abiertas</span>
                    <span class="rb-hint mt-0.5 block">
                      Revisa localmente títulos y procesos de Teams, Zoom, Meet y
                      Webex. Solo ofrece grabar; nunca inicia por sí solo.
                    </span>
                  </span>
                </label>
              </div>

              <div class="rb-settings-block">
                <p class="text-xs font-medium" style="color: var(--rb-text)">
                  Conservación de datos
                </p>
                <label class="rb-label mt-3">
                  Eliminar grabaciones con más de
                  <select
                    class="rb-field"
                    bind:value={cfg.retention_days}
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
                <label class="rb-check mt-3" class:opacity-50={cfg.retention_days === 0}>
                  <input
                    type="checkbox"
                    bind:checked={cfg.retention_auto_cleanup}
                    disabled={cfg.retention_days === 0}
                  />
                  Limpiar automáticamente al iniciar
                </label>
                <p class="rb-hint mt-2">
                  Incluye audio, transcripción y resumen. Los archivos exportados
                  fuera de Atic no se eliminan.
                </p>
                <div class="mt-3 flex flex-wrap gap-2">
                  <button
                    type="button"
                    class="rb-btn rb-btn-soft"
                    onclick={reviewRetention}
                    disabled={retentionBusy || cfg.retention_days === 0}
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
                  <p class="rb-hint mt-2" style="color: var(--rb-record)" role="alert">
                    {retentionError}
                  </p>
                {/if}
              </div>
            </div>
          {:else if activeSection === "capturas"}
            <div class="rb-settings-group">
              <div>
                <p class="mb-2 text-xs font-medium" style="color: var(--rb-muted)">
                  Atajo de captura
                </p>
                <HotkeyCapture
                  value={cfg.screenshot_shortcut}
                  defaultValue="CmdOrCtrl+Shift+4"
                  ariaLabel="Cambiar atajo para abrir la selección de captura"
                  onChange={(sc) => {
                    if (cfg) cfg.screenshot_shortcut = sc;
                  }}
                />
                <p class="rb-hint mt-1.5">
                  Abre la selección: clic en una ventana, arrastra una región o pulsa
                  Espacio para el monitor. Esc cancela.
                </p>
              </div>

              <label class="rb-label">
                Lado del shelf
                <select class="rb-input" bind:value={cfg.capture_shelf_side}>
                  <option value="right">Derecha</option>
                  <option value="left">Izquierda</option>
                </select>
              </label>

              <label class="rb-label">
                Retracción del shelf (segundos)
                <input
                  class="rb-input"
                  type="number"
                  min="5"
                  max="120"
                  bind:value={cfg.capture_shelf_timeout_seconds}
                />
              </label>

              <label class="rb-label">
                Conservar capturas (horas, 0 = sin límite)
                <input
                  class="rb-input"
                  type="number"
                  min="0"
                  max="720"
                  bind:value={cfg.capture_retention_hours}
                />
              </label>

              <label class="flex items-center gap-2 text-sm">
                <input type="checkbox" bind:checked={cfg.capture_include_cursor} />
                Incluir el cursor en la captura
              </label>

              <label class="rb-label">
                Al hacer clic en la miniatura
                <select class="rb-input" bind:value={cfg.capture_click_action}>
                  <option value="preview">Abrir vista previa</option>
                  <option value="location">Abrir ubicación</option>
                </select>
              </label>

              <div class="flex flex-wrap gap-2">
                <button
                  type="button"
                  class="rb-btn rb-btn-ghost text-xs"
                  onclick={openCapturesDir}
                >
                  Abrir carpeta
                </button>
                <button
                  type="button"
                  class="rb-btn rb-btn-ghost text-xs"
                  onclick={runCaptureCleanup}
                >
                  Limpiar capturas ahora
                </button>
              </div>
              {#if captureCleanupMsg}
                <p class="rb-hint mt-1">{captureCleanupMsg}</p>
              {/if}
            </div>
          {:else if activeSection === "mail"}
            <div class="rb-settings-group">
              <label class="rb-label">
                Backend
                <select class="rb-field" bind:value={cfg.mail_backend}>
                  <option value="mailto">Abrir cliente (mailto)</option>
                  <option value="smtp">SMTP directo</option>
                </select>
              </label>

              {#if cfg.mail_backend === "smtp"}
                <label class="rb-label">
                  Host
                  <input
                    class="rb-field"
                    bind:value={cfg.smtp_host}
                    placeholder="smtp.ejemplo.com"
                  />
                </label>
                <div class="rb-settings-two-col">
                  <label class="rb-label">
                    Puerto
                    <input
                      type="number"
                      class="rb-field"
                      bind:value={cfg.smtp_port}
                    />
                  </label>
                  <label class="rb-check items-end pb-2">
                    <input type="checkbox" bind:checked={cfg.smtp_use_tls} />
                    STARTTLS
                  </label>
                </div>
                <label class="rb-label">
                  Usuario
                  <input class="rb-field" bind:value={cfg.smtp_username} />
                </label>
                <label class="rb-label">
                  Remitente (From)
                  <input
                    class="rb-field"
                    bind:value={cfg.smtp_from}
                    placeholder="opcional"
                  />
                </label>
                <label class="rb-label">
                  Contraseña
                  <input
                    type="password"
                    class="rb-field"
                    placeholder={secrets.has_smtp_password
                      ? "•••••••• (ya guardada)"
                      : ""}
                    bind:value={smtpPassword}
                    autocomplete="off"
                  />
                </label>
                {#if secrets.has_smtp_password}
                  <button class="rb-btn rb-btn-danger" onclick={clearSmtpPassword}
                    >Eliminar contraseña</button
                  >
                {/if}
              {/if}
            </div>
          {:else if activeSection === "updates"}
            <div class="rb-settings-group">
              <p class="rb-hint">
                Comprueba si hay una versión nueva publicada en GitHub Releases.
                Requiere una build firmada (clave configurada en el release).
              </p>
              <div class="flex flex-wrap items-center gap-3">
                <button
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
                    class="rb-btn rb-btn-soft"
                    onclick={installPendingUpdate}
                  >
                    Descargar e instalar {updateUi.update.version}
                  </button>
                {/if}
              </div>
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
                {#if updateUi.percent !== null}
                  <div class="rb-level-track">
                    <div
                      class="rb-level-fill rb-level-mic"
                      style="width: {updateUi.percent}%"
                    ></div>
                  </div>
                {/if}
              {:else if updateUi.kind === "error"}
                <p class="rb-hint" style="color: var(--rb-warn)">
                  {updateUi.message}
                </p>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    </div>
  {/if}

  {#snippet actions()}
      {#if cfg && secrets}
        <button class="rb-btn rb-btn-ghost" onclick={onClose}>Cancelar</button>
        <button
          class="rb-btn rb-btn-primary"
          onclick={save}
          disabled={saving}
        >
          {saving ? "Guardando…" : "Guardar"}
        </button>
      {/if}
    {/snippet}
</ModalShell>
