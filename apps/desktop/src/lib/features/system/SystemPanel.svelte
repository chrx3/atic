<script lang="ts">
  import { onDestroy } from "svelte";
  import { system } from "$domain/system.svelte";
  import { t } from "$domain/i18n.svelte";
  import { createToasts } from "$domain/toasts.svelte";
  import ToastStack from "$ui/ToastStack.svelte";
  import { systemAlerts } from "$domain/systemAlerts.svelte";
  import Icon from "$ui/Icon.svelte";
  import ConfirmDialog from "$ui/ConfirmDialog.svelte";
  import { tip } from "$surfaces/overlay/tip.svelte";
  import { Coffee, Lock, Moon, Trash2, Volume2, VolumeX, X } from "$lib/icons";
  import { barWidth, formatBytes, formatPercent } from "./systemFormat";
  import type { SystemApp } from "$ipc/system";
  import OverlaySlider from "$surfaces/overlay/OverlaySlider.svelte";

  let {
    island = false,
  }: {
    island?: boolean;
  } = $props();

  /** Memoria usada en %, que es como se piden los umbrales. */
  const ramPercent = $derived(
    system.snapshot && system.snapshot.ram_total > 0
      ? (system.snapshot.ram_used / system.snapshot.ram_total) * 100
      : 0,
  );

  /**
   * Avisos de este panel, no los globales.
   *
   * Los globales los pinta el host del overlay, fijo al fondo de la ventana
   * transparente: «ya no estaba abierta» quedaba en la barra de tareas, lejos
   * de la pill. Acá la pila vive dentro del propio panel.
   */
  const notices = createToasts();
  onDestroy(() => notices.clear());

  let forceTarget = $state<SystemApp | null>(null);
  let forceBusy = $state(false);
  /**
   * La acción rápida que espera confirmación.
   *
   * Bloquear y suspender te sacan de lo que estabas haciendo, y la papelera no
   * tiene vuelta atrás: un clic de paso no alcanza para ninguna de las tres.
   * Silenciar y el café se deshacen con otro clic, así que van directo.
   */
  type QuickAsk = "lock" | "sleep" | "trash";
  let ask = $state<QuickAsk | null>(null);

  const sessions = $derived(system.audio?.sessions ?? []);

  /**
   * Cada pestaña a su ritmo, y solo la que se está viendo.
   *
   * Refrescar las tres cada segundo significaba enumerar los procesos de la
   * máquina mientras mirabas el volumen. Los recursos cambian solos y hay que
   * seguirlos; el volumen puede cambiar desde fuera, pero despacio; el brillo
   * no cambia solo, así que se lee al entrar y punto.
   */
  const CADENCIA: Record<string, number> = {
    resources: 1500,
    audio: 2500,
    display: 0,
    // Avisos muestra el valor de ahora junto al umbral: sin refresco, el
    // número que ayuda a decidir estaría congelado.
    alerts: 2000,
  };

  // Primera carga: las tres, para que cambiar de pestaña no parpadee.
  $effect(() => {
    void system.hydrateAll();
  });

  $effect(() => {
    const tab = system.tab;
    void system.hydrate(tab);
    const cada = CADENCIA[tab] ?? 0;
    if (cada === 0) return;
    const timer = window.setInterval(() => void system.hydrate(tab), cada);
    return () => window.clearInterval(timer);
  });

  function askForce(app: SystemApp) {
    if (!app.can_force) return;
    forceTarget = app;
  }

  /**
   * Cerrar avisa qué pasó.
   *
   * Rust devuelve a cuántos procesos se les pidió cerrar y ese número se
   * tiraba: pedías cerrar Chrome, la fila seguía ahí un segundo y parecía que
   * el botón no había hecho nada.
   */
  async function closeApp(app: SystemApp) {
    try {
      const n = await system.closeApp(app.id);
      notices.push(
        n > 0
          ? t("overlay.system.closing", { name: app.name })
          : t("overlay.system.closedNone", { name: app.name }),
      );
    } catch (err) {
      notices.push(err instanceof Error ? err.message : String(err));
    }
  }

  /** Clic en la fila: traerla al frente. Cerrar y forzar son la excepción. */
  async function focusApp(app: SystemApp) {
    if (!app.can_focus) return;
    try {
      await system.focusApp(app.id);
    } catch {
      /* se fue entre el barrido y el clic */
    }
  }

  async function confirmAsk() {
    const action = ask;
    ask = null;
    if (!action) return;
    try {
      await system.runAction(action);
    } catch (err) {
      notices.push(err instanceof Error ? err.message : String(err));
    }
  }

  async function confirmForce() {
    const target = forceTarget;
    if (!target) return;
    forceBusy = true;
    try {
      await system.forceApp(target.id);
      forceTarget = null;
    } finally {
      forceBusy = false;
    }
  }
