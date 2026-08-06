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
    hideClipboardWindow,
    onClipboardBubbleAnchor,
    onClipboardBubbleDismiss,
  } from "$ipc/clipboard";
  import { onOverlayDismiss } from "$ipc/overlay";
  import { Bubble } from "$surfaces/overlay/bubble.svelte";
  import { boxShape, gapBetween } from "$lib/liquid/geometry";
  import { REACH } from "$lib/liquid/constants";
  import { liquid } from "$surfaces/overlay/group.svelte";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";

  const CORNER = 18;
  const bubble = new Bubble();
  let el = $state<HTMLElement | null>(null);

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

  function close() {
    if (!bubble.shown) return;
    bubble.hide();
    void hideClipboardWindow();
  }

  onMount(() => {
    const un: Promise<() => void>[] = [
      onClipboardBubbleAnchor((a) => bubble.place(a)),
      onClipboardBubbleDismiss(() => {
        bubble.hide();
      }),
      onOverlayDismiss(() => {
        if (bubble.shown) close();
      }),
    ];
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && bubble.shown) {
        e.preventDefault();
        close();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
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
    <header class="cf-head">
      <h2 class="cf-title">Clipboard</h2>
      <button type="button" class="cf-close" onclick={close} aria-label="Cerrar">
        <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
          <path
            d="M6 6l12 12M18 6L6 18"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
          />
        </svg>
      </button>
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
    z-index: 2;
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
  }

  .cf-title {
    margin: 0;
    font-size: 0.75rem;
    font-weight: 650;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--rb-muted);
    text-wrap: balance;
  }

  .cf-close {
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    border: 0;
    border-radius: 999px;
    padding: 0;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .cf-close:hover {
    color: var(--rb-text);
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .cf-close:active {
    transform: scale(0.96);
  }

  .cf-body {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    overflow: hidden;
  }
</style>
