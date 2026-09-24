<script lang="ts">
  /**
   * Una consola en la pizarra: marco que se mueve desde su barra y se
   * redimensiona desde los bordes, con la terminal adentro.
   *
   * Vive dentro del plano de la pizarra, que ya lleva el corrimiento y el
   * zoom: se posiciona en coordenadas de pizarra y el arrastre se divide por
   * el zoom, o al alejar la consola se escaparía del puntero.
   *
   * Maximizada ocupa lo que la pizarra le da y no se arrastra ni se
   * redimensiona: se vuelve con el mismo botón o con doble clic en la barra.
   *
   * Mientras se arrastra solo avisa el rectángulo en vivo (`commit` falso);
   * al soltar lo avisa una vez más para guardarlo. Guardar en cada cuadro
   * escribiría el storage decenas de veces por segundo.
   */
  import type { Snippet } from "svelte";
  import { emerge } from "$lib/motion";
  import { t } from "$domain/i18n.svelte";
  import Icon from "$ui/Icon.svelte";
  import { Maximize2, Minimize2, X } from "$lib/icons";
  import AgentLogo from "./AgentLogo.svelte";
  import ConsoleStateDot from "./ConsoleStateDot.svelte";
  import type { ConsoleState } from "./consoleStatus";
  import { dragRect, type Handle, type Rect } from "./agentBoard";

  let {
    label,
    cli,
    rect,
    zoom,
    z,
    active,
    status,
    attention,
    maximized,
    dropping,
    onFocus,
    onRect,
    onClose,
    onMaximize,
    onActivate,
    actions,
    children,
  }: {
    label: string;
    cli: string | null;
    rect: Rect;
    zoom: number;
    z: number;
    active: boolean;
    status: ConsoleState;
    attention: boolean;
    maximized: boolean;
    /** Hay archivos arrastrándose encima: se van a pegar acá. */
    dropping: boolean;
    onFocus: () => void;
    /** `moved`: se arrastró entera (no se redimensionó). */
    onRect: (rect: Rect, commit: boolean, moved: boolean) => void;
    onClose: () => void;
    /** Botón o doble clic en la barra: maximizar o volver. */
    onMaximize: () => void;
    /** Un clic (sin arrastre) en cualquier parte: la pizarra la encuadra si hace falta. */
    onActivate: () => void;
    /** Botones propios de esta tarjeta, antes de maximizar y cerrar. */
    actions?: Snippet;
    children: Snippet;
  } = $props();

  const GRIPS: Handle[] = ["e", "s", "w", "se", "sw"];

  let drag: { handle: Handle; x: number; y: number; start: Rect } | null = null;
  /** Dónde bajó el puntero, para distinguir un clic de un arrastre. */
  let press: { x: number; y: number } | null = null;

  function onPress(event: PointerEvent) {
    press = { x: event.clientX, y: event.clientY };
    onFocus();
  }

  function onRelease(event: PointerEvent) {
    if (!press) return;
    const moved = Math.hypot(event.clientX - press.x, event.clientY - press.y);
    press = null;
    if (moved < 5) onActivate();
  }
  let dragging = $state(false);

  function begin(event: PointerEvent, handle: Handle) {
    if (event.button !== 0 || maximized) return;
    // El botón de cerrar vive en la barra: tocarlo no es arrastrar.
    if ((event.target as HTMLElement).closest("button")) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    drag = { handle, x: event.clientX, y: event.clientY, start: rect };
    dragging = true;
    onFocus();
  }

  function delta(event: PointerEvent): Rect {
    if (!drag) return rect;
    return dragRect(
      drag.start,
      drag.handle,
      (event.clientX - drag.x) / zoom,
      (event.clientY - drag.y) / zoom,
    );
  }

  function move(event: PointerEvent) {
    if (!drag) return;
    onRect(delta(event), false, drag.handle === "move");
  }

  function end(event: PointerEvent) {
    if (!drag) return;
    const next = delta(event);
    const moved = drag.handle === "move";
    drag = null;
    dragging = false;
    onRect(next, true, moved);
  }
</script>

<section
  class="card"
  class:is-active={active}
  class:is-dragging={dragging}
  class:is-maximized={maximized}
  class:is-dropping={dropping}
  style:left={`${rect.x}px`}
  style:top={`${rect.y}px`}
  style:width={`${rect.w}px`}
  style:height={`${rect.h}px`}
  style:z-index={z}
  aria-label={label}
  transition:emerge
  onpointerdowncapture={onPress}
  onpointerupcapture={onRelease}
