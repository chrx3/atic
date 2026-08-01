<script lang="ts">
  import type { Snippet } from "svelte";
  import Titlebar from "$lib/Titlebar.svelte";
  import AppNavIcon from "$lib/AppNavIcon.svelte";
  import AticMark from "$lib/AticMark.svelte";
  import LiquidSkin from "$lib/floating-v2/liquid/LiquidSkin.svelte";
  import {
    APP_SECTIONS,
    sectionById,
    type AppSectionId,
  } from "$lib/tools";
  import type { UiTheme } from "$lib/theme";

  let {
    section = $bindable<AppSectionId>("home"),
    theme = "system",
    onToggleTheme,
    children,
  }: {
    section?: AppSectionId;
    theme?: UiTheme;
    onToggleTheme?: () => void;
    children: Snippet;
  } = $props();

  const activeIndex = $derived(
    Math.max(0, APP_SECTIONS.findIndex((item) => item.id === section)),
  );
  const current = $derived(sectionById(section));

  const verticalBoxes = $derived([
    { id: "pill", cx: 31, cy: 186, hw: 23, hh: 178, r: 23 },
    {
      id: "active",
      cx: 94,
      cy: 31 + activeIndex * 62,
      hw: 79,
      hh: 23,
      r: 23,
    },
  ]);
  const verticalBridges = $derived([
    {
      id: "nav-bridge",
      ax: 31,
      ay: 31 + activeIndex * 62,
      bx: 94,
      by: 31 + activeIndex * 62,
      r: 18,
    },
  ]);

  const horizontalBoxes = $derived([
    { id: "pill", cx: 210, cy: 49, hw: 204, hh: 23, r: 23 },
    {
      id: "active",
      cx: 35 + activeIndex * 70,
      cy: 27,
      hw: 28,
      hh: 25,
      r: 25,
    },
  ]);
  const horizontalBridges = $derived([
    {
      id: "nav-bridge",
      ax: 35 + activeIndex * 70,
      ay: 27,
      bx: 35 + activeIndex * 70,
      by: 49,
      r: 18,
    },
  ]);

  const liquidParameters = { k: 13, cell: 3, smooth: 2 };

  function choose(next: AppSectionId) {
    section = next;
    requestAnimationFrame(() => {
      document.querySelector<HTMLElement>("#main-content")?.focus();
    });
  }
</script>

