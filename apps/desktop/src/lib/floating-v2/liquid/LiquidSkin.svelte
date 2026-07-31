<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { LiquidEngine, type LiquidBox, type LiquidBridge, type LiquidParameters } from "./engine";

  let {
    boxes,
    bridges = [],
    parameters,
    width,
    height,
    duration = 300,
    onRender,
  }: {
    boxes: LiquidBox[];
    bridges?: LiquidBridge[];
    parameters: LiquidParameters;
    width: number;
    height: number;
    duration?: number;
    onRender?: (milliseconds: number) => void;
  } = $props();

  const engine = new LiquidEngine();
  const gradientId = `pearl-${Math.random().toString(36).slice(2)}`;
  let currentBoxes: LiquidBox[] = [];
  let currentBridges: LiquidBridge[] = [];
  let currentParameters: LiquidParameters = { k: 0, cell: 5, smooth: 2 };
  let animationFrame = 0;
  let initialized = false;

  engine.setBoxes(currentBoxes);
  engine.setBridges(currentBridges);
  engine.setParameters(currentParameters);
  let path = $state(engine.compute().d);

  const lerp = (from: number, to: number, progress: number) => from + (to - from) * progress;
  const ease = (progress: number) =>
    progress < 0.5
      ? 4 * progress * progress * progress
      : 1 - Math.pow(-2 * progress + 2, 3) / 2;

  function collapsedBox(box: LiquidBox, anchor: LiquidBox): LiquidBox {
    return {
      ...box,
      cx: anchor.cx,
      cy: anchor.cy,
      hw: 1.5,
      hh: 1.5,
      r: 1.5,
    };
  }

  function alignedBoxes(from: LiquidBox[], to: LiquidBox[]) {
    const anchor = to.find((box) => box.id === "pill") ?? from.find((box) => box.id === "pill") ?? to[0] ?? from[0];
    if (!anchor) return { from: [], to: [] };
    const ids = [...new Set([...from.map((box) => box.id), ...to.map((box) => box.id)])];
    return {
      from: ids.map((id) => {
        const source = from.find((box) => box.id === id);
        const target = to.find((box) => box.id === id);
        return source ? { ...source } : collapsedBox(target ?? anchor, anchor);
      }),
      to: ids.map((id) => {
        const target = to.find((box) => box.id === id);
        const source = from.find((box) => box.id === id);
        return target ? { ...target } : collapsedBox(source ?? anchor, anchor);
      }),
    };
  }

  function collapsedBridge(bridge: LiquidBridge, anchor: LiquidBox): LiquidBridge {
    return { ...bridge, ax: anchor.cx, ay: anchor.cy, bx: anchor.cx, by: anchor.cy, r: 1 };
  }

  function alignedBridges(from: LiquidBridge[], to: LiquidBridge[], anchor: LiquidBox) {
    const ids = [...new Set([...from.map((bridge) => bridge.id), ...to.map((bridge) => bridge.id)])];
    return {
      from: ids.map((id) => {
        const source = from.find((bridge) => bridge.id === id);
        const target = to.find((bridge) => bridge.id === id);
        return source ? { ...source } : collapsedBridge(target!, anchor);
      }),
      to: ids.map((id) => {
        const target = to.find((bridge) => bridge.id === id);
        const source = from.find((bridge) => bridge.id === id);
        return target ? { ...target } : collapsedBridge(source!, anchor);
      }),
    };
  }

  function render(boxValues: LiquidBox[], bridgeValues: LiquidBridge[], parameterValues: LiquidParameters) {
    const started = performance.now();
    engine.setBoxes(boxValues);
    engine.setBridges(bridgeValues);
    engine.setParameters(parameterValues);
    path = engine.compute().d;
    onRender?.(performance.now() - started);
  }

  function morph(targetBoxes: LiquidBox[], targetBridges: LiquidBridge[], targetParameters: LiquidParameters) {
    cancelAnimationFrame(animationFrame);
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (!initialized || reduced || duration <= 0) {
      initialized = true;
      currentBoxes = targetBoxes.map((box) => ({ ...box }));
      currentBridges = targetBridges.map((bridge) => ({ ...bridge }));
      currentParameters = { ...targetParameters };
      render(currentBoxes, currentBridges, currentParameters);
      return;
    }

    const boxesById = alignedBoxes(currentBoxes, targetBoxes);
    const anchor = targetBoxes.find((box) => box.id === "pill") ?? boxesById.to[0];
    const bridgesById = alignedBridges(currentBridges, targetBridges, anchor);
    const fromParameters = { ...currentParameters };
    let started = 0;

    const frame = (time: number) => {
      if (!started) started = time;
      const progress = ease(Math.min(1, (time - started) / duration));
      currentBoxes = boxesById.from.map((from, index) => {
        const to = boxesById.to[index];
        return {
          id: from.id,
          cx: lerp(from.cx, to.cx, progress),
          cy: lerp(from.cy, to.cy, progress),
          hw: lerp(from.hw, to.hw, progress),
          hh: lerp(from.hh, to.hh, progress),
          r: lerp(from.r, to.r, progress),
        };
      });
      currentBridges = bridgesById.from.map((from, index) => {
        const to = bridgesById.to[index];
        return {
          id: from.id,
          ax: lerp(from.ax, to.ax, progress),
          ay: lerp(from.ay, to.ay, progress),
          bx: lerp(from.bx, to.bx, progress),
          by: lerp(from.by, to.by, progress),
          r: lerp(from.r, to.r, progress),
        };
      });
      currentParameters = {
        k: lerp(fromParameters.k, targetParameters.k, progress),
        cell: targetParameters.cell,
        smooth: targetParameters.smooth,
      };
      render(currentBoxes, currentBridges, currentParameters);

      if (progress < 1) animationFrame = requestAnimationFrame(frame);
      else {
        currentBoxes = targetBoxes.map((box) => ({ ...box }));
        currentBridges = targetBridges.map((bridge) => ({ ...bridge }));
        currentParameters = { ...targetParameters };
        render(currentBoxes, currentBridges, currentParameters);
      }
    };
    animationFrame = requestAnimationFrame(frame);
  }

  $effect(() => {
    const targetBoxes = boxes.map((box) => ({ ...box }));
    const targetBridges = bridges.map((bridge) => ({ ...bridge }));
    const targetParameters = { ...parameters };
    void width;
    void height;
    untrack(() => morph(targetBoxes, targetBridges, targetParameters));
  });

  onDestroy(() => cancelAnimationFrame(animationFrame));
