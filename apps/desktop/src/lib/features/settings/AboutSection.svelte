<script lang="ts">
  /**
   * Quiénes somos, qué versión corre y si hay un instalador nuevo en GitHub.
   */
  import { getVersion } from "@tauri-apps/api/app";
  import { appUpdate } from "$domain/appUpdate.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { GITHUB_RELEASES_URL, GITHUB_REPO_URL } from "$ipc/updates";
  import { openExternalUrl } from "$ipc/config";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";
  import ProgressBar from "$ui/ProgressBar.svelte";

  let version = $state("");

  const busy = $derived(appUpdate.checking || appUpdate.downloading);
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

  {#if appUpdate.pending && !appUpdate.downloading}
    <Banner tone="info" title="Hay una versión nueva: {appUpdate.version}">
      Se descarga el instalador, se aplica y Atic vuelve a abrir.
    </Banner>
  {:else if appUpdate.error}
    <Banner tone="warn" title="No se pudo consultar">
      {appUpdate.error}
    </Banner>
  {:else if appUpdate.checked && !appUpdate.checking && !appUpdate.pending}
    <Banner tone="info" title="Estás al día" />
  {/if}

  <SettingsGroup
    title="Actualizaciones"
    hint="Mira el último release de GitHub. Si hay uno más nuevo, aparece el botón para instalarlo y reiniciar."
  >
    <SettingsRow bare>
      {#snippet control()}
        <div class="flex flex-col gap-2">
          {#if appUpdate.pending && !appUpdate.downloading}
            <Button
              variant="primary"
              size="sm"
              full
              disabled={busy}
              onclick={() => void appUpdate.install()}
            >
              Actualizar a {appUpdate.version}
            </Button>
          {/if}
          <Button
            variant={appUpdate.pending ? "soft" : "primary"}
            size="sm"
            full
            loading={appUpdate.checking}
            disabled={busy}
            onclick={() => void appUpdate.check()}
          >
            {appUpdate.checking ? "Buscando…" : "Buscar actualizaciones"}
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

    {#if appUpdate.downloading}
      <SettingsRow bare>
        {#snippet control()}
          <ProgressBar
            value={(appUpdate.percent ?? 0) / 100}
            indeterminate={appUpdate.percent === null}
            label="Descargando {appUpdate.version}"
            tone="ok"
          />
        {/snippet}
      </SettingsRow>
    {/if}
  </SettingsGroup>
</div>
