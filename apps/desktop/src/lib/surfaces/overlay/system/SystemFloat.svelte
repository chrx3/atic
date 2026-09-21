<script lang="ts">
  import { tip } from "$surfaces/overlay/tip.svelte";
  /**
   * Float de sistema: hermano de la pill, fundido al liquid.
   *
   * Apertura (pill-liquid-emerge): nace fused chico → crece w/h → se separa
   * hasta cortar el cuello. Cierre = reverse: approach → shrink → dismiss.
   */
  import { onMount, tick } from "svelte";
  import SystemPanel from "$features/system/SystemPanel.svelte";
  import {
    hideSystemWindow,
    onSystemBubbleAnchor,
    onSystemBubbleDismiss,
    setSystemAlwaysOnTop,
    systemAlwaysOnTop,
  } from "$ipc/system";
  import { onOverlayDismiss, overlayWorkAreas } from "$ipc/overlay";
  import type { Area } from "$ipc/overlay";
  import type { BubbleOpen } from "$core/types";
  import { Bubble } from "$surfaces/overlay/bubble.svelte";
  import { createBubbleDrag } from "$surfaces/overlay/bubbleDrag";
  import {
    FUSED_GAP_PX,
    expandPanelFromSeed,
    placePanelFusedFull,
    placePanelFusedSeed,
    placePanelResting,
  } from "$surfaces/overlay/floatPlace";
  import { gapBetween } from "$lib/liquid/geometry";
  import {
    awayFromPill,
    retachesOnDrop,
    type MagnetRect,
  } from "$surfaces/overlay/retachMagnet";
  import { REACH } from "$lib/liquid/constants";
  import { liquid, LIQUID_HUB } from "$surfaces/overlay/group.svelte";
  import {
    publishEmergeSkin,
    publishFollowSkin,
  } from "$surfaces/overlay/floatEmergeSkin";
  import { separateAxisProp, waitFrames } from "$surfaces/overlay/floatReveal";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import {
    notifyToolResting,
    toolBirth,
    toolResting,
  } from "$surfaces/overlay/toolBirth";
  import {
    armOpenDismissGrace,
    isOpenDismissGrace,
  } from "$surfaces/overlay/openDismissGrace";
  import { afterTransition, MOTION, ms, prefersReducedMotion, wait } from "$lib/motion";
  import Icon from "$ui/Icon.svelte";
  import { t } from "$domain/i18n.svelte";
  import { Pin, PanelTopClose, X } from "$lib/icons";
  import { emit } from "@tauri-apps/api/event";

  const CORNER = 20;
  const SEED_HOLD_MS = 60;
  const bubble = new Bubble();
  let el = $state<HTMLElement | null>(null);
  /** Imán del re-acople (reglas y porqués en `retachMagnet`). */
  let dropRetachArmed = false;
  function armDropRetach(): void {
    const pill = surfaces.live["pill-skin"] ?? surfaces.live["pill"];
    dropRetachArmed = awayFromPill(pill, bubble.anchor);
  }
  const { startDrag, endDrag } = createBubbleDrag(bubble, () => el, {
    // El gesto arranca: si ya está lejos, el retach al soltar vale desde ya.
    onGrab: () => armDropRetach(),
    onMove: () => {
      // Salió de la zona: el retach queda armado para este gesto.
      if (!dropRetachArmed) armDropRetach();
    },
    onDrop: (info) => maybeRetachOnDrop(info.frame),
  });
  /** Pin always-on-top (misma semántica que agentes). */
  let pinned = $state(false);
  let workAreas = $state<Area[]>([]);
  /** Último ancla: re-colocar cuando llegan work areas. */
  let lastOpen: BubbleOpen | null = null;

  type RevealPhase = "hidden" | "expand" | "separate" | "ready" | "approach" | "shrink";
  let revealPhase = $state<RevealPhase>("hidden");
  let revealEpoch = 0;
  let closing = false;
  let ignoreIpcDismiss = false;
  /** El panel entró por despegue: directo al reposo, sin morph de nacimiento. */
  let detachDirect = false;
  /** Retach en curso: el dismiss del overlay no debe cerrar el panel. */
  let retaching = false;
  const expanding = $derived(revealPhase === "expand" || revealPhase === "shrink");
  const separating = $derived(revealPhase === "separate" || revealPhase === "approach");
  const motionPhase = $derived(expanding || separating);

  let openDur = $state(100);
  let separateDur = $state(90);
  let closeDur = $state(100);

  function armOpenDur() {
    openDur = ms(MOTION.launcherBar);
    separateDur = ms(MOTION.launcherSeparate);
  }

  function armCloseDur() {
    closeDur = ms(MOTION.floatClose);
  }

  function cancelReveal() {
    revealEpoch += 1;
  }

  function pillForOpen() {
    return toolBirth() ?? surfaces.live["pill-skin"] ?? surfaces.live["pill"];
  }

  function applyRestingPlace(a: BubbleOpen) {
    const rest = toolResting();
    if (rest) {
      // Despegue: el reposo EXACTO es el rect de la cara — mismo tamaño y
      // misma posición. El panel es el mismo que estaba pegado; el notch se
      // queda con su pestaña.
      bubble.place({ ...a, x: rest.x, y: rest.y, w: rest.w, h: rest.h });
      notifyToolResting();
      return;
    }
    const pill = pillForOpen();
    if (!pill) {
      bubble.place(a);
      notifyToolResting();
      return;
    }
    bubble.place({
      ...a,
      ...placePanelResting(
        pill,
        { w: a.w, h: a.h },
        { corner: CORNER, work: workAreas },
      ),
    });
    notifyToolResting();
  }

  function placeFusedToPill(a: BubbleOpen, pill = pillForOpen()) {
    const rest = toolResting();
    if (rest) {
      // Despegue: la semilla ES el rect de la cara — mismo tamaño y misma
      // posición. De ahí crece a su tamaño de float conservando la esquina
      // (side "top"), así que la transición es continuidad, no nacimiento.
      bubble.place({
        ...a,
        x: rest.x,
        y: rest.y,
        w: rest.w,
        h: rest.h,
        side: "top",
        offset: rest.w / 2,
      });
      return;
    }
    if (!pill) {
      bubble.place(a);
      return;
    }
    bubble.place({
      ...a,
      ...placePanelFusedSeed(
        pill,
        { w: a.w, h: a.h },
        { corner: CORNER, work: workAreas },
      ),
    });
  }

  async function placeFromPill(a: BubbleOpen) {
    lastOpen = a;
    const fresh = !bubble.alive || !bubble.shown;
    if (fresh) {
      armOpenDur();
      armOpenDismissGrace();
    }
    if (workAreas.length === 0) {
      try {
        workAreas = await overlayWorkAreas();
      } catch {
        workAreas = [];
      }
    }
    if (lastOpen !== a) return;
    // Despegue: el panel ya nace en su rect exacto (era la cara). Sin gota,
    // sin grow, sin chrome escondido: el mismo elemento, cortado en dos.
    if (toolResting()) {
      detachDirect = true;
      applyRestingPlace(a);
      // Sin frame replegado: el panel ya está en su rect, como si la cara
      // nunca se hubiera ido. `place` siempre programa un cuadro en scale +
      // viaje hacia la pill; en el despegue ese nacimiento rompe el corte.
      bubble.shown = true;
      return;
    }
    // No reposo durante birth/close: un re-anchor hacía snap separado.
    if (fresh || revealPhase === "hidden") {
      placeFusedToPill(a);
      return;
    }
    if (revealPhase === "ready" && !closing) {
      applyRestingPlace(a);
    }
  }

  async function runOpenReveal() {
    const epoch = ++revealEpoch;
    // Despegue: directo al reposo. La bandera es local a propósito: el `rest`
    // global ya se limpió cuando corre el efecto, y depender de él resucitaba
    // el morph (la caja vacía que parecía rehacer el elemento).
    if (detachDirect) {
      if (lastOpen) applyRestingPlace(lastOpen);
      revealPhase = "ready";
      return;
    }
    if (prefersReducedMotion()) {
      if (lastOpen) applyRestingPlace(lastOpen);
      revealPhase = "ready";
      return;
    }

    revealPhase = "expand";
    await tick();
    await waitFrames(2);
    await wait(SEED_HOLD_MS);
    if (epoch !== revealEpoch) return;

    const full = lastOpen;
    if (full && bubble.anchor) {
      // Crece desde la semilla solapada (borde clavado). No re-place fused
      // full: eso saltaba a gap+2 y se leía como panel externo.
      bubble.place({
        ...full,
        ...expandPanelFromSeed(
          {
            side: bubble.anchor.side as BubbleOpen["side"],
            offset: bubble.anchor.offset,
            x: bubble.anchor.x,
            y: bubble.anchor.y,
            w: bubble.anchor.w,
            h: bubble.anchor.h,
          },
          { w: full.w, h: full.h },
        ),
      });
    }
    await afterTransition(el, "width", openDur);
    if (epoch !== revealEpoch) return;

    revealPhase = "separate";
    await tick();
    await waitFrames(2);
    if (epoch !== revealEpoch) return;
    const separateProp = separateAxisProp(bubble.anchor?.side);
    if (lastOpen) applyRestingPlace(lastOpen);
    await afterTransition(el, separateProp, separateDur);
    if (epoch !== revealEpoch) return;
    revealPhase = "ready";
  }

  /** Close = reverse: approach (fuse) → shrink (seed solapada) → dismiss. */
  async function runCloseReveal(epoch: number): Promise<void> {
    if (prefersReducedMotion()) return;

    const full = lastOpen;
    const pill = surfaces.live["pill-skin"] ?? surfaces.live["pill"];
    const side = (bubble.anchor?.side ?? full?.side ?? "top") as BubbleOpen["side"];
    // El cierre conserva el tamaño VIVO: el panel pudo nacer con el rect de
    // la cara (despegue) y crecer a 312×372 solo por el ancla de Rust.
    const cur = bubble.anchor;
    const size = {
      w: cur?.w ?? full?.w ?? 0,
      h: cur?.h ?? full?.h ?? 0,
    };

    revealPhase = "approach";
    await tick();
    await waitFrames(2);
    if (epoch !== revealEpoch) return;
    if (full && pill) {
      bubble.place({
        ...full,
        ...placePanelFusedFull(pill, size, side, {
          corner: CORNER,
          work: workAreas,
          fusedGap: FUSED_GAP_PX,
        }),
      });
    } else if (full) {
      applyRestingPlace(full);
    }
    await afterTransition(el, separateAxisProp(side), separateDur);
    // Sin semilla ni "pelota": el panel llega a la pill y se apaga ahí; la
    // cara —con el notch ya agrandado— aparece del otro lado. El re-tach es
    // el despegue al revés.
  }

  $effect(() => {
    if (!bubble.alive) {
      if (revealPhase !== "hidden") revealPhase = "hidden";
      closing = false;
      return;
    }
    if (bubble.shown && revealPhase === "hidden" && !closing) {
      void runOpenReveal();
    }
  });

  const pillSkin = $derived(surfaces.live["pill-skin"]);
  const joined = $derived.by(() => {
    const a = bubble.anchor;
    const p = pillSkin;
    if (!a || !p || !bubble.alive) return false;
    return gapBetween(p, a) <= REACH;
  });

  $effect(() => {
    if (!bubble.alive || !el) {
      liquid.publish("system", []);
      return;
    }
    void bubble.shown;
    void revealPhase;
    void bubble.anchor;
    const group = motionPhase || joined ? LIQUID_HUB : undefined;
    if (motionPhase) {
      return publishFollowSkin("system", el, CORNER, group);
    }
    return publishEmergeSkin("system", el, CORNER, group);
  });

  $effect(() => {
    if (bubble.shown) surfaces.bringToFront("system");
  });

  $effect(() => {
    if (!el || !bubble.alive) return;
    const stop = surfaces.add("system", el);
    void surfaces.flush();
    return stop;
  });
  $effect(() => {
    if (!bubble.alive || !bubble.shown) return;
    void bubble.anchor;
    void surfaces.recoverHits();
    const t = window.setTimeout(
      () => {
        void surfaces.recoverHits();
      },
      ms(MOTION.floatOpen) + 48,
    );
    return () => window.clearTimeout(t);
  });
  $effect(() => {
    void bubble.anchor;
    void surfaces.dragging;
    if (surfaces.dragging) return;
    surfaces.schedule();
  });

  async function togglePin() {
    const next = !pinned;
    pinned = next;
    try {
      await setSystemAlwaysOnTop(next);
    } catch {
      pinned = !next;
    }
  }

  /**
   * Retach: el despegue al revés, sin coreografía.
   *
   * El panel se apaga EN EL ACTO (sin fade, sin pelota, sin corrimiento) y
   * la isla abre su cara —del mismo tamaño— en el mismo tramo. Nunca hay dos
   * paneles vivos a la vez: es el mismo corte que el despegue, al revés.
   */
  function retachToPill(): void {
    if (retaching || closing) return;
    retaching = true;
    void emit("dock-tool-face", "system").catch(() => {});
    finishDismiss(bubble.shown);
    bubble.alive = false;
  }

  /**
   * Soltado cerca de la pill CON el imán armado: se coloca. Lejos —o sin
   * armar, porque el gesto nació pegado al notch y nunca se alejó— queda
   * flotando.
   */
  function maybeRetachOnDrop(frame: MagnetRect): void {
    const pill = surfaces.live["pill-skin"] ?? surfaces.live["pill"];
    if (!retachesOnDrop(dropRetachArmed, pill, frame)) return;
    dropRetachArmed = false;
    retachToPill();
  }

  function finishDismiss(wasShown: boolean, opts: { skipHideWindow?: boolean } = {}) {
    lastOpen = null;
    revealPhase = "hidden";
    detachDirect = false;
    retaching = false;
    endDrag();
    surfaces.resetInteraction();
    armCloseDur();
    bubble.hide();
    if (!wasShown) bubble.alive = false;
    if (!opts.skipHideWindow) {
      ignoreIpcDismiss = true;
      void hideSystemWindow().finally(() => {
        window.setTimeout(() => {
          ignoreIpcDismiss = false;
        }, 320);
      });
    }
    closing = false;
  }

  async function close(opts: { fromIpcDismiss?: boolean } = {}) {
    if (!bubble.shown && !bubble.alive) return;
    if (closing) {
      if (opts.fromIpcDismiss) return;
      cancelReveal();
      finishDismiss(bubble.shown, { skipHideWindow: opts.fromIpcDismiss });
      return;
    }
    closing = true;
    const wasShown = bubble.shown;
    endDrag();
    surfaces.resetInteraction();
    const epoch = ++revealEpoch;
    await runCloseReveal(epoch);
    if (!closing) return;
    if (epoch !== revealEpoch) {
      closing = false;
      return;
    }
    finishDismiss(wasShown, { skipHideWindow: opts.fromIpcDismiss });
  }

  function tryAutoClose() {
    if (!bubble.shown) return;
    void systemAlwaysOnTop()
      .then((on) => {
        if (on || !bubble.shown) return;
        close();
      })
      .catch(() => {
        /* sin lectura del pin, no cerrar */
      });
  }

  onMount(() => {
    void systemAlwaysOnTop()
      .then((on) => {
        pinned = on;
      })
      .catch(() => {
        pinned = false;
      });
    void overlayWorkAreas()
      .then((areas) => {
        workAreas = areas;
        if (lastOpen && bubble.alive && revealPhase === "ready") {
          applyRestingPlace(lastOpen);
        } else if (
          lastOpen &&
          bubble.alive &&
          (revealPhase === "hidden" || revealPhase === "expand") &&
          !bubble.shown
        ) {
          placeFusedToPill(lastOpen);
        }
      })
      .catch(() => {
        workAreas = [];
      });
    const un: Promise<() => void>[] = [
      onSystemBubbleAnchor((a) => {
        void placeFromPill(a);
      }),
      onSystemBubbleDismiss(() => {
        if (retaching) return;
        if (ignoreIpcDismiss) return;
        void close({ fromIpcDismiss: true });
      }),
      onOverlayDismiss(() => {
        surfaces.resetInteraction();
        if (isOpenDismissGrace()) return;
        tryAutoClose();
      }),
    ];
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      surfaces.resetInteraction();
      if (!bubble.shown && !bubble.alive) return;
      void systemAlwaysOnTop()
        .then((on) => {
          if (!on && (bubble.shown || bubble.alive)) void close();
        })
        .catch(() => {
          /* sin pin, no cerrar */
        });
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      endDrag();
      surfaces.resetInteraction();
      for (const p of un) void p.then((fn) => fn());
      liquid.publish("system", []);
    };
  });
