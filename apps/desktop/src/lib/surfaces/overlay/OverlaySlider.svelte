<script lang="ts">
  /**
   * Slider del overlay. El `<input type="range">` nativo en WebKit arranca un
   * tracking de AppKit: si el overlay parpadea a click-through, el mouseup
   * nunca llega y el pulgar sigue al cursor.
   *
   * # Por qué no alcanza con `pointermove`
   *
   * Tampoco sirve el reemplazo obvio. En esta ventana el `setPointerCapture`
   * se pierde en cuanto el overlay rearma sus hit-rects (que es justo lo que
   * pasa al empezar a arrastrar), y con él llega `lostpointercapture`: el
   * gesto se cancelaba en el primer píxel y el slider quedaba funcionando
   * solo a clics sueltos. Y si otra ventana se queda con el mouse, el webview
   * deja de recibir `pointermove` del todo.
   *
   * Por eso el arrastre sigue el **cursor de Rust**, igual que la pill y los
   * floats (`bubbleDrag`): el DOM manda mientras esté fresco —es más rápido y
   * no cuesta IPC— y el cursor del sistema es la red cuando se queda mudo. El
   * soltar lo recogen oyentes de `window` en captura, que sí llegan siempre.
   */
  import { overlayCursor } from "$ipc/overlay";

  import { surfaces } from "./surfaces.svelte";

  let {
    value,
    min = 0,
    max = 1,
    step = 0.01,
    label,
    onValue,
  }: {
    value: number;
    min?: number;
    max?: number;
    step?: number;
    label?: string;
    onValue: (value: number) => void;
  } = $props();

  let track = $state<HTMLElement | null>(null);
  let held = false;
  /** Último cursor visto por el DOM: sin IPC y con la mano al día. */
  let domX = 0;
  let domAt = 0;
  let raf = 0;

  /** Sin `pointermove` del DOM en este rato, el cursor de Rust toma el relevo. */
  const DOM_STALE_MS = 32;

  const span = $derived(max - min);
  const ratio = $derived(
    span <= 0 ? 0 : Math.min(1, Math.max(0, (value - min) / span)),
  );

  function snap(next: number): number {
    if (span <= 0) return min;
    const stepped = min + Math.round((next - min) / step) * step;
    return Math.min(max, Math.max(min, stepped));
  }

  function valueAt(clientX: number): number {
    const el = track;
    if (!el || span <= 0) return min;
    const box = el.getBoundingClientRect();
    if (box.width <= 0) return min;
    return snap(min + ((clientX - box.left) / box.width) * span);
  }

  function apply(clientX: number) {
    onValue(valueAt(clientX));
  }

  function release() {
    if (!held) return;
    held = false;
    surfaces.dragging = false;
    cancelAnimationFrame(raf);
    raf = 0;
    window.removeEventListener("pointerup", release, true);
    window.removeEventListener("pointercancel", release, true);
    window.removeEventListener("pointermove", onDomMove, true);
  }

  function onDomMove(event: PointerEvent) {
    domX = event.clientX;
    domAt = performance.now();
  }

  /** Un cuadro del arrastre: el DOM si está fresco, el cursor de Rust si no. */
  async function follow() {
    if (!held) return;
    const fresco = domAt !== 0 && performance.now() - domAt < DOM_STALE_MS;
    const x = fresco ? domX : ((await overlayCursor().catch(() => null))?.x ?? null);
    if (!held) return;
    if (x != null) apply(x);
    raf = requestAnimationFrame(() => void follow());
  }

  function down(event: PointerEvent) {
    if (event.button !== 0) return;
    event.preventDefault();
    // La pill y los floats no pueden interpretar esto como un arrastre suyo.
    event.stopPropagation();
    held = true;
    surfaces.dragging = true;
    domX = event.clientX;
    domAt = performance.now();
    apply(event.clientX);
    window.addEventListener("pointerup", release, true);
    window.addEventListener("pointercancel", release, true);
    window.addEventListener("pointermove", onDomMove, true);
    raf = requestAnimationFrame(() => void follow());
  }

  function key(event: KeyboardEvent) {
    const jump = event.shiftKey ? step * 10 : step;
    if (event.key === "Home") {
      event.preventDefault();
      onValue(min);
      return;
    }
    if (event.key === "End") {
      event.preventDefault();
      onValue(max);
      return;
    }
    const dir =
      event.key === "ArrowLeft" || event.key === "ArrowDown"
        ? -1
        : event.key === "ArrowRight" || event.key === "ArrowUp"
          ? 1
          : 0;
    if (!dir) return;
    event.preventDefault();
    onValue(snap(value + dir * jump));
  }
</script>

<div
  bind:this={track}
  class="ov-slider"
  role="slider"
  tabindex="0"
  aria-label={label}
  aria-valuemin={min}
  aria-valuemax={max}
  aria-valuenow={value}
  onpointerdown={down}
  onkeydown={key}
>
  <i class="ov-slider-fill" style:width={`${ratio * 100}%`}></i>
  <i class="ov-slider-thumb" style:left={`${ratio * 100}%`}></i>
</div>

<style>
  .ov-slider {
    position: relative;
    height: 1.35rem;
    cursor: pointer;
    touch-action: none;
    user-select: none;
  }

  .ov-slider::before {
    content: "";
    position: absolute;
    inset: calc(50% - 2px) 0 auto;
    height: 4px;
    border-radius: 999px;
    background: color-mix(in sRGB, var(--text) 18%, transparent);
  }

  .ov-slider-fill {
    position: absolute;
    inset: calc(50% - 2px) auto auto 0;
    height: 4px;
    border-radius: 999px;
    background: var(--text);
    pointer-events: none;
  }

  .ov-slider-thumb {
    position: absolute;
    top: 50%;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--elevated);
    box-shadow: 0 0 0 1px color-mix(in sRGB, var(--text) 22%, transparent);
    transform: translate(-50%, -50%);
    pointer-events: none;
  }
</style>
