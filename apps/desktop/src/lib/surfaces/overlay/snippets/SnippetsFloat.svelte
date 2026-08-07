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
    setSnippetsAlwaysOnTop,
    snippetsAlwaysOnTop,
  } from "$ipc/snippets";
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
  let tab = $state<"list" | "scratchpad">("list");
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

  async function togglePin() {
    const next = !pinned;
    pinned = next;
    try {
      await setSnippetsAlwaysOnTop(next);
    } catch {
      pinned = !next;
    }
  }

  function close() {
    if (!bubble.shown) return;
    endDrag();
    snippets.flushScratchpad();
    bubble.hide();
    void hideSnippetsWindow();
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
        pinned = true;
      });
    const un: Promise<() => void>[] = [
      onSnippetsBubbleAnchor((a) => bubble.place(a)),
      onSnippetsBubbleDismiss(() => {
        snippets.flushScratchpad();
        bubble.hide();
      }),
      onOverlayDismiss(() => {
        tryAutoClose();
      }),
    ];
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !bubble.shown) return;
      e.preventDefault();
      void snippetsAlwaysOnTop()
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
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header class="sf-head" onpointerdown={startDrag}>
      <div class="sf-tabs" role="tablist" aria-label="Textos y notas" data-no-drag>
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
      <!-- Zona de arrastre entre tabs y acciones. -->
      <div class="sf-drag" aria-hidden="true"></div>
      <div class="sf-acts" data-no-drag>
        <button
          type="button"
          class="sf-icon"
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
          class="sf-icon"
          onclick={close}
          aria-label="Cerrar"
          title="Cerrar"
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
    .sf-icon:active,
    .sf-tab:active {
      transform: none;
    }
  }
</style>
