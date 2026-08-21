<script lang="ts">
  /**
   * Float de textos / notas: hermano de la pill, fundido al liquid.
   *
   * Apertura (pill-liquid-emerge): nace fused chico → crece w/h → se separa
   * hasta cortar el cuello. Cierre = reverse: approach → shrink → dismiss.
   */
  import { onMount, tick } from "svelte";
  import SnippetsList from "$lib/SnippetsList.svelte";
  import { snippets } from "$domain/snippets.svelte";
  import { sessionEffect } from "$domain/session";
  import {
    afterTransition,
    MOTION,
    ms,
    prefersReducedMotion,
    tabPanel,
    wait,
  } from "$lib/motion";
  import {
    hideSnippetsWindow,
    onSnippetsBubbleAnchor,
    onSnippetsBubbleDismiss,
    setSnippetsAlwaysOnTop,
    snippetsAlwaysOnTop,
  } from "$ipc/snippets";
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
  import Icon from "$ui/Icon.svelte";
  import { t } from "$domain/i18n.svelte";
  import { Pin, X } from "$lib/icons";

  const CORNER = 20;
  const SEED_HOLD_MS = 60;
  const bubble = new Bubble();
  let el = $state<HTMLElement | null>(null);
  const { startDrag, endDrag } = createBubbleDrag(bubble, () => el);
  let tab = $state<"list" | "scratchpad">("list");
  /** Pin always-on-top (misma semántica que agentes). */
  let pinned = $state(false);
  let workAreas = $state<Area[]>([]);
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
      liquid.publish("snippets", []);
      return;
    }
    void bubble.shown;
    void revealPhase;
    void bubble.anchor;
    if (motionPhase) {
      return publishFollowSkin("snippets", el, CORNER);
    }
    return publishEmergeSkin("snippets", el, CORNER);
  });

  $effect(() => {
    if (!el || !bubble.alive) return;
    const stop = surfaces.add("snippets", el);
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

  $effect(() => sessionEffect(["snippets"]));

  async function togglePin() {
    const next = !pinned;
    pinned = next;
    try {
      await setSnippetsAlwaysOnTop(next);
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
    snippets.flushScratchpad();
    armCloseDur();
    bubble.hide();
    if (!wasShown) bubble.alive = false;
    if (!opts.skipHideWindow) {
      ignoreIpcDismiss = true;
      void hideSnippetsWindow().finally(() => {
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
    void snippetsAlwaysOnTop()
      .then((on) => {
        if (on || !bubble.shown) return;
        close();
      })
      .catch(() => {
        /* sin lectura del pin, no cerrar */
      });
  }

  onMount(() => {
    void snippetsAlwaysOnTop()
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
      onSnippetsBubbleAnchor((a) => {
        void placeFromPill(a);
      }),
      onSnippetsBubbleDismiss(() => {
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
      void snippetsAlwaysOnTop()
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
      liquid.publish("snippets", []);
    };
  });
</script>

{#if bubble.alive}
  <div
    class="sf"
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
    aria-label={t("overlay.snippets")}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header class="sf-head" onpointerdown={startDrag}>
      <div class="sf-tabs" role="tablist" aria-label={t("overlay.snippets")} data-no-drag>
        <button
          type="button"
          role="tab"
          class="sf-tab"
          class:active={tab === "list"}
          aria-selected={tab === "list"}
          onclick={() => (tab = "list")}
        >
          {t("overlay.texts")}
        </button>
        <button
          type="button"
          role="tab"
          class="sf-tab"
          class:active={tab === "scratchpad"}
          aria-selected={tab === "scratchpad"}
          onclick={() => (tab = "scratchpad")}
        >
          {t("overlay.notes")}
        </button>
      </div>
      <!-- Zona de arrastre entre tabs y acciones. -->
      <div class="sf-drag" aria-hidden="true"></div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="sf-acts" data-no-drag onpointerdown={(e) => e.stopPropagation()}>
        <button
          type="button"
          class="sf-icon"
          class:is-on={pinned}
          aria-label={pinned ? t("overlay.unpin") : t("overlay.pin")}
          aria-pressed={pinned}
          title={pinned ? t("overlay.unpin") : t("overlay.pin")}
          onclick={() => void togglePin()}
        >
          <Icon icon={Pin} size={13} />
        </button>
        <button
          type="button"
          class="sf-icon"
          onclick={() => void close()}
          aria-label={t("overlay.close")}
          title={t("overlay.close")}
        >
          <Icon icon={X} size={14} />
        </button>
      </div>
    </header>
    <div class="sf-body">
      {#if tab === "list"}
        <div class="sf-pane" in:tabPanel|local out:tabPanel|local>
          <SnippetsList
            items={snippets.items}
            loading={false}
            compact
            onRefresh={() => void snippets.hydrate()}
            onPasted={() => void close()}
          />
        </div>
      {:else}
        <div class="sf-pane" in:tabPanel|local out:tabPanel|local>
          <textarea
            class="sf-scratch"
            value={snippets.scratchpad?.body ?? ""}
            oninput={(e) => snippets.editScratchpad(e.currentTarget.value)}
            placeholder={t("overlay.scratchPlaceholder")}
            aria-label={t("overlay.scratchAria")}
          ></textarea>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  /*
   * Fused grow → separate: nace seed pegado a la pill, crece w/h con borde
   * clavado, luego se aleja (gap > REACH) y corta el cuello.
   */
  .sf {
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
    /* Opaco debajo del skin (mismo patrón que AgentsDemo): el fondo no
       desaparece si el goo va un frame atras al mover. */
    background: var(--rb-surface);
    color: var(--rb-text);
    overflow: hidden;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--float-close-dur) var(--ease-smooth-out);
  }

  .sf.is-shown {
    opacity: 1;
    pointer-events: auto;
  }

  .sf.is-expanding {
    transition:
      width var(--launcher-bar-open-dur) var(--ease-smooth-out),
      height var(--launcher-bar-open-dur) var(--ease-smooth-out),
      left var(--launcher-bar-open-dur) var(--ease-smooth-out),
      top var(--launcher-bar-open-dur) var(--ease-smooth-out),
      opacity var(--float-close-dur) var(--ease-smooth-out);
  }

  /* Semilla = silueta líquida; chrome visible solo fuera del grow/shrink. */
  .sf.is-expanding .sf-head,
  .sf.is-expanding .sf-body {
    opacity: 0;
    pointer-events: none;
  }

  .sf.is-separating {
    transition:
      left var(--launcher-separate-dur) var(--ease-smooth-out),
      top var(--launcher-separate-dur) var(--ease-smooth-out),
      width var(--duration-quick) var(--ease-smooth-out),
      height var(--duration-quick) var(--ease-smooth-out),
      opacity var(--float-close-dur) var(--ease-smooth-out);
  }

  .sf-head {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.35rem;
    margin-bottom: 0.35rem;
    min-height: 2rem;
    cursor: grab;
    touch-action: none;
    user-select: none;
  }

  .sf-head:active {
    cursor: grabbing;
  }

  .sf-tabs {
    display: flex;
    flex-shrink: 0;
    gap: 0.3rem;
  }

  .sf-drag {
    flex: 1;
    min-width: 0.75rem;
    align-self: stretch;
  }

  .sf-tab {
    min-height: 1.75rem;
    border: 0;
    border-radius: 999px;
    padding: 0.2rem 0.55rem;
    background: transparent;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    font-weight: 600;
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .sf-tab:active {
    transform: scale(0.96);
  }

  .sf-tab.active {
    background: color-mix(in sRGB, var(--rb-accent, var(--accent)) 12%, transparent);
    color: var(--rb-accent, var(--accent));
  }

  .sf-acts {
    position: relative;
    z-index: 2;
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.15rem;
  }

  .sf-icon {
    display: grid;
    place-items: center;
    box-sizing: border-box;
    width: 1.75rem;
    height: 1.75rem;
    border: 1px solid transparent;
    border-radius: 0.4rem;
    padding: 0;
    background: transparent;
    color: var(--rb-faint, var(--rb-muted));
    cursor: pointer;
    box-shadow: none;
    filter: none;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      border-color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .sf-icon :global(svg) {
    pointer-events: none;
  }

  .sf-icon:hover,
  .sf-icon.is-on {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
  }

  .sf-icon:active {
    transform: scale(0.96);
  }

  .sf-body {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    overflow: hidden;
  }

  .sf-pane {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
  }

  .sf-scratch {
    min-height: 0;
    flex: 1;
    border: 0;
    border-radius: 0.45rem;
    padding: 0.4rem 0.5rem;
    background: color-mix(in sRGB, var(--rb-bg0, var(--bg)) 80%, transparent);
    color: var(--rb-text);
    font-family: inherit;
    font-size: 0.75rem;
    resize: none;
    outline: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .sf,
    .sf.is-expanding,
    .sf.is-separating {
      transition: none;
    }

    .sf-icon:active,
    .sf-tab:active {
      transform: none;
    }
  }
</style>