>
  <header
    class="bar"
    role="presentation"
    onpointerdown={(e) => begin(e, "move")}
    onpointermove={move}
    onpointerup={end}
    onpointercancel={end}
    ondblclick={(e) => {
      if (!(e.target as HTMLElement).closest("button")) onMaximize();
    }}
  >
    <AgentLogo agent={cli} size={14} />
    <span class="label" title={label}>{label}</span>
    <ConsoleStateDot {status} {attention} label />
    {@render actions?.()}
    <button
      type="button"
      class="close"
      aria-label={maximized
        ? t("page.agents.board.restore")
        : t("page.agents.board.maximize")}
      title={maximized
        ? t("page.agents.board.restore")
        : t("page.agents.board.maximize")}
      onclick={onMaximize}
    >
      <Icon icon={maximized ? Minimize2 : Maximize2} size={12} />
    </button>
    <button
      type="button"
      class="close"
      aria-label={t("page.agents.window.close", { name: label })}
      title={t("page.agents.window.close", { name: label })}
      onclick={onClose}
    >
      <Icon icon={X} size={13} />
    </button>
  </header>

  <div class="body">
    {@render children()}
  </div>

  {#each maximized ? [] : GRIPS as handle (handle)}
    <span
      class="grip is-{handle}"
      aria-hidden="true"
      onpointerdown={(e) => begin(e, handle)}
      onpointermove={move}
      onpointerup={end}
      onpointercancel={end}
    ></span>
  {/each}
</section>

<style>
  .card {
    position: absolute;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-radius: 12px;
    background: var(--rb-bg0);
    box-shadow:
      0 0 0 1px color-mix(in sRGB, var(--rb-text) 12%, transparent),
      0 18px 48px -20px rgb(0 0 0 / 55%);
    transition: box-shadow var(--duration-medium) ease;
  }

  .card.is-active {
    box-shadow:
      0 0 0 1.5px color-mix(in sRGB, var(--accent) 70%, transparent),
      0 24px 60px -18px rgb(0 0 0 / 65%);
  }

  /* Mientras se arrastra, la terminal no roba el puntero ni la selección. */
  .card.is-dragging,
  .card.is-dragging .body {
    user-select: none;
  }

  .card.is-dragging .body {
    pointer-events: none;
  }

  .bar {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 6px 0 12px;
    background: var(--rb-surface);
    color: var(--rb-muted);
    font-size: 12px;
    cursor: grab;
    touch-action: none;
    user-select: none;
  }

  .card.is-dropping {
    box-shadow:
      0 0 0 2px var(--accent),
      0 0 0 6px color-mix(in sRGB, var(--accent) 22%, transparent),
      0 24px 60px -18px rgb(0 0 0 / 65%);
  }

  .card.is-maximized .bar {
    cursor: default;
  }

  .card.is-dragging .bar {
    cursor: grabbing;
  }

  .card.is-active .bar {
    color: var(--rb-text);
  }

  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-weight: 560;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .close {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 24px;
    height: 24px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--rb-faint);
    cursor: pointer;
    transition:
      background-color var(--duration-fast) ease,
      color var(--duration-fast) ease;
  }

  .close:hover {
    background: color-mix(in sRGB, var(--rb-text) 9%, transparent);
    color: var(--rb-text);
  }

  .body {
    position: relative;
    flex: 1;
    min-height: 0;
  }

  /* Bordes para redimensionar: finos a la vista, generosos al puntero. */
  .grip {
    position: absolute;
    z-index: 2;
    touch-action: none;
  }

  .grip.is-e {
    top: 32px;
    right: 0;
    bottom: 12px;
    width: 8px;
    cursor: ew-resize;
  }

  .grip.is-w {
    top: 32px;
    bottom: 12px;
    left: 0;
    width: 8px;
    cursor: ew-resize;
  }

  .grip.is-s {
    right: 12px;
    bottom: 0;
    left: 12px;
    height: 8px;
    cursor: ns-resize;
  }

  .grip.is-se {
    right: 0;
    bottom: 0;
    width: 14px;
    height: 14px;
    cursor: nwse-resize;
  }

  .grip.is-sw {
    bottom: 0;
    left: 0;
    width: 14px;
    height: 14px;
    cursor: nesw-resize;
  }
</style>
