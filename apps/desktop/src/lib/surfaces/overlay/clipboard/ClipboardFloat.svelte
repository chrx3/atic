<script lang="ts">
  /**
   * Float de clipboard: hermano de la pill, fundido al liquid.
   * La pill ya no crece; esto sale con `.float-emerge`.
   */
  import { onMount } from "svelte";
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
  import { onOverlayDismiss } from "$ipc/overlay";
  import { Bubble } from "$surfaces/overlay/bubble.svelte";
  import { createBubbleDrag } from "$surfaces/overlay/bubbleDrag";
  import { boxShape, gapBetween } from "$lib/liquid/geometry";
  import { REACH } from "$lib/liquid/constants";
  import { liquid } from "$surfaces/overlay/group.svelte";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import Icon from "$ui/Icon.svelte";
  import { Pin, X } from "$lib/icons";

  const CORNER = 18;
  const bubble = new Bubble();
  let el = $state<HTMLElement | null>(null);
  const { startDrag, endDrag } = createBubbleDrag(bubble, () => el);
  /** Pin always-on-top (misma semántica que agentes). */
  let pinned = $state(true);

  const pillSkin = $derived(surfaces.live["pill-skin"]);
  const joined = $derived.by(() => {
    const a = bubble.anchor;
    const p = pillSkin;
    if (!a || !p || !bubble.alive) return false;
    return gapBetween(p, a) <= REACH;
  });

  $effect(() => {
    if (!bubble.alive || !bubble.anchor) {
      liquid.publish("clipboard", []);
      return;
    }
    liquid.publish("clipboard", [boxShape(bubble.anchor, CORNER)]);
  });

  $effect(() =>
    el && bubble.shown ? surfaces.add("clipboard", el) : undefined,
  );
  $effect(() => {
    void bubble.anchor;
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

  function close() {
    if (!bubble.shown) return;
    endDrag();
    bubble.hide();
    void hideClipboardWindow();
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
        pinned = true;
      });
    const un: Promise<() => void>[] = [
      onClipboardBubbleAnchor((a) => bubble.place(a)),
      onClipboardBubbleDismiss(() => {
        bubble.hide();
      }),
      onOverlayDismiss(() => {
        tryAutoClose();
      }),
    ];
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !bubble.shown) return;
      e.preventDefault();
      void clipboardAlwaysOnTop()
        .then((on) => {
          if (!on && bubble.shown) close();
        })
        .catch(() => {
          /* sin pin, no cerrar */
        });
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      endDrag();
      for (const p of un) void p.then((fn) => fn());
      liquid.publish("clipboard", []);
    };
  });
</script>

{#if bubble.alive}
  <div
    class="cf float-emerge"
    class:is-shown={bubble.shown}
    class:is-joined={joined}
    data-side={bubble.anchor?.side ?? "top"}
    style={bubble.vars}
    bind:this={el}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header class="cf-head" onpointerdown={startDrag}>
      <h2 class="cf-title">Clipboard</h2>
      <div class="cf-acts" data-no-drag>
        <button
          type="button"
          class="cf-icon"
          class:is-on={pinned}
          aria-label={pinned ? "Desfijar" : "Fijar arriba"}
          aria-pressed={pinned}
          title={pinned ? "Desfijar" : "Fijar arriba"}
          onclick={() => void togglePin()}
        >
          <Icon icon={Pin} size={13} />
        </button>
        <button
          type="button"
          class="cf-icon"
          onclick={close}
          aria-label="Cerrar"
          title="Cerrar"
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
        onPasted={close}
      />
    </div>
  </div>
{/if}

<style>
  .cf {
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
    color: var(--rb-text);
    overflow: hidden;
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

  .cf-title {
    margin: 0;
    flex: 1;
    min-width: 0;
    font-size: 0.75rem;
    font-weight: 650;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--rb-muted);
    text-wrap: balance;
  }

  .cf-acts {
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

  .cf-icon:hover,
  .cf-icon.is-on {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
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
    .cf-icon:active {
      transform: none;
    }
  }
</style>