</script>

<div class="sys" class:is-island={island}>
  <div class="sys-quick" role="group" aria-label={t("tools.system.label")}>
    <button
      type="button"
      class="sys-quick-btn"
      use:tip={t("overlay.system.lockHint")}
      onclick={() => (ask = "lock")}
    >
      <Icon icon={Lock} size={13} />
      <span>{t("overlay.system.lock")}</span>
    </button>
    <button
      type="button"
      class="sys-quick-btn"
      use:tip={t("overlay.system.sleepHint")}
      onclick={() => (ask = "sleep")}
    >
      <Icon icon={Moon} size={13} />
      <span>{t("overlay.system.sleep")}</span>
    </button>
    <button
      type="button"
      class="sys-quick-btn"
      use:tip={t("overlay.system.muteHint")}
      onclick={() => void system.runAction("mute")}
    >
      <Icon icon={system.audio?.muted ? VolumeX : Volume2} size={13} />
      <span>{t("overlay.system.muteAction")}</span>
    </button>
    <button
      type="button"
      class="sys-quick-btn"
      use:tip={t("overlay.system.trashHint")}
      onclick={() => (ask = "trash")}
    >
      <Icon icon={Trash2} size={13} />
      <span>{t("overlay.system.trash")}</span>
    </button>
    <!-- Café: mientras esté puesto, el equipo no se duerme. No se persiste —
         un "no te duermas" que sobrevive al reinicio es una batería vacía. -->
    <button
      type="button"
      class="sys-quick-btn"
      class:is-on={system.awake}
      aria-pressed={system.awake}
      use:tip={system.awake
        ? t("overlay.system.awakeOn")
        : t("overlay.system.awakeHint")}
      onclick={() => void system.setAwake(!system.awake)}
    >
      <Icon icon={Coffee} size={13} />
      <span>{t("overlay.system.awake")}</span>
    </button>
  </div>

  <div class="sys-tabs" role="tablist" aria-label={t("tools.system.label")}>
    <button
      type="button"
      role="tab"
      aria-controls="sys-panel"
      class="sys-tab"
      class:is-on={system.tab === "resources"}
      aria-selected={system.tab === "resources"}
      onclick={() => (system.tab = "resources")}
    >
      {t("overlay.system.resources")}
    </button>
    <button
      type="button"
      role="tab"
      aria-controls="sys-panel"
      class="sys-tab"
      class:is-on={system.tab === "audio"}
      aria-selected={system.tab === "audio"}
      onclick={() => (system.tab = "audio")}
    >
      {t("overlay.system.audio")}
    </button>
    <button
      type="button"
      role="tab"
      aria-controls="sys-panel"
      class="sys-tab"
      class:is-on={system.tab === "display"}
      aria-selected={system.tab === "display"}
      onclick={() => (system.tab = "display")}
    >
      {t("overlay.system.display")}
    </button>
    <button
      type="button"
      role="tab"
      aria-controls="sys-panel"
      class="sys-tab"
      class:is-on={system.tab === "alerts"}
      aria-selected={system.tab === "alerts"}
      onclick={() => (system.tab = "alerts")}
    >
      {t("overlay.system.alerts")}
    </button>
  </div>

  <!-- `display: contents`: el panel existe para la accesibilidad (es el
       `tabpanel` que las pestañas controlan) sin meterse en el layout. -->
  <div class="sys-panel" id="sys-panel" role="tabpanel" tabindex="-1">
    {#if system.loading && !system.snapshot}
      <p class="sys-empty">{t("overlay.system.loading")}</p>
    {:else if system.error && !system.snapshot}
      <p class="sys-empty">{system.error}</p>
    {:else if system.tab === "resources"}
      <div class="sys-meters">
        <div class="sys-meter">
          <span
            >{t("overlay.system.cpu")} {formatPercent(system.snapshot?.cpu ?? 0)}</span
          >
          <i class="sys-bar"
            ><i style:width={formatPercent(system.snapshot?.cpu ?? 0)}></i></i
          >
        </div>
        <div class="sys-meter">
          <span>
            {t("overlay.system.ram")}
            {formatBytes(system.snapshot?.ram_used ?? 0)} / {formatBytes(
              system.snapshot?.ram_total ?? 0,
            )}
          </span>
          <i class="sys-bar"
            ><i
              style:width={barWidth(
                system.snapshot?.ram_used ?? 0,
                system.snapshot?.ram_total ?? 0,
              )}
            ></i></i
          >
        </div>
      </div>
      <div class="sys-sort">
        <button
          type="button"
          class="sys-sort-btn"
          class:is-on={system.sort === "cpu"}
          onclick={() => (system.sort = "cpu")}
        >
          {t("overlay.system.sortCpu")}
        </button>
        <button
          type="button"
          class="sys-sort-btn"
          class:is-on={system.sort === "ram"}
          onclick={() => (system.sort = "ram")}
        >
          {t("overlay.system.sortRam")}
        </button>
        {#if system.hasBackground}
          <!-- Segundo plano: `node`, `cargo`, helpers. Apagado por defecto
             porque son decenas; con una búsqueda escrita salen igual. -->
          <button
            type="button"
            class="sys-sort-btn"
            class:is-on={system.showBackground}
            aria-pressed={system.showBackground}
            use:tip={t("overlay.system.backgroundHint")}
            onclick={() => (system.showBackground = !system.showBackground)}
          >
            {t("overlay.system.showBackground")}
          </button>
        {/if}
      </div>
      <input
        class="sys-search"
        type="search"
        autocomplete="off"
        spellcheck="false"
        aria-label={t("overlay.system.search")}
        placeholder={t("overlay.system.searchPlaceholder")}
        bind:value={system.query}
      />
      {#if system.apps.length === 0}
        <p class="sys-empty">
          {system.query.trim()
            ? t("overlay.system.noMatches", { query: system.query.trim() })
            : t("overlay.system.empty")}
        </p>
      {:else}
        <ul class="sys-list">
          {#each system.apps as app (app.id)}
            <li class="sys-row" class:is-pending={system.pending[app.id]}>
              <!-- La fila entera trae la app al frente: es lo que uno quiere
                 hacer nueve de cada diez veces. Cerrar y forzar aparecen al
                 pasar por encima, para no competir con el nombre. -->
              <button
                type="button"
                class="sys-row-main"
                disabled={!app.can_focus}
                aria-label={app.can_focus
                  ? t("overlay.system.focusApp") + ": " + app.name
                  : app.name}
                onclick={() => void focusApp(app)}
              >
                <!-- Mismo hueco que las sesiones de audio: 1rem fijo, con ícono
                   o sin él, para que ninguna fila se corra ni cambie de alto. -->
                <span class="sys-app-icon">
                  {#if app.icon}
                    <img
                      src={app.icon}
                      width="16"
                      height="16"
                      alt=""
                      draggable="false"
                    />
                  {/if}
                </span>
                <strong>{app.name}</strong>
                <span>
                  {formatPercent(app.cpu)} · {formatBytes(app.ram_bytes)}
                  {#if app.background}
                    <em>{t("overlay.system.showBackground")}</em>
                  {/if}
                </span>
              </button>
              {#if app.can_force}
                <button type="button" class="sys-force" onclick={() => askForce(app)}>
                  {t("overlay.system.forceApp")}
                </button>
              {/if}
              {#if app.can_close}
                <button
                  type="button"
                  class="sys-icon"
                  aria-label={t("overlay.system.closeApp") + ": " + app.name}
                  onclick={() => void closeApp(app)}
                >
                  <Icon icon={X} size={12} />
                </button>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    {:else if system.tab === "audio"}
      {#if system.audio}
        <label class="sys-slider">
          <span>{t("overlay.system.volume")}</span>
          <OverlaySlider
            label={t("overlay.system.volume")}
            value={system.audio.volume}
            onValue={(v) => void system.setVolume(v)}
          />
        </label>
        <button
          type="button"
          class="sys-mute"
          class:is-on={system.audio.muted}
          onclick={() => void system.setMuted(!system.audio?.muted)}
        >
          <Icon icon={system.audio.muted ? VolumeX : Volume2} size={14} />
          {system.audio.muted ? t("overlay.system.unmute") : t("overlay.system.mute")}
        </button>
        {#if system.audio.per_app && system.audio.sessions.length > 0}
          <p class="sys-kicker">{t("overlay.system.sessions")}</p>
          <ul class="sys-list">
            {#each sessions as session (session.id)}
              <li class="sys-row sys-row-stack">
                <span class="sys-session">
                  <!-- El hueco va siempre, con ícono o sin él: la fila no se
                     corre cuando una app no entrega el suyo. -->
                  <span class="sys-session-icon">
                    {#if session.icon}
                      <img
                        src={session.icon}
                        width="16"
                        height="16"
                        alt=""
                        draggable="false"
                      />
                    {/if}
                  </span>
                  <strong>{session.name}</strong>
                </span>
                <OverlaySlider
                  label={session.name}
                  value={session.volume}
                  onValue={(v) => void system.setSessionVolume(session.id, v)}
                />
              </li>
            {/each}
          </ul>
        {:else if !system.audio.per_app}
          <p class="sys-empty">{t("overlay.system.noPerApp")}</p>
        {/if}
      {:else}
        <p class="sys-empty">{t("overlay.system.loading")}</p>
      {/if}
    {:else if system.tab === "alerts"}
      <!-- Los umbrales viven acá y no en Ajustes porque es donde se miran las
           cifras que hay que decidir: "esto que veo, ¿me lo avisas?". -->
      <label class="sys-check">
        <input
          type="checkbox"
          checked={system.alerts.enabled}
          onchange={(e) => system.saveAlerts({ enabled: e.currentTarget.checked })}
        />
        <span>{t("overlay.system.alertsOn")}</span>
      </label>
      <label class="sys-slider">
        <span>
          {t("overlay.system.alertCpu")}
          {system.alerts.cpu}%
        </span>
        <OverlaySlider
          label={t("overlay.system.alertCpu")}
          min={30}
          max={100}
          step={5}
          value={system.alerts.cpu}
          onValue={(v) => system.saveAlerts({ cpu: Math.round(v) })}
        />
      </label>
      <label class="sys-slider">
        <span>
          {t("overlay.system.alertRam")}
          {system.alerts.ram}%
        </span>
        <OverlaySlider
          label={t("overlay.system.alertRam")}
          min={30}
          max={100}
          step={5}
          value={system.alerts.ram}
          onValue={(v) => system.saveAlerts({ ram: Math.round(v) })}
        />
      </label>
      <label class="sys-slider">
        <span>
          {t("overlay.system.alertFor")}
          {Math.round(system.alerts.seconds / 60)}
          {t("overlay.system.alertMinutes")}
        </span>
        <OverlaySlider
          label={t("overlay.system.alertFor")}
          min={1}
          max={15}
          step={1}
          value={system.alerts.seconds / 60}
          onValue={(v) => system.saveAlerts({ seconds: Math.round(v) * 60 })}
        />
      </label>
      <p class="sys-kicker">{t("overlay.system.alertsHint")}</p>
      <!-- El número de ahora, al lado del umbral: es lo que hace falta para
           decidir dónde ponerlo. Sin esto, la pestaña pide adivinar. -->
      <div class="sys-meters">
        <div class="sys-meter">
          <span>
            {t("overlay.system.alertNow")}
            {t("overlay.system.cpu")}
            {formatPercent(system.snapshot?.cpu ?? 0)} · {t("overlay.system.ram")}
            {formatPercent(ramPercent)}
          </span>
        </div>
      </div>
      {#if systemAlerts.list.length > 0}
        <ul class="sys-list">
          {#each systemAlerts.list as aviso (aviso.kind)}
            <li class="sys-row sys-row-stack">
              <strong>
                {aviso.kind === "cpu"
                  ? t("overlay.system.cpu")
                  : t("overlay.system.ram")}
                {Math.round(aviso.value)}%
              </strong>
              <span>{aviso.culprit ?? t("overlay.system.alertNoCulprit")}</span>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="sys-empty">{t("overlay.system.alertsQuiet")}</p>
      {/if}
    {:else}
      {#if system.displays.length === 0}
        <p class="sys-empty">{t("overlay.system.loading")}</p>
      {:else}
        <ul class="sys-list">
          {#each system.displays as display (display.id)}
            <li class="sys-row sys-row-stack">
              <strong>
                {display.name}
                {#if display.primary}
                  <em>{t("overlay.system.primary")}</em>
                {/if}
              </strong>
              {#if display.brightness != null}
                <label class="sys-slider">
                  <span>{t("overlay.system.brightness")}</span>
                  <OverlaySlider
                    label={t("overlay.system.brightness")}
                    value={display.brightness}
                    onValue={(v) => void system.setBrightness(display.id, v)}
                  />
                </label>
              {:else}
                <p class="sys-empty">{t("overlay.system.noBrightness")}</p>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  </div>

  {#if forceTarget}
    <ConfirmDialog
      contained
      tone="danger"
      title={t("overlay.system.forceTitle", { name: forceTarget.name })}
      body={t("overlay.system.forceBody")}
      confirmLabel={t("overlay.system.forceConfirm")}
      busy={forceBusy}
      onConfirm={() => void confirmForce()}
      onCancel={() => (forceTarget = null)}
    />
  {/if}

  {#if ask}
    <!-- La papelera no confía en el aviso de Finder, que el usuario puede
         tener apagado: es lo único irreversible del panel y va en rojo. -->
    <ConfirmDialog
      contained
      tone={ask === "trash" ? "danger" : "default"}
      title={t(`overlay.system.${ask}Title`)}
      body={t(`overlay.system.${ask}Body`)}
      confirmLabel={ask === "trash"
        ? t("overlay.system.trashConfirm")
        : t(`overlay.system.${ask}`)}
      onConfirm={() => void confirmAsk()}
      onCancel={() => (ask = null)}
    />
  {/if}
  <ToastStack
    placement="local"
    items={notices.items}
    onDismiss={(id) => notices.dismiss(id)}
  />
</div>

<style>
  .sys {
    position: relative;
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.35rem;
    overflow: hidden;
  }

  /* La pila es absoluta al panel: sin esto el aviso crece a 400px y se sale. */
  .sys :global([aria-live="polite"] > div) {
    max-width: 100%;
  }

  .sys-quick,
  .sys-tabs,
  .sys-sort {
    display: flex;
    flex-shrink: 0;
    gap: 0.25rem;
  }

  .sys-quick-btn,
  .sys-tab,
  .sys-sort-btn,
  .sys-mute,
  .sys-force {
    border: 1px solid transparent;
    border-radius: 0.4rem;
    background: color-mix(in sRGB, var(--text) 7%, transparent);
    color: var(--faint);
    cursor: pointer;
    font: inherit;
  }

  .sys-quick-btn {
    display: flex;
    flex: 1;
    align-items: center;
    justify-content: center;
    gap: 0.25rem;
    min-height: 1.6rem;
    padding: 0 0.35rem;
    font-size: 0.6875rem;
    font-weight: 600;
  }

  .sys-tab,
  .sys-sort-btn {
    flex: 1;
    min-height: 1.5rem;
    font-size: 0.6875rem;
    font-weight: 650;
  }

  .sys-tab.is-on,
  .sys-sort-btn.is-on,
  .sys-mute.is-on,
  .sys-quick-btn:hover,
  .sys-tab:hover,
  .sys-sort-btn:hover {
    color: var(--text);
    background: color-mix(in sRGB, var(--text) 12%, transparent);
  }

  .sys-panel {
    display: contents;
  }

  .sys-meters {
    display: flex;
    flex-shrink: 0;
    flex-direction: column;
    gap: 0.3rem;
  }

  .sys-meter {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    font-size: 0.6875rem;
    font-weight: 600;
    color: var(--text);
  }

  .sys-bar {
    display: block;
    height: 4px;
    overflow: hidden;
    border-radius: 999px;
    background: color-mix(in sRGB, var(--text) 12%, transparent);
  }

  .sys-bar > i {
    display: block;
    height: 100%;
    background: var(--text);
  }

  .sys-list {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.2rem;
    margin: 0;
    padding: 0;

    /* El pulgar del slider mide 14 px y va centrado sobre el riel: en los
       extremos sobresale ~7 px del riel y eso volvía scrolleable esta lista en
       horizontal (barra abajo de cada slider). El recorte horizontal cae en el
       mismo borde donde ya recorta `.sys`, que es quien encierra el panel, así
       que visualmente no se corta nada que no se cortara antes. */
    overflow: hidden auto;
    list-style: none;
  }

  .sys-row {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.2rem 0;
  }

  .sys-row-stack {
    flex-direction: column;
    align-items: stretch;
    gap: 0.2rem;
  }

  .sys-session {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 0.35rem;
  }

  .sys-session strong {
    flex: 1;
    min-width: 0;
  }

  /* Cuadrado fijo: mismo hueco con ícono y sin él. */
  .sys-session-icon,
  .sys-app-icon {
    display: grid;
    width: 1rem;
    height: 1rem;
    flex-shrink: 0;
    place-items: center;
  }

  .sys-session-icon img,
  .sys-app-icon img {
    display: block;
    width: 1rem;
    height: 1rem;
    border-radius: 0.2rem;
    object-fit: contain;
  }

  .sys-row-main {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    column-gap: 0.35rem;
    min-width: 0;
    flex: 1;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  /* El hueco del ícono ocupa la primera columna y las dos filas: el nombre y
     las cifras quedan alineados al mismo borde, con ícono o sin él. */
  .sys-row-main .sys-app-icon {
    grid-column: 1;
    grid-row: 1 / span 2;
  }

  .sys-row-main strong,
  .sys-row-main > span:not(.sys-app-icon) {
    grid-column: 2;
  }

  .sys-row-main:disabled {
    cursor: default;
  }

  .sys-row-main:not(:disabled):hover strong,
  .sys-row-main:focus-visible strong {
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  /* Lo destructivo no compite con el nombre: aparece al acercarse. */
  .sys-row .sys-force,
  .sys-row .sys-icon {
    opacity: 0;
  }

  .sys-row:hover .sys-force,
  .sys-row:hover .sys-icon,
  .sys-row:focus-within .sys-force,
  .sys-row:focus-within .sys-icon {
    opacity: 1;
  }

  @media (hover: none) {
    .sys-row .sys-force,
    .sys-row .sys-icon {
      opacity: 1;
    }
  }

  .sys-row.is-pending {
    opacity: 0.55;
  }

  .sys-check {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--text);
    font-size: 0.6875rem;
    font-weight: 600;
    cursor: pointer;
  }

  .sys-check input {
    accent-color: var(--text);
  }

  .sys-search {
    flex-shrink: 0;
    min-height: 1.6rem;
    padding: 0 0.4rem;
    border: 1px solid color-mix(in sRGB, var(--text) 12%, transparent);
    border-radius: 0.4rem;
    background: color-mix(in sRGB, var(--text) 5%, transparent);
    color: var(--text);
    font: inherit;
    font-size: 0.6875rem;
  }

  .sys-search::placeholder {
    color: var(--faint);
  }

  .sys-search:focus-visible {
    border-color: color-mix(in sRGB, var(--text) 28%, transparent);
    outline: none;
  }

  .sys-row strong {
    overflow: hidden;
    font-size: 0.75rem;
    font-weight: 650;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sys-row span,
  .sys-empty,
  .sys-kicker {
    color: var(--faint);
    font-size: 0.6875rem;
  }

  .sys-row em {
    margin-left: 0.3rem;
    font-style: normal;
    color: var(--faint);
    font-size: 0.625rem;
    font-weight: 650;
  }

  .sys-icon,
  .sys-force {
    flex-shrink: 0;
    min-height: 1.35rem;
    padding: 0 0.35rem;
    font-size: 0.625rem;
    font-weight: 650;
  }

  .sys-icon {
    display: grid;
    width: 1.35rem;
    place-items: center;
    padding: 0;
    color: var(--faint);
    background: transparent;
  }

  .sys-slider {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    font-size: 0.6875rem;
    font-weight: 650;
  }

  .sys-mute {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.35rem;
    min-height: 1.7rem;
    font-size: 0.75rem;
    font-weight: 650;
  }

  .sys-empty {
    margin: 0.4rem 0 0;
  }

  .sys-kicker {
    margin: 0.2rem 0 0;
    font-weight: 650;
  }

  .sys.is-island .sys-quick-btn span {
    display: none;
  }
</style>
