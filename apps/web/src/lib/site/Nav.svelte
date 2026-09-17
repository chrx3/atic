<script lang="ts">
  /**
   * La barra del sitio: marca, secciones, selector de tema y GitHub.
   * El selector usa las mismas paletas que la app; ver `theme.svelte.ts`.
   */
  import AticMark from "$lib/atic/AticMark.svelte";
  import Icon from "$lib/atic/Icon.svelte";
  import GithubIcon from "$lib/atic/GithubIcon.svelte";
  import { ChevronDown, Check, Sun, Moon } from "$lib/atic/icons";
  import { THEMES, theme } from "./theme.svelte";

  let open = $state(false);
  let menu = $state<HTMLDivElement | null>(null);

  const current = $derived(THEMES.find((item) => item.id === theme.current) ?? THEMES[0]);

  function choose(id: (typeof THEMES)[number]["id"]) {
    theme.set(id);
    open = false;
  }

  function onWindowPointer(event: PointerEvent) {
    if (!open) return;
    if (menu && !menu.contains(event.target as Node)) open = false;
  }

  const SWATCH: Record<string, [string, string]> = {
    dark: ["#121211", "#e8e8e0"],
    light: ["#ecece6", "#1a1a17"],
    graphite: ["#2a2a27", "#ebebe3"],
    midnight: ["#1a1e26", "#dde4f0"],
    sepia: ["#ded4c1", "#302818"],
    mist: ["#d6dade", "#21262e"],
    claude: ["#eae8e0", "#c15f3c"],
    "claude-dark": ["#1f1e1c", "#c15f3c"],
  };
</script>

<svelte:window onpointerdown={onWindowPointer} />

<header class="nav">
  <div class="wrap">
    <a class="brand" href="#top">
      <span class="brand-mark"><AticMark size={26} alive={true} /></span>
      <span class="brand-name">Atic</span>
    </a>

    <nav class="links" aria-label="Secciones">
      <a href="#demo">Demo</a>
      <a href="#herramientas">Herramientas</a>
      <a href="#videos">Videos</a>
      <a href="#privacidad">Privacidad</a>
    </nav>

    <div class="actions">
      <div class="theme" bind:this={menu}>
        <button
          type="button"
          class="theme-btn"
          aria-haspopup="menu"
          aria-expanded={open}
          onclick={() => (open = !open)}
        >
          <span class="swatch" style="background: {SWATCH[current.id][0]}">
            <i style="background: {SWATCH[current.id][1]}"></i>
          </span>
          <span class="theme-name">{current.label}</span>
          <Icon icon={ChevronDown} size={13} />
        </button>
        {#if open}
          <div class="theme-menu" role="menu">
            {#each THEMES as item (item.id)}
              <button
                type="button"
                role="menuitem"
                class="theme-item"
                class:is-on={item.id === theme.current}
                onclick={() => choose(item.id)}
              >
                <span class="swatch" style="background: {SWATCH[item.id][0]}">
                  <i style="background: {SWATCH[item.id][1]}"></i>
                </span>
                <span class="theme-item-name">{item.label}</span>
                {#if item.id === theme.current}
                  <Icon icon={Check} size={13} />
                {:else}
                  <span class="theme-base">
                    <Icon icon={item.base === "dark" ? Moon : Sun} size={12} />
                  </span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <a class="ghost" href="https://github.com/chrx3/atic" target="_blank" rel="noreferrer">
        <GithubIcon size={15} />
        <span>GitHub</span>
      </a>

      <a class="cta" href="https://github.com/chrx3/atic/releases/latest" target="_blank" rel="noreferrer">
        Descargar
      </a>
    </div>
  </div>
</header>

<style>
  .nav {
    position: sticky;
    top: 0;
    z-index: var(--z-sticky);
    border-bottom: 1px solid var(--line);
    background: color-mix(in srgb, var(--bg) 86%, transparent);
    backdrop-filter: blur(14px);
  }

  .wrap {
    display: flex;
    max-width: 1080px;
    height: 58px;
    align-items: center;
    gap: 18px;
    margin: 0 auto;
    padding: 0 20px;
  }

  .brand {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--text);
    text-decoration: none;
  }

  .brand-mark {
    display: grid;
    width: 34px;
    height: 34px;
    place-items: center;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--skin);
  }

  .brand-name {
    font-size: var(--text-md);
    font-weight: var(--font-weight-semibold);
    letter-spacing: -0.01em;
  }

  .links {
    display: flex;
    flex: 1;
    gap: 4px;
  }

  .links a {
    border-radius: var(--radius-sm);
    padding: 6px 10px;
    color: var(--muted);
    font-size: var(--text-xs);
    text-decoration: none;
  }

  .links a:hover {
    background: color-mix(in srgb, var(--text) 7%, transparent);
    color: var(--text);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .ghost,
  .cta {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border-radius: var(--radius-sm);
    padding: 7px 12px;
    font-size: var(--text-xs);
    text-decoration: none;
  }

  .ghost {
    border: 1px solid var(--line);
    color: var(--muted);
  }

  .ghost:hover {
    border-color: var(--line-strong);
    color: var(--text);
  }

  .cta {
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    background: var(--accent);
    color: var(--on-accent);
  }

  .theme {
    position: relative;
  }

  .theme-btn {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    padding: 6px 9px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--text-xs);
    cursor: pointer;
  }

  .theme-btn:hover {
    border-color: var(--line-strong);
    color: var(--text);
  }

  .swatch {
    position: relative;
    display: inline-block;
    width: 16px;
    height: 16px;
    overflow: hidden;
    flex-shrink: 0;
    border: 1px solid var(--line-strong);
    border-radius: 5px;
  }

  .swatch i {
    position: absolute;
    right: 2px;
    bottom: 2px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }

  .theme-menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: var(--z-popover);
    display: flex;
    width: 210px;
    flex-direction: column;
    gap: 1px;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    padding: 5px;
    background: var(--surface);
    box-shadow: var(--shadow-pop);
  }

  .theme-item {
    display: flex;
    align-items: center;
    gap: 9px;
    border: 0;
    border-radius: var(--radius-sm);
    padding: 7px 8px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--text-xs);
    text-align: left;
    cursor: pointer;
  }

  .theme-item:hover,
  .theme-item.is-on {
    background: color-mix(in srgb, var(--text) 7%, transparent);
    color: var(--text);
  }

  .theme-item-name {
    flex: 1;
  }

  .theme-base {
    display: grid;
    place-items: center;
    color: var(--faint);
  }

  @media (max-width: 720px) {
    .links,
    .theme-name {
      display: none;
    }
  }
</style>
