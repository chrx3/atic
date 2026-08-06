<script lang="ts">
  /**
   * Float de textos / notas: hermano de la pill, fundido al liquid.
   */
  import { onMount } from "svelte";
  import SnippetsList from "$lib/SnippetsList.svelte";
  import { snippets } from "$domain/snippets.svelte";
  import { sessionEffect } from "$domain/session";
  import { tabPanel } from "$lib/motion";
  import {
    hideSnippetsWindow,
    onSnippetsBubbleAnchor,
    onSnippetsBubbleDismiss,
  } from "$ipc/snippets";
  import { onOverlayDismiss } from "$ipc/overlay";
  import { Bubble } from "$surfaces/overlay/bubble.svelte";
  import { boxShape, gapBetween } from "$lib/liquid/geometry";
  import { REACH } from "$lib/liquid/constants";
  import { liquid } from "$surfaces/overlay/group.svelte";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";

  const CORNER = 18;
  const bubble = new Bubble();
  let el = $state<HTMLElement | null>(null);
  let tab = $state<"list" | "scratchpad">("list");

  const pillSkin = $derived(surfaces.live["pill-skin"]);
  const joined = $derived.by(() => {
    const a = bubble.anchor;
    const p = pillSkin;
    if (!a || !p || !bubble.alive) return false;
    return gapBetween(p, a) <= REACH;
  });

  $effect(() => {
    if (!bubble.alive || !bubble.anchor) {
      liquid.publish("snippets", []);
      return;
    }
    liquid.publish("snippets", [boxShape(bubble.anchor, CORNER)]);
  });

  $effect(() =>
    el && bubble.shown ? surfaces.add("snippets", el) : undefined,
  );
  $effect(() => {
    void bubble.anchor;
    surfaces.schedule();
  });

  $effect(() => sessionEffect(["snippets"]));

  function close() {
    if (!bubble.shown) return;
    snippets.flushScratchpad();
    bubble.hide();
    void hideSnippetsWindow();
  }

  onMount(() => {
    const un: Promise<() => void>[] = [
      onSnippetsBubbleAnchor((a) => bubble.place(a)),
      onSnippetsBubbleDismiss(() => {
        snippets.flushScratchpad();
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
      liquid.publish("snippets", []);
    };
  });
</script>

{#if bubble.alive}
  <div
    class="sf float-emerge"
    class:is-shown={bubble.shown}
    class:is-joined={joined}
    data-side={bubble.anchor?.side ?? "top"}
    style={bubble.vars}
    bind:this={el}
  >
    <header class="sf-head">
      <div class="sf-tabs" role="tablist" aria-label="Textos y notas">
        <button
          type="button"
          role="tab"
          class="sf-tab"
          class:active={tab === "list"}
          aria-selected={tab === "list"}
          onclick={() => (tab = "list")}
        >
          Textos
        </button>
        <button
          type="button"
          role="tab"
          class="sf-tab"
          class:active={tab === "scratchpad"}
          aria-selected={tab === "scratchpad"}
          onclick={() => (tab = "scratchpad")}
        >
          Notas
        </button>
      </div>
      <button type="button" class="sf-close" onclick={close} aria-label="Cerrar">
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
    <div class="sf-body">
      {#if tab === "list"}
        <div class="sf-pane" in:tabPanel|local out:tabPanel|local>
          <SnippetsList
            items={snippets.items}
            loading={false}
            compact
            onRefresh={() => void snippets.hydrate()}
            onPasted={close}
          />
        </div>
      {:else}
        <div class="sf-pane" in:tabPanel|local out:tabPanel|local>
          <textarea
            class="sf-scratch"
            value={snippets.scratchpad?.body ?? ""}
            oninput={(e) => snippets.editScratchpad(e.currentTarget.value)}
            placeholder="Notas temporales…"
            aria-label="Bloc de notas"
          ></textarea>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .sf {
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

  .sf-head {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    margin-bottom: 0.35rem;
    min-height: 2rem;
  }

  .sf-tabs {
    display: flex;
    gap: 0.3rem;
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

  .sf-close {
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

  .sf-close:hover {
    color: var(--rb-text);
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .sf-close:active {
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
</style>
