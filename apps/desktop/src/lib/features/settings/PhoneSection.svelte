<script lang="ts">
  /**
   * El celular: vincularlo con un QR, ver si está conectado y desvincularlo.
   *
   * El canal (`phone_sync.rs`) no se abre hasta que alguien pide un QR o ya hay
   * celulares pareados: sin eso Atic no escucha en la red.
   */
  import { onMount } from "svelte";
  import { on } from "$ipc/events";
  import {
    phonePairAnswer,
    phonePairCancel,
    phonePairStart,
    phoneSetClipboard,
    phoneStatus,
    phoneUnpair,
    type PhonePairing,
    type PhoneStatus,
  } from "$ipc/phone";
  import { toastError } from "$domain/toasts.svelte";
  import { t } from "$domain/i18n.svelte";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Button from "$ui/Button.svelte";
  import Switch from "$ui/Switch.svelte";

  let status = $state<PhoneStatus | null>(null);
  let pairing = $state<PhonePairing | null>(null);
  let remaining = $state(0);
  let opening = $state(false);

  const countdown = $derived(
    `${Math.floor(remaining / 60)}:${String(remaining % 60).padStart(2, "0")}`,
  );

  onMount(() => {
    void phoneStatus()
      .then((s) => (status = s))
      .catch(toastError);

    let unlisten: (() => void) | undefined;
    void on("phone-sync", (next) => {
      // Llegó un celular nuevo mientras el QR estaba a la vista: ya se usó.
      if (pairing && next.devices.length > (status?.devices.length ?? 0)) {
        pairing = null;
      }
      status = next;
    }).then((fn) => (unlisten = fn));

    const timer = setInterval(() => {
      if (!pairing) return;
      remaining = Math.max(0, remaining - 1);
      if (remaining === 0) pairing = null;
    }, 1000);

    return () => {
      unlisten?.();
      clearInterval(timer);
      // Salir de Ajustes con el QR abierto lo invalida: nadie lo va a escanear.
      if (pairing) void phonePairCancel();
    };
  });

  async function startPairing() {
    opening = true;
    try {
      pairing = await phonePairStart();
      remaining = pairing.expiresInSecs;
    } catch (err) {
      toastError(err);
    } finally {
      opening = false;
    }
  }

  function cancelPairing() {
    pairing = null;
    void phonePairCancel().catch(toastError);
  }

  function unpair(id: string) {
    void phoneUnpair(id).catch(toastError);
  }

  function answer(deviceId: string, accept: boolean) {
    if (accept) pairing = null;
    void phonePairAnswer(deviceId, accept).catch(toastError);
  }

  function setClipboard(enabled: boolean) {
    void phoneSetClipboard(enabled).catch(toastError);
  }
</script>

<div class="flex flex-col gap-5">
  <SettingsGroup title={t("settings.phone.title")} hint={t("settings.phone.hint")}>
    {#if status?.pendingPair}
      {@const request = status.pendingPair}
      <!-- Tener el QR no basta: alguien pudo fotografiarlo. Se acepta acá. -->
      <div class="flex flex-col items-center gap-3 py-3 text-center">
        <p class="text-sm text-text">
          {t("settings.phone.approve", { name: request.deviceName })}
        </p>
        <p class="text-xs text-faint">{t("settings.phone.approveHint")}</p>
        <div class="flex gap-2">
          <Button variant="ghost" onclick={() => answer(request.deviceId, false)}>
            {t("settings.phone.reject")}
          </Button>
          <Button variant="primary" onclick={() => answer(request.deviceId, true)}>
            {t("settings.phone.accept")}
          </Button>
        </div>
      </div>
    {:else if pairing}
      <div class="flex flex-col items-center gap-3 py-3">
        <!-- El QR va sobre blanco siempre: con el tema oscuro las cámaras no lo leen. -->
        <img
          class="rounded-2xl bg-white p-2"
          src={`data:image/svg+xml;utf8,${encodeURIComponent(pairing.qrSvg)}`}
          alt={t("settings.phone.qrAlt")}
          width="236"
          height="236"
        />
        <p class="text-sm text-text">{t("settings.phone.scan")}</p>
        <p class="text-xs text-faint">{t("settings.phone.expires", { time: countdown })}</p>
        <Button variant="ghost" size="sm" onclick={cancelPairing}>
          {t("settings.phone.cancel")}
        </Button>
      </div>
    {:else}
      <SettingsRow label={t("settings.phone.pair")} hint={t("settings.phone.pairHint")}>
        {#snippet control()}
          <Button variant="primary" full loading={opening} onclick={startPairing}>
            {t("settings.phone.pairButton")}
          </Button>
        {/snippet}
      </SettingsRow>
    {/if}
  </SettingsGroup>

  {#if status && status.devices.length > 0}
    <SettingsGroup title={t("settings.phone.share")}>
      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={status?.clipboard ?? false}
            label={t("settings.phone.clipboard")}
            hint={t("settings.phone.clipboardHint")}
            onchange={setClipboard}
          />
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    <SettingsGroup title={t("settings.phone.devices")}>
      {#each status.devices as device (device.id)}
        <SettingsRow
          label={device.name}
          hint={device.connected ? t("settings.phone.connected") : t("settings.phone.offline")}
        >
          {#snippet control()}
            <Button variant="danger" size="sm" full onclick={() => unpair(device.id)}>
              {t("settings.phone.unpair")}
            </Button>
          {/snippet}
        </SettingsRow>
      {/each}
    </SettingsGroup>
  {/if}
</div>