<div class="atic-shell">
  <Titlebar
    tool={current}
    {theme}
    {onToggleTheme}
    onOpenSettings={() => choose("settings")}
  />

  <div class="atic-shell-body">
    <aside class="pearl-rail" aria-label="Navegación principal">
      <div class="pearl-rail-brand" aria-hidden="true">
        <AticMark size={18} />
        <span>Atic</span>
      </div>
      <div class="pearl-rail-stage">
        <LiquidSkin
          boxes={verticalBoxes}
          bridges={verticalBridges}
          parameters={liquidParameters}
          width={180}
          height={372}
          duration={230}
        />
        <nav class="pearl-rail-items">
          {#each APP_SECTIONS as item (item.id)}
            <button
              type="button"
              class="pearl-nav-item"
              class:is-active={section === item.id}
              aria-current={section === item.id ? "page" : undefined}
              title={`${item.label} — ${item.short}`}
              onclick={() => choose(item.id)}
            >
              <span class="pearl-nav-icon"><AppNavIcon id={item.id} /></span>
              <span class="pearl-nav-copy">
                <strong>{item.label}</strong>
                <small>{item.short}</small>
              </span>
            </button>
          {/each}
        </nav>
      </div>
    </aside>

    <div class="atic-shell-main">
      {@render children()}
    </div>

    <div class="pearl-dock-wrap">
      <div class="pearl-dock-stage">
        <LiquidSkin
          boxes={horizontalBoxes}
          bridges={horizontalBridges}
          parameters={liquidParameters}
          width={420}
          height={74}
          duration={230}
        />
        <nav class="pearl-dock" aria-label="Navegación principal">
          {#each APP_SECTIONS as item (item.id)}
            <button
              type="button"
              class="pearl-dock-item"
              class:is-active={section === item.id}
              aria-current={section === item.id ? "page" : undefined}
              aria-label={item.label}
              title={item.label}
              onclick={() => choose(item.id)}
            >
              <AppNavIcon id={item.id} size={19} />
              <span>{item.label}</span>
            </button>
          {/each}
        </nav>
      </div>
    </div>
  </div>
</div>

<style>
  .atic-shell {
    container-type: inline-size;
    container-name: atic-shell;
    display: flex;
    height: 100dvh;
    flex-direction: column;
    overflow: hidden;
    color: var(--rb-text);
    background: var(--rb-bg0);
  }

  .atic-shell-body {
    position: relative;
    display: flex;
    min-height: 0;
    flex: 1 1 auto;
  }

  .atic-shell-main {
    container-type: inline-size;
    container-name: atic-main;
    display: flex;
    min-width: 0;
    min-height: 0;
    flex: 1 1 auto;
    flex-direction: column;
    overflow: auto;
    overscroll-behavior: contain;
  }

  .pearl-rail {
    display: flex;
    width: 12rem;
    flex: 0 0 auto;
    flex-direction: column;
    padding: 0.8rem 0.45rem 0.8rem 0.35rem;
    background: color-mix(in oklch, var(--rb-bg1) 68%, var(--rb-bg0));
  }

  .pearl-rail-brand {
    display: flex;
    height: 2.25rem;
    align-items: center;
    gap: 0.5rem;
    padding-inline: 0.8rem;
    color: var(--rb-text);
    font-size: 0.8125rem;
    font-weight: 650;
  }

  .pearl-rail-stage {
    position: relative;
    width: 11.25rem;
    height: 23.25rem;
    margin-top: 0.5rem;
  }

  .pearl-rail-items {
    position: relative;
    z-index: 1;
    display: grid;
    height: 100%;
    grid-template-rows: repeat(6, 3.875rem);
  }

  .pearl-nav-item {
    display: grid;
    width: 100%;
    min-width: 0;
    grid-template-columns: 3.875rem minmax(0, 1fr);
    align-items: center;
    border: 0;
    padding: 0;
    color: var(--rb-muted);
    background: transparent;
    text-align: left;
    cursor: pointer;
  }

  .pearl-nav-icon {
    display: grid;
    width: 3.875rem;
    height: 3.875rem;
    place-items: center;
  }

  .pearl-nav-copy {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 0.12rem;
    padding-right: 0.65rem;
  }

  .pearl-nav-copy strong {
    color: inherit;
    font-size: 0.8125rem;
    font-weight: 620;
  }

  .pearl-nav-copy small {
    overflow: hidden;
    color: var(--rb-faint);
    font-size: 0.625rem;
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pearl-nav-item:hover,
  .pearl-nav-item.is-active {
    color: var(--rb-text);
  }

  .pearl-nav-item:focus-visible,
  .pearl-dock-item:focus-visible {
    z-index: 2;
    border-radius: 999px;
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .pearl-dock-wrap {
    display: none;
  }

  @container atic-shell (max-width: 47.999rem) {
    .pearl-rail {
      display: none;
    }

    .atic-shell-main {
      padding-bottom: 5.1rem;
    }

    .pearl-dock-wrap {
      position: absolute;
      z-index: 20;
      right: 0;
      bottom: max(0.25rem, env(safe-area-inset-bottom));
      left: 0;
      display: flex;
      justify-content: center;
      padding-inline: 0.4rem;
      pointer-events: none;
    }

    .pearl-dock-stage {
      position: relative;
      width: min(26.25rem, 100%);
      height: 4.625rem;
      pointer-events: auto;
    }

    .pearl-dock {
      position: relative;
      z-index: 1;
      display: grid;
      height: 100%;
      grid-template-columns: repeat(6, minmax(0, 1fr));
    }

    .pearl-dock-item {
      display: flex;
      min-width: 0;
      flex-direction: column;
      align-items: center;
      justify-content: flex-start;
      gap: 0.16rem;
      border: 0;
      padding: 0.72rem 0.08rem 0;
      color: var(--rb-muted);
      background: transparent;
      cursor: pointer;
    }

    .pearl-dock-item span {
      max-width: 100%;
      overflow: hidden;
      font-size: 0.5625rem;
      font-weight: 600;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .pearl-dock-item:hover,
    .pearl-dock-item.is-active {
      color: var(--rb-text);
    }
  }

  @container atic-shell (max-width: 28rem) {
    .pearl-dock-item span {
      display: none;
    }

    .pearl-dock-item {
      justify-content: flex-start;
      padding-top: 0.95rem;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .pearl-nav-item,
    .pearl-dock-item {
      transition: none;
    }
  }
</style>
