<script lang="ts">
  /**
   * Quiénes somos, qué versión corre y si hay un instalador nuevo en GitHub.
   */
  import { getVersion } from "@tauri-apps/api/app";
  import { toastError } from "$domain/toasts.svelte";
  import {
    checkAppUpdate,
    friendlyUpdateError,
    GITHUB_RELEASES_URL,
    GITHUB_REPO_URL,
    installAppUpdateAndRelaunch,
    type AppUpdate,
  } from "$ipc/updates";
  import { openExternalUrl } from "$ipc/config";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";
  import ProgressBar from "$ui/ProgressBar.svelte";

  type UpdateUi =
    | { kind: "idle" }
    | { kind: "checking" }
    | { kind: "up_to_date" }
    | { kind: "available"; update: AppUpdate }
    | { kind: "downloading"; version: string; percent: number | null }
    | { kind: "error"; message: string };

  let version = $state("");
  let updateUi = $state<UpdateUi>({ kind: "idle" });
  let pending = $state<AppUpdate | null>(null);

  const busy = $derived(updateUi.kind === "checking" || updateUi.kind === "downloading");
  const buildLabel = $derived(
    version
      ? `${navigator.userAgent.includes("Mac") ? "macOS" : "Windows"} · v${version}`
      : "…",
  );

  $effect(() => {
    void getVersion()
      .then((value) => {
        version = value;
      })
      .catch(toastError);
  });

  async function searchUpdates() {
    if (busy) return;
    updateUi = { kind: "checking" };
    pending = null;
    try {
      const update = await checkAppUpdate();
      if (!update) {
        updateUi = { kind: "up_to_date" };
        return;
      }
      pending = update;
      updateUi = { kind: "available", update };
    } catch (error) {
      updateUi = { kind: "error", message: friendlyUpdateError(error) };
    }
  }

  async function installPending() {
    const update = pending;
    if (!update || updateUi.kind === "downloading") return;
    let downloaded = 0;
    let contentLength: number | null = null;
    updateUi = { kind: "downloading", version: update.version, percent: null };
    try {
      await installAppUpdateAndRelaunch(update, (event) => {
        if (event.event === "Started") {
          contentLength = event.data.contentLength ?? null;
          updateUi = { kind: "downloading", version: update.version, percent: 0 };
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          const percent =
            contentLength && contentLength > 0
              ? Math.round((downloaded / contentLength) * 100)
              : null;
          updateUi = { kind: "downloading", version: update.version, percent };
        } else if (event.event === "Finished") {
          updateUi = { kind: "downloading", version: update.version, percent: 100 };
        }
      });
    } catch (error) {
      updateUi = { kind: "error", message: friendlyUpdateError(error) };
    }
  }
</script>

<div class="flex flex-col gap-5">
  <div
    class="flex flex-col items-center gap-1.5 rounded-sm border border-line px-4 py-5
           text-center"
  >
    <p class="text-lg font-semibold text-text">Atic</p>
    <p class="max-w-[40ch] text-xs leading-relaxed text-muted">
      Grabá llamadas, transcribí y resumí en tu PC.
    </p>
    {#if version}
      <Chip tone="neutral">v{version}</Chip>
    {/if}
  </div>

  <SettingsGroup title="Detalles">
    <SettingsRow label="Compilación">
      {#snippet control()}
        <p class="font-mono text-sm text-text" data-numeric>{buildLabel}</p>
      {/snippet}
    </SettingsRow>
    <SettingsRow label="Identificador">
      {#snippet control()}
        <p class="font-mono text-sm text-text">com.ciat.atic</p>
      {/snippet}
    </SettingsRow>
    <SettingsRow label="Licencia">
      {#snippet control()}
        <p class="text-sm text-text">MIT</p>
      {/snippet}
    </SettingsRow>
    <SettingsRow label="Código" hint="El instalador nuevo también vive en Releases.">
      {#snippet control()}
        <Button
          variant="soft"
          size="sm"
          full
          onclick={() => void openExternalUrl(GITHUB_REPO_URL).catch(toastError)}
        >
          GitHub
        </Button>
      {/snippet}
    </SettingsRow>
  </SettingsGroup>

  {#if updateUi.kind === "available"}
    <Banner tone="info" title="Hay una versión nueva: {updateUi.update.version}">
      Se descarga el instalador, se aplica y Atic vuelve a abrir.
    </Banner>
  {:else if updateUi.kind === "up_to_date"}
    <Banner tone="info" title="Estás al día" />
  {:else if updateUi.kind === "error"}
    <Banner tone="warn" title="No se pudo consultar">
      {updateUi.message}
    </Banner>
  {/if}

  <SettingsGroup
    title="Actualizaciones"
    hint="Mira el último release de GitHub. Si hay uno más nuevo, aparece el botón para instalarlo y reiniciar."
  >
    <SettingsRow bare>
      {#snippet control()}
        <div class="flex flex-col gap-2">
          {#if updateUi.kind === "available"}
            <Button
              variant="primary"
              size="sm"
              full
              disabled={busy}
              onclick={() => void installPending()}
            >
              Actualizar a {updateUi.update.version}
            </Button>
          {/if}
          <Button
            variant={updateUi.kind === "available" ? "soft" : "primary"}
            size="sm"
            full
            loading={updateUi.kind === "checking"}
            disabled={busy}
            onclick={() => void searchUpdates()}
          >
            {updateUi.kind === "checking" ? "Buscando…" : "Buscar actualizaciones"}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            full
            onclick={() => void openExternalUrl(GITHUB_RELEASES_URL).catch(toastError)}
          >
            Ver releases
          </Button>
        </div>
      {/snippet}
    </SettingsRow>

    {#if updateUi.kind === "downloading"}
      <SettingsRow bare>
        {#snippet control()}
          <ProgressBar
            value={(updateUi.percent ?? 0) / 100}
            indeterminate={updateUi.percent === null}
            label="Descargando {updateUi.version}"
            tone="ok"
          />
        {/snippet}
      </SettingsRow>
    {/if}
  </SettingsGroup>
</div>
