<script lang="ts">
  /**
   * La lista de permisos de macOS, con su estado en vivo.
   *
   * Tiene dos casas: un paso del primer uso (antes de practicar, porque la
   * práctica dicta y pega) y la ventana que vuelve al arrancar si falta algo.
   * Los permisos se conceden en Ajustes, fuera de Atic, así que mientras está
   * visible el estado se refresca por intervalo y al recuperar el foco.
   */
  import { onMount } from "svelte";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { t } from "$domain/i18n.svelte";
  import {
    macOpenPrivacyPane,
    macPermissionsStatus,
    macRequestPermission,
    type MacPermissions,
  } from "$ipc/permissions";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";
  import {
    hasMissingPermissions,
    permissionRows,
    type PermissionRowKind,
  } from "./model";

  let {
    missing = $bindable(true),
  }: {
    /** Falta alguno de los obligatorios (las notificaciones no cuentan). */
    missing?: boolean;
  } = $props();

  let status = $state<MacPermissions | null>(null);
  let notifications = $state<boolean | null>(null);
  /** Se pidió y el sistema no mostró diálogo: ya estaban denegadas. */
  let notificationsBlocked = $state(false);
  let busy = $state<PermissionRowKind | null>(null);
  let restarting = $state(false);

  const rows = $derived(status ? permissionRows(status, notifications) : []);

  $effect(() => {
    missing = status ? hasMissingPermissions(status) : true;
  });

  async function notificationsApi() {
    return import("@tauri-apps/plugin-notification");
  }

  export async function refresh() {
    try {
      status = await macPermissionsStatus();
    } catch {
      // Fuera de Tauri (labs del navegador): la lista no aplica.
    }
    try {
      notifications = await (await notificationsApi()).isPermissionGranted();
    } catch {
      notifications = null;
    }
  }

  async function request(kind: PermissionRowKind) {
    if (busy) return;
    busy = kind;
    try {
      if (kind === "notifications") {
        const answer = await (await notificationsApi()).requestPermission();
        notificationsBlocked = answer !== "granted";
      } else {
        await macRequestPermission(kind);
      }
      await refresh();
    } catch {
      /* el comando solo existe en macOS */
    } finally {
      busy = null;
    }
  }

  async function openSettings(kind: PermissionRowKind) {
    if (kind === "notifications") return;
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

<div class="flex flex-col gap-3">
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
            {:else if row.optional}
              <Chip>{t("permissions.optional")}</Chip>
            {:else}
              <Chip tone="warn">{t("permissions.pending")}</Chip>
            {/if}
          </span>
          <span class="text-xs leading-relaxed text-faint">
            {t(`permissions.${row.kind}.hint`)}
          </span>
          {#if !row.granted && row.kind === "accessibility"}
            <span class="text-xs leading-relaxed text-faint">
              {t("permissions.accessibility.how")}
            </span>
          {/if}
          {#if !row.granted && row.kind === "notifications" && notificationsBlocked}
            <span class="text-xs leading-relaxed text-warn">
              {t("permissions.notifications.blocked")}
            </span>
          {/if}
        </div>

        {#if !row.granted}
          <div class="flex shrink-0 items-center gap-1.5">
            {#if row.action === "allow"}
              <Button
                variant={row.optional ? "soft" : "primary"}
                size="sm"
                loading={busy === row.kind}
                onclick={() => void request(row.kind)}
              >
                {t("permissions.allow")}
              </Button>
            {/if}
            {#if row.kind !== "notifications"}
              <Button
                variant={row.action === "settings" ? "primary" : "ghost"}
                size="sm"
                onclick={() =>
                  void (row.kind === "accessibility"
                    ? request(row.kind)
                    : openSettings(row.kind))}
              >
                {t("permissions.openSettings")}
              </Button>
            {/if}
          </div>
        {/if}
      </li>
    {/each}
  </ul>

  {#if status && !missing}
    <Banner tone="info" title={t("permissions.allGrantedTitle")}>
      {t("permissions.allGrantedBody")}
    </Banner>
  {:else if status}
    <div class="flex items-center justify-between gap-3">
      <p class="max-w-[52ch] text-xs leading-relaxed text-faint">
        {t("permissions.restartHint")}
      </p>
      <Button
        variant="ghost"
        size="sm"
        disabled={restarting}
        onclick={() => void restart()}
      >
        {t("permissions.restart")}
      </Button>
    </div>
  {/if}
</div>
