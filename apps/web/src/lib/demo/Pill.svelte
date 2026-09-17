<script lang="ts">
  /**
   * La pill y su rueda, en una sola pieza.
   *
   * Es la versión web de `PillSurface` + `ParticleWheel` (variante compacta):
   * en la app el disco cerrado y el núcleo de la rueda son la MISMA forma, y
   * por eso acá también lo son. Abrir no cambia de componente: las gotas se
   * desprenden del núcleo, con el cuello de goo cruzándose entre ellas.
   *
   * Medidas de `pillStage.ts` / `SKIN` de `ParticleWheel.svelte`: bar 52 (disco
   * en reposo), núcleo 58, gota 56 y anillo 65 (0.28 · 232). El hueco al
   * núcleo queda en 8 px, bajo el alcance del goo (1.72·σ ≈ 10.3 con σ = 6),
   * así que las gotas se leen como lóbulos del núcleo, no como botones
   * sueltos. Ángulos, curvas y escalonado son los de la app.
   */
  import AticMark from "$lib/atic/AticMark.svelte";
  import ToolIcon from "$lib/atic/ToolIcon.svelte";
  import { goo, type Goo } from "$lib/atic/liquid";
  import { nodePosition, wedgeClip } from "./wheelGeometry";
  import { navigatorLabel } from "./state.svelte";
  import type { AppIconId } from "$lib/atic/icons";

  /**
   * Un gajo de la rueda. No es `ToolDef`: la pill mete además el gajo «Más»
   * y «Ventana», que no son herramientas —no tienen acción, no viven en
   * `TOOLS`—. Por eso el id admite los especiales y el icono puede venir
   * aparte (misma convención que la app: el id de una herramienta ES su icono).
   */
  export type WheelNode = {
    id: AppIconId | string;
    label: string;
    short: string;
    icon?: AppIconId;
  };

  let {
    tools,
    open = false,
    onSelect,
    onToggle,
  }: {
    tools: readonly WheelNode[];
    open?: boolean;
    onSelect?: (id: string) => void;
    onToggle?: () => void;
  } = $props();

  const SIDE = 252; // `PILL.wheel`
  const CENTER = SIDE / 2;
  const CORE = 58;
  const NODE = 56;
  const RING = 65;
  /** El disco en reposo: `PILL.bar`. */
  const CLOSED = 52;
  const COUNT = $derived(tools.length);

  /** El atajo real de la rueda (`pill_radial_shortcut`, no Alt+Z). */
  const wheelShortcut = `${navigatorLabel()}+Shift+Espacio`;

  let skin = $state<HTMLDivElement | null>(null);
  let fusion: Goo | null = null;
  let active = $state<string | null>(null);

  /**
   * El disco sube y las gotas salen desde el centro, donde estaba el puntero:
   * sin una gracia, el cursor queda dentro del gajo de abajo y lo selecciona
   * solo. El hover arranca recién cuando el abanico terminó de abrirse.
   */
  const HOVER_GRACE = 280;
  let openedAt = 0;
  $effect(() => {
    if (open) {
      openedAt = performance.now();
      active = null;
    }
  });

  function canHover() {
    return performance.now() - openedAt > HOVER_GRACE;
  }

  $effect(() => {
    if (!skin) return;
    const instance = goo(skin, {
      // σ = 8: el alcance teórico (1.72·σ) vale para bordes rectos; entre
      // círculos rinde menos, y con σ = 6 el cuello no llegaba a cruzar los
      // 8 px al núcleo. La app usa SDF con alcance 12 para esta misma
      // geometría; en goo SVG hace falta más sigma para el mismo cuello.
      sigma: 8,
      shadow: "drop-shadow(0 10px 22px rgb(0 0 0 / 38%))",
    });
    fusion = instance;
    return () => {
      instance.destroy();
      fusion = null;
    };
  });

  function position(index: number) {
    return nodePosition(index, COUNT, { width: SIDE, height: SIDE }, RING);
  }

  const nodes = $derived(
    tools.map((tool, index) => ({
      tool,
      ...position(index),
      clip: wedgeClip(index, COUNT, { width: SIDE, height: SIDE }),
    })),
  );

  function pick(id: string) {
    onSelect?.(id);
  }

  /**
   * El icono del gajo. Por convención el id de una herramienta ES su id de
   * icono; «Más» y «Ventana» también están en el catálogo (`icons.ts`).
   */
  function iconOf(item: WheelNode): AppIconId {
    return item.icon ?? (item.id as AppIconId);
  }
</script>

