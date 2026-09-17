<script lang="ts">
  /**
   * Color: la rosa cromática y el cuentagotas.
   *
   * En la app el modo principal es la lupa —sigue al cursor píxel a píxel— y
   * esta rosa es su editor (tecla R). Acá no hay pantalla del sistema que
   * mirar, así que la rosa va al frente y el cuentagotas es el del navegador
   * (`EyeDropper`, de verdad cuando existe). Elegir un color lo copia al
   * portapapeles, como en la app.
   */
  import Float from "../Float.svelte";
  import Icon from "$lib/atic/Icon.svelte";
  import { Copy, Pipette } from "$lib/atic/icons";
  import { COLOR_RECENTS } from "../data";
  import { copyText, demo, loadJSON, saveJSON } from "../state.svelte";

  let { onClose }: { onClose?: () => void } = $props();

  const RECENTS_KEY = "atic-demo-color-recents";

  let hue = $state(6);
  let light = $state(62);
  const saturation = 68;
  let recents = $state<string[]>(loadJSON(RECENTS_KEY, COLOR_RECENTS));

  const hex = $derived(hslToHex(hue, saturation, light));
  const rgb = $derived(hslToRgb(hue, saturation, light));
  const rgbText = $derived(`rgb(${rgb.map((v) => Math.round(v)).join(", ")})`);
  const hsl = $derived(
    `hsl(${Math.round(hue)}, ${saturation}%, ${light}%)`,
  );

  let wheelEl = $state<HTMLDivElement | null>(null);
  let dragging = $state(false);

  /** El punto sobre la rueda: radio según la luz, ángulo según el tono. */
  const knobR = $derived(62 * (0.55 + 0.45 * ((light - 30) / 45)));
  const knobX = $derived((Math.sin((hue * Math.PI) / 180) * knobR).toFixed(1));
  const knobY = $derived((-Math.cos((hue * Math.PI) / 180) * knobR).toFixed(1));

  function hslToHex(h: number, s: number, l: number): string {
    const [r, g, b] = hslToRgb(h, s, l);
    const channel = (v: number) =>
      Math.round(v)
        .toString(16)
        .padStart(2, "0");
    return `#${channel(r)}${channel(g)}${channel(b)}`.toUpperCase();
  }

  function hslToRgb(h: number, s: number, l: number): [number, number, number] {
    const a = (s / 100) * Math.min(l / 100, 1 - l / 100);
    const channel = (n: number) => {
      const k = (n + h / 30) % 12;
      return 255 * (l / 100 - a * Math.max(-1, Math.min(k - 3, Math.min(9 - k, 1))));
    };
    return [channel(0), channel(8), channel(4)];
  }

  /** `#3A82F6` → tono y luz aproximados para mover la rosa. */
  function hexToHsl(value: string): { h: number; l: number } | null {
    const match = /^#?([0-9a-f]{6})$/i.exec(value.trim());
    if (!match) return null;
    const n = parseInt(match[1], 16);
    const r = (n >> 16) & 255;
    const g = (n >> 8) & 255;
    const b = n & 255;
    const max = Math.max(r, g, b) / 255;
    const min = Math.min(r, g, b) / 255;
    const l = ((max + min) / 2) * 100;
    if (max === min) return { h: 0, l: Math.round(l) };
    const d = max - min;
    const h =
      (max === r / 255
        ? ((g - b) / 255 / d + (g < b ? 6 : 0))
        : max === g / 255
          ? (b - r) / 255 / d + 2
          : (r - g) / 255 / d + 4) * 60;
    return { h: Math.round(h), l: Math.round(l) };
  }

  function fromPointer(event: PointerEvent) {
    const el = wheelEl;
    if (!el) return;
    const box = el.getBoundingClientRect();
    const dx = event.clientX - (box.left + box.width / 2);
    const dy = event.clientY - (box.top + box.height / 2);
    const deg = (Math.atan2(dy, dx) * 180) / Math.PI + 90;
    hue = ((deg % 360) + 360) % 360;
    const dist = Math.min(1, Math.hypot(dx, dy) / (box.width / 2));
    light = Math.round(30 + dist * 45);
  }

  function onDown(event: PointerEvent) {
    dragging = true;
    (event.target as HTMLElement).setPointerCapture(event.pointerId);
    fromPointer(event);
  }

  function onMove(event: PointerEvent) {
    if (dragging) fromPointer(event);
  }

  function onUp() {
    dragging = false;
  }

  function remember(value: string) {
    recents = [value, ...recents.filter((item) => item !== value)].slice(0, 8);
    saveJSON(RECENTS_KEY, recents);
  }

  async function copy() {
    remember(hex);
    await copyText(hex, `${hex} copiado`);
  }

  async function copyFormat(value: string) {
    await copyText(value, `${value} copiado`);
  }

  function applyHex(value: string) {
    const parsed = hexToHsl(value);
    if (!parsed) {
      demo.toast("Escribe seis dígitos hexadecimales, por ejemplo #3A82F6", "info");
      return;
    }
    hue = parsed.h;
    light = Math.min(75, Math.max(30, parsed.l));
    remember(hex);
  }

  const eyeDropperSupported =
    typeof window !== "undefined" && "EyeDropper" in window;

  async function dropper() {
    if (!eyeDropperSupported) {
      demo.toast("Este navegador no tiene cuentagotas; usa la rosa", "info");
      return;
    }
    try {
      // @ts-expect-error EyeDropper no está en los tipos del DOM todavía.
      const dropper = new window.EyeDropper();
      const result: { sRGBHex: string } = await dropper.open();
      const picked = result.sRGBHex.toUpperCase();
      remember(picked);
      await copyText(picked, `${picked} elegido y copiado`);
    } catch {
      /* Cancelado: no hay nada que decir. */
    }
  }

  function useRecent(value: string) {
    void copyText(value, `${value} copiado`);
  }

  let hexDraft = $state("");
  $effect(() => {
    // El campo muestra el color actual salvo que se esté editando.
    void hex;
    if (document.activeElement?.getAttribute("data-hex") !== "1") hexDraft = hex;
  });