</script>

{#if bubble.alive}
  <div
    class="sys-float float-emerge"
    class:is-shown={bubble.shown}
    class:is-joined={joined}
    class:is-expanding={expanding}
    class:is-separating={separating}
    data-float="system"
    data-side={bubble.anchor?.side ?? "top"}
    style={bubble.vars}
    style:--float-stack={surfaces.stack("system")}
    style:--launcher-bar-open-dur="{openDur}ms"
    style:--launcher-separate-dur="{separateDur}ms"
    style:--float-close-dur="{closeDur}ms"
    bind:this={el}
    role="dialog"
    aria-label={t("tools.system.label")}
  >
    <!-- Barra de arrastre al centro y controles mínimos a la derecha: el
         mismo espíritu que la cara de la isla, sin título ni hint. -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header class="sys-head" onpointerdown={startDrag}>
      <i class="sys-grab" aria-hidden="true"></i>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="sys-acts" data-no-drag onpointerdown={(e) => e.stopPropagation()}>
        <button
          type="button"
          class="sys-icon"
          class:is-on={pinned}
          aria-label={pinned ? t("overlay.unpin") : t("overlay.pin")}
          aria-pressed={pinned}
          use:tip={pinned ? t("overlay.unpin") : t("overlay.pin")}
          onclick={() => void togglePin()}
        >
          <Icon icon={Pin} size={13} />
        </button>
        <button
          type="button"
          class="sys-icon"
          onclick={retachToPill}
          aria-label={t("overlay.retach")}
          use:tip={t("overlay.retach")}
        >
          <Icon icon={PanelTopClose} size={14} />
        </button>
        <button
          type="button"
          class="sys-icon"
          onclick={() => void close()}
          aria-label={t("overlay.close")}
          use:tip={t("overlay.close")}
        >
          <Icon icon={X} size={14} />
        </button>
      </div>
    </header>
    <div class="sys-body">
      <SystemPanel />
    </div>
  </div>
{/if}

<style>
  /*
   * Fused grow → separate: nace seed pegado a la pill, crece w/h con borde
   * clavado, luego se aleja (gap > REACH) y corta el cuello.
   */

  /* Abierto sin clic, el anillo de foco del sistema se lee como “seleccionado”. */
  .sys-float:focus,
  .sys-float:focus-visible {
    outline: none;
  }

  .sys-float {
    /* Duraciones heredadas de :root (app.css); sin overrides locales. */
    position: absolute;
    z-index: calc(var(--z-overlay-float) + var(--float-stack, 0));
    display: flex;
    flex-direction: column;
    left: var(--x);
    top: var(--y);
    width: var(--w);
    height: var(--h);
    box-sizing: border-box;
    padding: 0.45rem 0.5rem 0.55rem;
    border-radius: 18px;

    /* Transparente: un fill opaco corta la sombra de la piel en el cuello. */
    background: transparent;
    color: var(--text);
    overflow: hidden;

    /*
     * La entrada y la salida las lleva `.float-emerge` (app.css): opacidad, scale
     * y viaje hacia la pill, con `data-side` como origen. Abrir invita
     * (`--float-open-dur`) y cerrar se aparta (`--float-close-dur`). Antes había
     * sólo opacidad, y con la duración del cierre para los dos lados.
     */
  }

  .sys-float.is-expanding {
    transition:
      width var(--launcher-bar-open-dur) var(--ease-smooth-out),
      height var(--launcher-bar-open-dur) var(--ease-smooth-out),
      left var(--launcher-bar-open-dur) var(--ease-smooth-out),
      top var(--launcher-bar-open-dur) var(--ease-smooth-out),
      transform var(--float-open-dur) var(--ease-smooth-out),
      opacity var(--float-close-dur) var(--ease-smooth-out);
  }

  /* Semilla = silueta líquida; chrome visible solo fuera del grow/shrink. */
  .sys-float.is-expanding .sys-head,
  .sys-float.is-expanding .sys-body {
    opacity: 0;
    pointer-events: none;
  }

  .sys-float.is-separating {
    transition:
      left var(--launcher-separate-dur) var(--ease-smooth-out),
      top var(--launcher-separate-dur) var(--ease-smooth-out),
      width var(--duration-quick) var(--ease-smooth-out),
      height var(--duration-quick) var(--ease-smooth-out),
      transform var(--float-close-dur) var(--ease-smooth-out),
      opacity var(--float-close-dur) var(--ease-smooth-out);
  }

  .sys-head {
    position: relative;
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-bottom: 0.2rem;
    min-height: 1.5rem;
    cursor: grab;
    touch-action: none;
    user-select: none;
  }

  .sys-head:active {
    cursor: grabbing;
  }

  /* La barrita del centro: mismo lenguaje que el grab de la cara. */
  .sys-grab {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 2rem;
    height: 3px;
    transform: translate(-50%, -50%);
    border-radius: 999px;
    background: color-mix(in sRGB, var(--rb-text) 24%, transparent);
  }

  .sys-acts {
    position: relative;
    z-index: 2;
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.15rem;
  }

  .sys-icon {
    display: grid;
    place-items: center;
    box-sizing: border-box;
    width: 1.75rem;
    height: 1.75rem;
    border: 1px solid transparent;
    border-radius: 0.4rem;
    padding: 0;
    background: transparent;
    color: var(--faint);
    cursor: pointer;
    box-shadow: none;
    filter: none;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      border-color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .sys-icon :global(svg) {
    pointer-events: none;
  }

  .sys-icon:hover,
  .sys-icon.is-on {
    color: var(--text);
    background: color-mix(in sRGB, var(--text) 8%, transparent);
  }

  .sys-icon:active {
    transform: scale(0.96);
  }

  .sys-body {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    overflow: hidden;
  }

  @media (prefers-reduced-motion: reduce) {
    .sys-float,
    .sys-float.is-expanding,
    .sys-float.is-separating {
      transition: none;
    }

    .sys-icon:active {
      transform: none;
    }
  }
</style>