<div class="pw" class:is-open={open} style="--side: {SIDE}px">
  <!-- La piel: núcleo + gotas. Nada de contenido acá adentro — el filtro
       difumina todo lo que tenga. -->
  <div class="skin" bind:this={skin} aria-hidden="true">
    <i
      class="blob core"
      style="--d: {CORE}px; --sc: {CLOSED / CORE}"
    ></i>
    {#each nodes as node, index (node.tool.id)}
      <i
        class="blob node"
        style="left: {node.x}px; top: {node.y}px;
               --d: {NODE}px;
               --tx: {CENTER - node.x}px; --ty: {CENTER - node.y}px;
               --sc: {CLOSED / NODE};
               --delay: calc({index} * var(--morph-stagger))"
      ></i>
    {/each}
  </div>

  <!-- El ink: iconos y marca. Intacto, encima de la silueta.
       Sin separadores ni rótulo: en la variante compacta de la app las gotas
       ya dividen el espacio y el nombre vive solo en el tooltip. -->
  <div class="ink">
    <div class="nodes" role="toolbar" aria-label="Herramientas de Atic">
      {#each nodes as node (node.tool.id)}
        <button
          type="button"
          class="hit"
          class:is-hot={active === node.tool.id}
          style="clip-path: {node.clip}"
          aria-label="{node.tool.label}. {node.tool.short}"
          title="{node.tool.label} — {node.tool.short}"
          tabindex={open ? 0 : -1}
          onpointerenter={() => {
            if (canHover()) active = node.tool.id;
          }}
          onpointerleave={() => {
            if (active === node.tool.id) active = null;
          }}
          onfocus={() => (active = node.tool.id)}
          onblur={() => {
            if (active === node.tool.id) active = null;
          }}
          onclick={() => pick(node.tool.id)}
        >
          <span class="node-body" style="left: {node.x}px; top: {node.y}px">
            <span class="node-icon">
              <ToolIcon id={iconOf(node.tool)} size={21} />
            </span>
          </span>
        </button>
      {/each}
    </div>

    <button
      type="button"
      class="core-btn"
      class:is-open={open}
      style="width: {open ? CORE : CLOSED}px; height: {open ? CORE : CLOSED}px"
      aria-label={open ? "Cerrar la rueda" : "Abrir la rueda de herramientas"}
      title={open ? "Cerrar" : `Herramientas — ${wheelShortcut}`}
      onclick={onToggle}
    >
      <span class="mark">
        <AticMark size={32} alive />
      </span>
    </button>
  </div>
</div>

<style>
  .pw {
    position: absolute;
    top: 50%;
    left: 50%;
    width: var(--side);
    height: var(--side);
    margin: calc(var(--side) / -2) 0 0 calc(var(--side) / -2);
    user-select: none;
  }

  /* ─── La piel ───────────────────────────────────────────────────────── */
  .skin {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }

  .blob {
    position: absolute;
    top: 50%;
    left: 50%;
    width: var(--d);
    height: var(--d);
    margin: calc(var(--d) / -2) 0 0 calc(var(--d) / -2);
    border-radius: 999px;
    background: var(--skin);
  }

  .core {
    /* Cerrado, el núcleo ES el disco de la pill: mismo tamaño, mismo sitio. */
    transform: scale(var(--sc));
    transition: transform var(--morph-close-dur) var(--morph-close-ease);
  }

  .node {
    transform: translate(var(--tx), var(--ty)) scale(var(--sc));
    transition: transform var(--morph-close-dur) var(--morph-close-ease);
  }

  .pw.is-open .core {
    transform: none;
    transition: transform var(--morph-open-dur) var(--morph-ease);
  }

  .pw.is-open .node {
    transform: none;
    transition: transform var(--morph-open-dur) var(--morph-ease) var(--delay, 0ms);
  }

  /* ─── El ink ────────────────────────────────────────────────────────── */
  .ink {
    position: absolute;
    inset: 0;
    color: var(--text);
  }

  .mark {
    display: block;
    color: var(--text);
    line-height: 0;
  }

  .core-btn {
    position: absolute;
    top: 50%;
    left: 50%;
    display: grid;
    margin: 0;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
    cursor: pointer;
    place-items: center;
    transform: translate(-50%, -50%);
    transition:
      width var(--morph-open-dur) var(--morph-ease),
      height var(--morph-open-dur) var(--morph-ease),
      opacity var(--morph-fade-dur) var(--morph-ease);
  }

  .core-btn:hover {
    opacity: 0.72;
  }

  .core-btn:active {
    transform: translate(-50%, -50%) scale(0.96);
  }

  .core-btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 4px;
  }

  /* ─── Nodos ─────────────────────────────────────────────────────────── */
  .nodes {
    position: absolute;
    inset: 0;
    opacity: 0;
    transform: scale(0.35);
    filter: blur(var(--morph-blur));
    pointer-events: none;
    transform-origin: 50% 50%;
    transition:
      opacity var(--morph-fade-dur) var(--morph-close-ease),
      transform var(--morph-close-dur) var(--morph-close-ease),
      filter var(--morph-fade-dur) var(--morph-close-ease);
  }

  .pw.is-open .nodes {
    opacity: 1;
    transform: scale(1);
    filter: blur(0);
    pointer-events: auto;
    transition:
      opacity var(--morph-fade-dur) var(--morph-ease),
      transform var(--morph-open-dur) var(--morph-ease),
      filter var(--morph-fade-dur) var(--morph-ease);
  }

  .hit {
    position: absolute;
    inset: 0;
    margin: 0;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }

  .hit:focus-visible {
    outline: none;
  }

  .node-body {
    position: absolute;
    display: grid;
    place-items: center;
    pointer-events: none;
    transform: translate(-50%, -50%);
    transition:
      transform var(--duration-quick) var(--ease-smooth-out),
      color var(--duration-quick) var(--ease-smooth-out);
  }

  .node-icon {
    display: grid;
    width: 2.1rem;
    height: 2.1rem;
    place-items: center;
    border-radius: 50%;
  }

  .hit.is-hot {
    color: var(--text);
  }

  .hit.is-hot .node-body {
    transform: translate(-50%, -50%) scale(1.05);
  }

  .hit:active .node-body {
    transform: translate(-50%, -50%) scale(0.95);
  }

  .hit:focus-visible .node-icon {
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }

  @media (prefers-reduced-motion: reduce) {
    .blob,
    .core,
    .node,
    .nodes,
    .core-btn,
    .node-body {
      transition: none !important;
    }
  }
</style>
