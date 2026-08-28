<script lang="ts">
  import { tip } from "$surfaces/overlay/tip.svelte";
  /**
   * Float de clipboard: hermano de la pill, fundido al liquid.
   *
   * Apertura (pill-liquid-emerge): nace fused chico → crece w/h → se separa
   * hasta cortar el cuello. Cierre = reverse: approach → shrink → dismiss.
   */
  import { onMount, tick } from "svelte";
  import ClipboardHistoryList from "$lib/ClipboardHistoryList.svelte";
  import { clipboard } from "$domain/clipboard.svelte";
  import { sessionEffect } from "$domain/session";
  import {
    clipboardAlwaysOnTop,
    hideClipboardWindow,
    onClipboardBubbleAnchor,
    onClipboardBubbleDismiss,
    setClipboardAlwaysOnTop,
  } from "$ipc/clipboard";
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
  import { REACH } from "$lib/liquid/constants";
  import { liquid } from "$surfaces/overlay/group.svelte";
  import {
    publishEmergeSkin,
    publishFollowSkin,
  } from "$surfaces/overlay/floatEmergeSkin";
  import {
    separateAxisProp,
    waitFrames,
  } from "$surfaces/overlay/floatReveal";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import {
    armOpenDismissGrace,
    isOpenDismissGrace,
  } from "$surfaces/overlay/openDismissGrace";
  import {
    afterTransition,
    MOTION,
    ms,
    prefersReducedMotion,
    wait,
  } from "$lib/motion";
  import Icon from "$ui/Icon.svelte";
  import { t } from "$domain/i18n.svelte";
  import ToastStack from "$ui/ToastStack.svelte";
  import { toasts } from "$domain/toasts.svelte";
  import { Pin, X } from "$lib/icons";

  const CORNER = 20;
  const SEED_HOLD_MS = 60;
  const bubble = new Bubble();
  let el = $state<HTMLElement | null>(null);
  const { startDrag, endDrag } = createBubbleDrag(bubble, () => el);
  /** Pin always-on-top (misma semántica que agentes). */
  let pinned = $state(false);
  let workAreas = $state<Area[]>([]);
  /** Último ancla: re-colocar cuando llegan work areas. */
  let lastOpen: BubbleOpen | null = null;

  type RevealPhase =
    | "hidden"
    | "expand"
    | "separate"
    | "ready"
    | "approach"
    | "shrink";
  let revealPhase = $state<RevealPhase>("hidden");
  let revealEpoch = 0;
  let closing = false;
  let ignoreIpcDismiss = false;
  const expanding = $derived(
    revealPhase === "expand" || revealPhase === "shrink",
  );
  const separating = $derived(
    revealPhase === "separate" || revealPhase === "approach",
  );
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

  function applyRestingPlace(a: BubbleOpen) {
    const pill = surfaces.live["pill-skin"] ?? surfaces.live["pill"];
    if (!pill) {
      bubble.place(a);
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
  }

  function placeFusedToPill(a: BubbleOpen) {
    const pill = surfaces.live["pill-skin"] ?? surfaces.live["pill"];
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

    revealPhase = "approach";
    await tick();
    await waitFrames(2);
    if (epoch !== revealEpoch) return;
    if (full && pill) {
      bubble.place({
        ...full,
        ...placePanelFusedFull(
          pill,
          { w: full.w, h: full.h },
          side,
          { corner: CORNER, work: workAreas, fusedGap: FUSED_GAP_PX },
        ),
      });
    } else if (full) {
      applyRestingPlace(full);
    }
    await afterTransition(el, separateAxisProp(side), separateDur);
    if (epoch !== revealEpoch) return;

    revealPhase = "shrink";
    await tick();
    await waitFrames(2);
    if (epoch !== revealEpoch) return;
    if (full) placeFusedToPill(full);
    await afterTransition(el, "width", openDur);
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
      liquid.publish("clipboard", []);
      return;
    }
    void bubble.shown;
    void revealPhase;
    void bubble.anchor;
    if (motionPhase) {
      return publishFollowSkin("clipboard", el, CORNER);
    }
    return publishEmergeSkin("clipboard", el, CORNER);
  });

  $effect(() => {
    if (!el || !bubble.alive) return;
    const stop = surfaces.add("clipboard", el);
    void surfaces.flush();
    return stop;
  });
  $effect(() => {
    if (!bubble.alive || !bubble.shown) return;
    void bubble.anchor;
    void surfaces.recoverHits();
    const t = window.setTimeout(() => {
      void surfaces.recoverHits();
    }, ms(MOTION.floatOpen) + 48);
    return () => window.clearTimeout(t);
  });
  $effect(() => {
    void bubble.anchor;
    void surfaces.dragging;
    if (surfaces.dragging) return;
    surfaces.schedule();
  });

  $effect(() => sessionEffect(["clipboard"]));

  async function togglePin() {
    const next = !pinned;
    pinned = next;
    try {
      await setClipboardAlwaysOnTop(next);
    } catch {
      pinned = !next;
    }
  }

  function finishDismiss(
    wasShown: boolean,
    opts: { skipHideWindow?: boolean } = {},
  ) {
    lastOpen = null;
    revealPhase = "hidden";
    endDrag();
    surfaces.resetInteraction();
    armCloseDur();
    bubble.hide();
    if (!wasShown) bubble.alive = false;
    if (!opts.skipHideWindow) {
      ignoreIpcDismiss = true;
      void hideClipboardWindow().finally(() => {
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
    void clipboardAlwaysOnTop()
      .then((on) => {
        if (on || !bubble.shown) return;
        close();
      })
      .catch(() => {
        /* sin lectura del pin, no cerrar */
      });
  }

  onMount(() => {
    void clipboardAlwaysOnTop()
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
      onClipboardBubbleAnchor((a) => {
        void placeFromPill(a);
      }),
      onClipboardBubbleDismiss(() => {
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
      void clipboardAlwaysOnTop()
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
      liquid.publish("clipboard", []);
    };
  });
</script>

{#if bubble.alive}
  <div
    class="cf"
    class:is-shown={bubble.shown}
    class:is-joined={joined}
    class:is-expanding={expanding}
    class:is-separating={separating}
    data-side={bubble.anchor?.side ?? "top"}
    style={bubble.vars}
    style:--launcher-bar-open-dur="{openDur}ms"
    style:--launcher-separate-dur="{separateDur}ms"
    style:--float-close-dur="{closeDur}ms"
    bind:this={el}
    role="dialog"
    aria-label={t("tools.clipboard.label")}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header class="cf-head" onpointerdown={startDrag}>
      <div class="cf-titles">
        <h2 class="cf-title">{t("tools.clipboard.label")}</h2>
        <p class="cf-hint">{t("overlay.clipboardHint")}</p>
      </div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="cf-acts" data-no-drag onpointerdown={(e) => e.stopPropagation()}>
        <button
          type="button"
          class="cf-icon"
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
          class="cf-icon"
          onclick={() => void close()}
          aria-label={t("overlay.close")}
          use:tip={t("overlay.close")}
        >
          <Icon icon={X} size={14} />
        </button>
      </div>
    </header>
    <div class="cf-body">
      <ClipboardHistoryList
        items={clipboard.items}
        loading={false}
        compact
        onRefresh={() => void clipboard.hydrate()}
        onError={(message) => toasts.push(message, 4200)}
      />
    </div>
    <ToastStack
      placement="local"
      items={toasts.items}
      onDismiss={(id) => toasts.dismiss(id)}
    />
  </div>
{/if}

<style>
  /*
   * Fused grow → separate: nace seed pegado a la pill, crece w/h con borde
   * clavado, luego se aleja (gap > REACH) y corta el cuello.
   */
  .cf {
    /* Duraciones heredadas de :root (app.css); sin overrides locales. */
    position: absolute;
    z-index: var(--z-overlay-float);
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
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--float-close-dur) var(--ease-smooth-out);
  }

  .cf.is-shown {
    opacity: 1;
    pointer-events: auto;
  }

  .cf.is-expanding {
    transition:
      width var(--launcher-bar-open-dur) var(--ease-smooth-out),
      height var(--launcher-bar-open-dur) var(--ease-smooth-out),
      left var(--launcher-bar-open-dur) var(--ease-smooth-out),
      top var(--launcher-bar-open-dur) var(--ease-smooth-out),
      opacity var(--float-close-dur) var(--ease-smooth-out);
  }

  /* Semilla = silueta líquida; chrome visible solo fuera del grow/shrink. */
  .cf.is-expanding .cf-head,
  .cf.is-expanding .cf-body {
    opacity: 0;
    pointer-events: none;
  }

  .cf.is-separating {
    transition:
      left var(--launcher-separate-dur) var(--ease-smooth-out),
      top var(--launcher-separate-dur) var(--ease-smooth-out),
      width var(--duration-quick) var(--ease-smooth-out),
      height var(--duration-quick) var(--ease-smooth-out),
      opacity var(--float-close-dur) var(--ease-smooth-out);
  }

  .cf-head {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    margin-bottom: 0.35rem;
    min-height: 2rem;
    cursor: grab;
    touch-action: none;
    user-select: none;
  }

  .cf-head:active {
    cursor: grabbing;
  }

  .cf-titles {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.05rem;
  }

  .cf-title {
    margin: 0;
    min-width: 0;
    font-size: 0.75rem;
    font-weight: 650;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--muted);
    text-wrap: balance;
  }

  .cf-hint {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    font-size: 0.625rem;
    font-weight: 500;
    line-height: 1.2;
    color: var(--faint);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .cf-acts {
    position: relative;
    z-index: 2;
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.15rem;
  }

  .cf-icon {
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

  .cf-icon :global(svg) {
    pointer-events: none;
  }

  .cf-icon:hover,
  .cf-icon.is-on {
    color: var(--text);
    background: color-mix(in srgb, var(--text) 8%, transparent);
  }

  .cf-icon:active {
    transform: scale(0.96);
  }

  .cf-body {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    overflow: hidden;
  }

  @media (prefers-reduced-motion: reduce) {
    .cf,
    .cf.is-expanding,
    .cf.is-separating {
      transition: none;
    }

    .cf-icon:active {
      transform: none;
    }
  }
</style>