</script>

<svg class="liquid-skin" viewBox={`0 0 ${Math.max(1, width)} ${Math.max(1, height)}`} preserveAspectRatio="none" aria-hidden="true">
  <defs>
    <linearGradient id={gradientId} x1="0" y1="0" x2="0.8" y2="1">
      <stop offset="0" stop-color="var(--liquid-pearl-top)" />
      <stop offset="0.58" stop-color="var(--liquid-pearl)" />
      <stop offset="1" stop-color="var(--liquid-pearl-deep)" />
    </linearGradient>
  </defs>
  <path class="skin-path" d={path} fill={`url(#${gradientId})`} />
  <path class="skin-highlight" d={path} fill="none" />
</svg>

<style>
  .liquid-skin {
    --liquid-pearl-top: oklch(1 0 0);
    --liquid-pearl: oklch(0.965 0.006 255);
    --liquid-pearl-deep: oklch(0.915 0.009 255);
    --liquid-shadow: oklch(0.2 0.015 255 / 0.14);
    --liquid-shadow-near: oklch(0.2 0.015 255 / 0.08);
    --liquid-highlight: oklch(1 0 0 / 0.72);
    position: absolute;
    z-index: 0;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: visible;
    pointer-events: none;
  }

  .skin-path {
    filter: drop-shadow(0 24px 32px var(--liquid-shadow)) drop-shadow(0 3px 7px var(--liquid-shadow-near));
  }

  .skin-highlight {
    stroke: var(--liquid-highlight);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }

  :global(:root[data-theme="dark"]) .liquid-skin {
    --liquid-pearl-top: oklch(0.3 0.01 255);
    --liquid-pearl: oklch(0.245 0.012 255);
    --liquid-pearl-deep: oklch(0.19 0.012 255);
    --liquid-shadow: oklch(0.03 0.01 255 / 0.42);
    --liquid-shadow-near: oklch(0.03 0.01 255 / 0.3);
    --liquid-highlight: oklch(1 0 0 / 0.16);
  }
</style>
