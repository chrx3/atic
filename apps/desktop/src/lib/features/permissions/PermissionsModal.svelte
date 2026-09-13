<script lang="ts">
  /**
   * Pantalla de inicio de macOS: pide micrófono, pantalla y accesibilidad.
   *
   * Se puede cerrar («Ahora no»): mientras falte alguno, vuelve en el próximo
   * arranque. Los permisos se conceden en Ajustes, fuera de Atic, así que
   * mientras está visible el estado se refresca por intervalo y al recuperar
   * el foco de la ventana.
   */
  import { onMount } from "svelte";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { t } from "$domain/i18n.svelte";
  import {
    macOpenPrivacyPane,
    macPermissionsStatus,
    macRequestPermission,
    type MacPermissionKind,
    type MacPermissions,
  } from "$ipc/permissions";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";
  import Modal from "$ui/Modal.svelte";
  import { hasMissingPermissions, permissionRows } from "./model";

  let { onClose }: { onClose: () => void } = $props();

  let status = $state<MacPermissions | null>(null);
  let busy = $state<MacPermissionKind | null>(null);
  let restarting = $state(false);

  const rows = $derived(status ? permissionRows(status) : []);
  const missing = $derived(status ? hasMissingPermissions(status) : true);

  async function refresh() {
    try {
      status = await macPermissionsStatus();
    } catch {
      // Fuera de Tauri (labs del navegador): la pantalla no aplica.
    }
  }

  async function request(kind: MacPermissionKind) {
    if (busy) return;
    busy = kind;
    try {
      await macRequestPermission(kind);
      await refresh();
    } catch {
      /* el comando solo existe en macOS */
    } finally {
      busy = null;
    }
  }

  async function openSettings(kind: MacPermissionKind) {
    try {
      await macOpenPrivacyPane(kind);
    } catch {
      /* noop */
    }
  }

  async function restart() {
    if (restarting) return;
    restarting = true;
    await relaunch();
  }

  onMount(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 1500);
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    };
  });
</script>

<Modal
  title={t("permissions.title")}
  subtitle={t("permissions.subtitle")}
  size="md"
  {onClose}
>
  <div class="flex flex-col gap-4">
    <p class="max-w-[60ch] text-sm leading-relaxed text-muted">
      {t("permissions.intro")}
    </p>

    <ul class="flex list-none flex-col gap-2">
      {#each rows as row (row.kind)}
        <li
          class="flex items-start justify-between gap-3 rounded-sm border border-line
                 px-3 py-2"
        >
          <div class="flex min-w-0 flex-col gap-0.5">
            <span class="flex items-center gap-2 text-sm font-medium text-text">
              {t(`permissions.${row.kind}.label`)}
              {#if row.granted}
                <Chip tone="ok">{t("permissions.granted")}</Chip>
              {:else}
                <Chip tone="warn">{t("permissions.pending")}</Chip>
              {/if}
            </span>
            <span class="text-xs text-faint">{t(`permissions.${row.kind}.hint`)}</span>
          </div>

          {#if !row.granted}
            <div class="flex shrink-0 items-center gap-1.5">
              {#if row.action === "allow"}
                <Button
                  variant="primary"
                  size="sm"
                  loading={busy === row.kind}
                  onclick={() => void request(row.kind)}
                >
                  {t("permissions.allow")}
                </Button>
              {/if}
              <Button
                variant="ghost"
                size="sm"
                onclick={() => void openSettings(row.kind)}
              >
                {t("permissions.openSettings")}
              </Button>
            </div>
          {/if}
        </li>
      {/each}
    </ul>

    {#if status && !missing}
      <Banner tone="info" title={t("permissions.allGrantedTitle")}>
        {t("permissions.allGrantedBody")}
      </Banner>
    {:else}
      <p class="max-w-[60ch] text-xs leading-relaxed text-faint">
        {t("permissions.restartHint")}
      </p>
    {/if}
  </div>

  {#snippet actions()}
    <div class="flex w-full items-center justify-between gap-2">
      <Button variant="ghost" size="sm" onclick={() => void refresh()}>
        {t("permissions.retry")}
      </Button>
      <div class="flex gap-2">
        {#if missing}
          <Button
            variant="ghost"
            size="sm"
            disabled={restarting}
            onclick={() => void restart()}
          >
            {t("permissions.restart")}
          </Button>
        {/if}
        <Button variant="primary" size="sm" onclick={onClose}>
          {t("permissions.done")}
        </Button>
      </div>
    </div>
  {/snippet}
</Modal>