</script>

<Float title="Color" icon="color" onClose={onClose}>
  <div class="color">
    <div class="wheel-wrap">
      <div
        class="wheel"
        bind:this={wheelEl}
        role="slider"
        aria-label="Tono"
        aria-valuemin="0"
        aria-valuemax="360"
        aria-valuenow={Math.round(hue)}
        tabindex="0"
        onpointerdown={onDown}
        onpointermove={onMove}
        onpointerup={onUp}
        onpointercancel={onUp}
        onkeydown={(event) => {
          if (event.key === "ArrowLeft") hue = (hue + 359) % 360;
          if (event.key === "ArrowRight") hue = (hue + 1) % 360;
        }}
      >
        <span
          class="knob"
          style="left: calc(50% + {knobX}px); top: calc(50% + {knobY}px); background: {hex}"
        ></span>
      </div>
      <div class="readout">
        <span class="swatch" style="background: {hex}"></span>
        <div class="values">
          <span class="value-row">
            <input
              class="hex-input"
              data-hex="1"
              bind:value={hexDraft}
              spellcheck="false"
              autocomplete="off"
              aria-label="HEX"
              onkeydown={(event) => {
                if (event.key === "Enter") applyHex(hexDraft);
              }}
              onblur={() => {
                if (hexDraft.trim() && hexDraft.trim().toUpperCase() !== hex) {
                  applyHex(hexDraft);
                } else {
                  hexDraft = hex;
                }
              }}
            />
            <button type="button" class="mini-copy" aria-label="Copiar {hex}" title="Copiar {hex}" onclick={copy}>
              <Icon icon={Copy} size={12} />
            </button>
          </span>
          <button type="button" class="format" onclick={() => copyFormat(rgbText)} title="Copiar">
            {rgbText}
          </button>
          <button type="button" class="format" onclick={() => copyFormat(hsl)} title="Copiar">
            {hsl}
          </button>
        </div>
      </div>
    </div>

    <label class="light">
      <span>Luz</span>
      <input
        type="range"
        min="30"
        max="75"
        bind:value={light}
        style="--c: {hex}"
        aria-label="Luz"
      />
    </label>

    <div class="recents" role="group" aria-label="Recientes">
      {#each recents as value (value)}
        <button
          type="button"
          class="recent"
          style="background: {value}"
          title="{value} — copiar"
          aria-label="Copiar {value}"
          onclick={() => useRecent(value)}
        ></button>
      {/each}
    </div>
    <p class="loupe-note">En la app el modo principal es la lupa (Clic o Enter: copiar · R: editar).</p>
  </div>

  {#snippet footer()}
    <div class="foot">
      <span>{eyeDropperSupported ? "Cuentagotas del navegador disponible" : "Sin cuentagotas en este navegador"}</span>
      <button type="button" class="btn is-primary" onclick={dropper}>
        <Icon icon={Pipette} size={12} /> Elegir de la pantalla
      </button>
    </div>
  {/snippet}
</Float>

<style>
  .color {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 16px;
    align-items: center;
  }

  .wheel-wrap {
    display: flex;
    flex-direction: column;
    gap: 12px;
    align-items: center;
  }

  .wheel {
    position: relative;
    width: 172px;
    height: 172px;
    border-radius: 50%;
    background: conic-gradient(
      from 0deg,
      #ff4d4d,
      #ffb84d,
      #f5f55e,
      #6fdd8b,
      #5bd6d6,
      #6f8bff,
      #b46fff,
      #ff6fdc,
      #ff4d4d
    );
    cursor: crosshair;
    touch-action: none;
  }

  .wheel::after {
    content: "";
    position: absolute;
    inset: 18%;
    border-radius: 50%;
    background: var(--surface);
  }

  .wheel:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 4px;
  }

  .knob {
    position: absolute;
    width: 16px;
    height: 16px;
    border: 2px solid var(--surface);
    border-radius: 50%;
    box-shadow: var(--shadow-pop);
    transform: translate(-50%, -50%);
    pointer-events: none;
  }

  .readout {
    display: flex;
    gap: 10px;
    align-items: center;
  }

  .swatch {
    width: 34px;
    height: 34px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
  }

  .values {
    display: flex;
    flex-direction: column;
    gap: 1px;
    align-items: flex-start;
  }

  .value-row {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .hex-input {
    width: 7.5ch;
    border: 0;
    border-bottom: 1px dashed transparent;
    background: transparent;
    color: var(--text);
    font-family: var(--font-mono);
    font-size: var(--text-md);
    outline: none;
    text-transform: uppercase;
  }

  .hex-input:hover,
  .hex-input:focus {
    border-bottom-color: var(--line-strong);
  }

  .mini-copy {
    display: grid;
    place-items: center;
    border: 0;
    padding: 2px;
    background: transparent;
    color: var(--faint);
    cursor: pointer;
  }

  .mini-copy:hover {
    color: var(--text);
  }

  .format {
    border: 0;
    padding: 0;
    background: transparent;
    color: var(--faint);
    font: inherit;
    font-size: var(--text-micro);
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }

  .format:hover {
    color: var(--text);
  }

  .light {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 10px;
    color: var(--faint);
    font-size: var(--text-micro);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
  }

  .light input {
    flex: 1;
    accent-color: var(--c);
  }

  .recents {
    display: flex;
    gap: 8px;
  }

  .recent {
    width: 26px;
    height: 26px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .recent:hover {
    border-color: var(--line-strong);
    transform: translateY(-1px);
  }

  .loupe-note {
    margin: 0;
    color: var(--faint);
    font-size: var(--text-micro);
    text-align: center;
  }

  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    padding: 4px 9px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--text-micro);
    cursor: pointer;
  }

  .btn.is-primary {
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    background: var(--accent);
    color: var(--on-accent);
  }
</style>
