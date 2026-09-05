<script lang="ts">
  /**
   * Lupa del cuentagotas en vivo.
   *
   * Rust mueve esta ventana junto al cursor y emite el parche de píxeles
   * físicos. Acá solo se pinta, se abre la rosa y se copia.
   */
  import {
    formatColor,
    hsvToRgb,
    inkOn,
    loadRecentColors,
    parseHex,
    pushRecentColor,
    rgbToHex,
    rgbToHsl,
    rgbToHsv,
    ROSE_HUES,
    roseSwatch,
    type ColorFormat,
    type Rgb,
    type Hsv,
  } from "$features/color/colorMath";
  import {
    completeColorPick,
    setColorPickerRose,
    stopColorPicker,
    colorPickerState,
  } from "$ipc/captures";
  import { onMount } from "svelte";
  import { slide } from "svelte/transition";
  import { on } from "$ipc/events";
  import { t } from "$domain/i18n.svelte";
  import { boxShape } from "$liquid/geometry";
  import { RectTracker } from "$liquid/measure.svelte";
  import Skin from "$liquid/Skin.svelte";
  import type { Shape } from "$liquid/sdf";
  import type { OverlayPatch } from "$core/types";

  const CELL = 10;
  const LOUPE = 13 * CELL;
  /** Radio del cuerpo y del lóbulo. Los dos grandes: esto es una gota. */
  const BODY_RADIUS = 24;
  const LOBE_RADIUS = 22;
  /**
   * Cuánto engorda el lóbulo del cuadro de muestra sobre su rectángulo.
   *
   * Es lo que lo hace **salir** del cuerpo en vez de quedar dibujado dentro:
   * la regla del sistema líquido es que dos formas se funden cuando una nace
   * de la otra, y sin este desborde no hay nada que fundir.
   */
  const LOBE_GROW = 12;
  /**
   * Lo que tarda la rosa en desplegarse y replegarse.
   *
   * Tiene que ser menor que `SHRINK_DELAY` en Rust: la ventana espera a que
   * la piel termine de encogerse antes de achicarse, y si achicara antes el
   * contenido se recortaría contra el borde a mitad del repliegue.
   */
  const ROSE_MS = 220;

  let roseOpen = $state(false);
  let format = $state<ColorFormat>("hex");
  let hsv = $state<Hsv>({ h: 0, s: 0, v: 128 / 255 });
  let session = $state(0);
  let endedSession = 0;
  let sample = $state<OverlayPatch | null>(null);
  const ready = $derived(sample !== null && sample.session === session);
  let copying = $state(false);
  let changingMode = $state(false);
  let error = $state("");
  let hexDraft = $state("");
  let recent = $state<string[]>(loadRecentColors());
  let canvasEl: HTMLCanvasElement | undefined = $state();
  let svEl: HTMLCanvasElement | undefined = $state();
  let ringEl: HTMLDivElement | undefined = $state();
  let dragging: "hue" | "sv" | null = null;

  /**
   * En qué punto de su vida está la lupa.
   *
   * La ventana nativa aparece y desaparece de golpe; esto es lo que hace que
   * la gota nazca y se despida. Rust espera `CLOSE_DELAY` —los mismos 200 ms
   * que dura la animación de salida— antes de esconderla, así que la
   * despedida se ve entera.
   */
  let phase = $state<"in" | "live" | "out">("in");
  /** Se copió: la despedida destella el color en vez de encogerse a secas. */
  let copied = $state(false);
  let stageEl: HTMLDivElement | undefined = $state();
  let bodyEl: HTMLDivElement | undefined = $state();
  const tracker = new RectTracker();

  /**
   * La silueta: el cuerpo, y el cuadro de muestra saliendo de él.
   *
   * Se mide del DOM en vez de calcularse, que es lo que hace que la piel siga
   * la animación cuadro a cuadro: cuando la rosa se despliega, el cuerpo crece
   * con CSS y la gota se estira con él sin que nadie interpole nada.
   */
  const skin = $derived.by(() => {
    const body = tracker.rects.body;
    if (!body) return [] as Shape[];
    const shapes: Shape[] = [boxShape(body, BODY_RADIUS)];
    const lobe = tracker.rects.lobe;
    if (lobe) {
      shapes.push(
        boxShape(
          {
            x: lobe.x - LOBE_GROW,
            y: lobe.y - LOBE_GROW,
            w: lobe.w + LOBE_GROW * 2,
            h: lobe.h + LOBE_GROW * 2,
          },
          LOBE_RADIUS,
        ),
      );
    }
    return shapes;
  });

  /** La gota respira mientras lee píxeles, y para cuando deja de leer. */
  const sampling = $derived(phase === "live" && ready && !roseOpen && !copying);

  const rgb = $derived(hsvToRgb(hsv));
  const hex = $derived(rgbToHex(rgb));
  const hsl = $derived(rgbToHsl(rgb));
  const value = $derived(formatColor(rgb, format));
  const ink = $derived(inkOn(rgb));

  function paintLoupe(next: OverlayPatch) {
    const ctx = canvasEl?.getContext("2d");
    if (!ctx) return;
    const size = next.size;
    const data = next.rgba;
    for (let y = 0; y < size; y += 1) {
      for (let x = 0; x < size; x += 1) {
        const i = (y * size + x) * 4;
        ctx.fillStyle = `rgb(${data[i]}, ${data[i + 1]}, ${data[i + 2]})`;
        ctx.fillRect(x * CELL, y * CELL, CELL, CELL);
      }
    }
    const mid = Math.floor(size / 2);
    ctx.strokeStyle = inkOn({ r: next.r, g: next.g, b: next.b });
    ctx.lineWidth = 2;
    ctx.strokeRect(mid * CELL + 1, mid * CELL + 1, CELL - 2, CELL - 2);
  }

  function paintSv(hue: number) {
    const ctx = svEl?.getContext("2d");
    if (!ctx || !svEl) return;
    const w = svEl.width;
    const h = svEl.height;
    const img = ctx.createImageData(w, h);
    const buf = img.data;
    for (let y = 0; y < h; y += 1) {
      const v = 1 - y / Math.max(1, h - 1);
      for (let x = 0; x < w; x += 1) {
        const s = x / Math.max(1, w - 1);
        const px = hsvToRgb({ h: hue, s, v });
        const i = (y * w + x) * 4;
        buf[i] = px.r;
        buf[i + 1] = px.g;
        buf[i + 2] = px.b;
        buf[i + 3] = 255;
      }
    }
    ctx.putImageData(img, 0, 0);
  }

  function applyPatch(next: OverlayPatch) {
    if (!Number.isSafeInteger(next.session) || next.session <= 0) {
      error = t("page.colorHud.protocolError");
      return;
    }
    if (next.session <= endedSession || next.session < session) return;
    if (next.session !== session) {
      session = next.session;
      roseOpen = false;
      copying = false;
      changingMode = false;
      sample = null;
      dragging = null;
      error = "";
      recent = loadRecentColors();
      // Sesión nueva en la misma ventana: la gota vuelve a nacer.
      copied = false;
      phase = "in";
    }
    if (roseOpen || copying || changingMode) return;
    setRgb(next);
    sample = next;
    paintLoupe(next);
  }

  function setRgb(next: Rgb) {
    setHsv(rgbToHsv(next));
  }

  function setHsv(next: Hsv) {
    hsv = next;
    hexDraft = rgbToHex(hsvToRgb(next));
  }

  async function commit() {
    if (!ready || copying || changingMode) return;
    const token = session;
    const selectedHex = hex;
    copying = true;
    error = "";
    try {
      await completeColorPick(value, token);
      // Record the captured value, never a later pointer sample, after success.
      recent = pushRecentColor(selectedHex, recent);
      copied = true;
    } catch (cause) {
      if (session === token && token > endedSession) {
        error = t("page.colorHud.copyError", { error: String(cause) });
        await setRose(true);
      }
    } finally {
      if (session === token) copying = false;
    }
  }

  async function setRose(open: boolean) {
    if (!session || changingMode) return;
    const token = session;
    const previous = roseOpen;
    roseOpen = open;
    hexDraft = hex;
    changingMode = true;
    try {
      await withTimeout(setColorPickerRose(open, token));
      if (session === token && token > endedSession) {
        roseOpen = open;
        hexDraft = hex;
        dragging = null;
      }
    } catch (cause) {
      if (session === token && token > endedSession) {
        roseOpen = previous;
        error = String(cause);
      }
    } finally {
      if (session === token) changingMode = false;
    }
  }

  async function withTimeout<T>(operation: Promise<T>): Promise<T> {
    let timeout: ReturnType<typeof setTimeout>;
    try {
      return await Promise.race([
        operation,
        new Promise<never>((_, reject) => {
          timeout = setTimeout(
            () => reject(new Error(t("page.colorHud.timeout"))),
            2500,
          );
        }),
      ]);
    } finally {
      clearTimeout(timeout!);
    }
  }

  function toggleRose() {
    if (!copying) void setRose(!roseOpen);
  }

  function setHueFromEvent(event: PointerEvent) {
    const ring = ringEl;
    if (!ring) return;
    const box = ring.getBoundingClientRect();
    const dx = event.clientX - (box.left + box.width / 2);
    const dy = event.clientY - (box.top + box.height / 2);
    let h = (Math.atan2(dy, dx) * 180) / Math.PI + 90;
    if (h < 0) h += 360;
    setHsv({ ...hsv, h });
  }

  function setSvFromEvent(event: PointerEvent) {
    if (!svEl) return;
    const box = svEl.getBoundingClientRect();
    const s = Math.min(1, Math.max(0, (event.clientX - box.left) / box.width));
    const v = Math.min(1, Math.max(0, 1 - (event.clientY - box.top) / box.height));
    setHsv({ ...hsv, s, v });
  }

  function onMove(event: PointerEvent) {
    if (dragging === "hue") setHueFromEvent(event);
    else if (dragging === "sv") setSvFromEvent(event);
  }

  function onUp() {
    dragging = null;
  }

  function beginDrag(event: PointerEvent, kind: "hue" | "sv") {
    if (event.button !== 0 || copying) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    dragging = kind;
    onMove(event);
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      void stopColorPicker(session).catch((cause) => (error = String(cause)));
    }
    if (event.repeat || event.ctrlKey || event.metaKey || event.altKey) return;
    if ((event.target as HTMLElement)?.closest("input, textarea")) return;
    if (event.key.toLowerCase() === "r") {
      event.preventDefault();
      toggleRose();
    } else if (
      event.key === "Enter" &&
      !(event.target as HTMLElement)?.closest("button")
    ) {
      event.preventDefault();
      void commit();
    }
  }

  function applyHex() {
    const parsed = parseHex(hexDraft);
    if (!parsed) {
      error = t("page.colorHud.invalidHex");
      return;
    }
    setRgb(parsed);
    error = "";
  }

  onMount(() => {
    let disposed = false;
    const offs: (() => void)[] = [];
    const subscriptions: Promise<void>[] = [];
    function subscribe(pending: Promise<() => void>) {
      subscriptions.push(
        pending
          .then((off) => {
            if (disposed) off();
            else offs.push(off);
          })
          .catch((cause) => {
            if (!disposed) error = String(cause);
          }),
      );
    }
    subscribe(on("color-patch", applyPatch));
    subscribe(
      on("color-toggle-rose", (token) => {
        if (token === session && token > endedSession) toggleRose();
      }),
    );
    subscribe(
      on("color-request-commit", ({ session: token, patch }) => {
        if (token <= endedSession || token < session || copying) return;
        if (patch) applyPatch(patch);
        if (token === session) void commit();
      }),
    );
    subscribe(
      on("color-picker-error", ({ session: token, message }) => {
        if (token <= endedSession || token < session) return;
        session = token;
        error = message;
      }),
    );
    subscribe(
      on("color-picker-ended", (token) => {
        endedSession = Math.max(endedSession, token);
        if (session <= token) {
          // `sample` no se borra: la gota se despide mostrando el color que
          // acaba de leer. Vaciarlo dejaba «Reading color…» durante la salida.
          roseOpen = false;
          dragging = null;
          copying = false;
          changingMode = false;
          phase = "out";
        }
      }),
    );
    // Restore state after HMR/reload or a missed first event. Events are hints;
    // the native session is the source of truth.
    void Promise.all(subscriptions)
      .then(() => withTimeout(colorPickerState()))
      .then((state) => {
        if (disposed || !state) return;
        if (state.active && state.session >= session && state.session > endedSession) {
          if (state.patch) applyPatch(state.patch);
          session = state.session;
          roseOpen = state.open;
        }
      })
      .catch((cause) => {
        if (!disposed) error = String(cause);
      });
    return () => {
      disposed = true;
      offs.forEach((off) => off());
    };
  });

  $effect(() => {
    if (roseOpen) paintSv(hsv.h);
  });

  // La entrada dura lo que la animación; después la gota simplemente vive.
  $effect(() => {
    if (phase !== "in") return;
    const timer = setTimeout(() => {
      if (phase === "in") phase = "live";
    }, 300);
    return () => clearTimeout(timer);
  });

  // Cada cambio de forma —abrir la rosa, entrar un error, cambiar el texto—
  // vuelve a poner el medidor a mirar; él solo se apaga cuando todo se aquieta.
  $effect(() => {
    void roseOpen;
    void error;
    void value;
    void phase;
    tracker.wake();
  });

  $effect(() => {
    tracker.origin = stageEl ?? null;
  });

  $effect(() => {
    if (!bodyEl) return;
    return tracker.track("body", bodyEl);
  });

  $effect(() => {
    if (!canvasEl || roseOpen) return;
    return tracker.track("lobe", canvasEl);
  });
</script>

<svelte:window
  onpointermove={onMove}
  onpointerup={onUp}
  onpointercancel={onUp}
  onkeydown={onKey}
/>

<div
  class="stage"
  class:is-rose={roseOpen}
  class:is-in={phase === "in"}
  class:is-out={phase === "out"}
  class:is-copied={copied}
  bind:this={stageEl}
>
  <!-- La silueta primero, o sea debajo: el contenido vive encima de ella. -->
  <Skin shapes={skin} breathe={sampling} />

  <div class="body" bind:this={bodyEl} aria-busy={copying}>
    <div class="preview">
      <canvas
        bind:this={canvasEl}
        width={LOUPE}
        height={LOUPE}
        class="grid"
        class:is-ready={ready}
      ></canvas>
    <div class="preview-content">
      <button
        type="button"
        class="read"
        style:background={hex}
        style:color={ink}
        disabled={!ready || copying || changingMode}
        onclick={() => void commit()}
      >
        <span class="swatch" style:background={hex}></span>
        <span class="hex" data-numeric
          >{ready ? value : t("page.colorHud.loading")}</span
        >
      </button>
      <div class="bar">
        <p class="help">
          {roseOpen ? t("page.colorHud.helpRose") : t("page.colorHud.help")}
        </p>
        <button
          type="button"
          class="rose-btn"
          aria-pressed={roseOpen}
          aria-label={roseOpen
            ? t("page.colorHud.roseClose")
            : t("page.colorHud.roseOpen")}
          onclick={toggleRose}
          disabled={!ready || copying || changingMode}
        >
          {roseOpen ? t("page.colorHud.back") : t("page.colorHud.edit")}
        </button>
      </div>
    </div>
  </div>

  {#if error}<p class="error" role="alert">{error}</p>{/if}

  {#if roseOpen}
    <div
      class="rose"
      role="dialog"
      aria-label={t("page.colorHud.roseAria")}
      inert={copying}
      transition:slide={{ duration: ROSE_MS }}
    >
      <div class="rose-head">
        <button
          type="button"
          class="rose-swatch"
          style:background={hex}
          aria-label={value}
          disabled={!ready || copying}
          onclick={() => void commit()}
        ></button>
        <div class="meta">
          <button
            type="button"
            class="code"
            class:is-on={format === "hex"}
            onclick={() => (format = "hex")}
          >
            {hex}
          </button>
          <button
            type="button"
            class="code"
            class:is-on={format === "rgb"}
            onclick={() => (format = "rgb")}
          >
            rgb({rgb.r}, {rgb.g}, {rgb.b})
          </button>
          <button
            type="button"
            class="code"
            class:is-on={format === "hsl"}
            onclick={() => (format = "hsl")}
          >
            hsl({Math.round(hsl.h)}, {Math.round(hsl.s * 100)}%, {Math.round(
              hsl.l * 100,
            )}%)
          </button>
        </div>
      </div>

      <div class="wheel">
        <div
          bind:this={ringEl}
          class="ring"
          style:--hue="{hsv.h}deg"
          role="slider"
          tabindex="0"
          aria-label={t("page.colorHud.hue")}
          aria-valuemin={0}
          aria-valuemax={359}
          aria-valuenow={Math.round(hsv.h) % 360}
          onkeydown={(event) => {
            const step = event.shiftKey ? 10 : 1;
            if (
              [
                "ArrowLeft",
                "ArrowDown",
                "ArrowRight",
                "ArrowUp",
                "Home",
                "End",
              ].includes(event.key)
            ) {
              event.preventDefault();
              const h =
                event.key === "Home"
                  ? 0
                  : event.key === "End"
                    ? 359
                    : (hsv.h +
                        (["ArrowLeft", "ArrowDown"].includes(event.key)
                          ? -step
                          : step) +
                        360) %
                      360;
              setHsv({ ...hsv, h });
            }
          }}
          onpointerdown={(event) => beginDrag(event, "hue")}
          onlostpointercapture={onUp}
        >
          <div class="ring-fill"></div>
          <div class="hue-knob"></div>
        </div>
        <canvas
          bind:this={svEl}
          class="sv"
          width="112"
          height="112"
          aria-hidden="true"
          onpointerdown={(event) => beginDrag(event, "sv")}
          onlostpointercapture={onUp}
        ></canvas>
        <div
          class="sv-knob"
          style:left="{36 + hsv.s * 104}px"
          style:top="{36 + (1 - hsv.v) * 104}px"
        ></div>
      </div>

      <div class="channels">
        <label
          >{t("page.colorHud.saturation")}
          <input
            type="range"
            aria-label={t("page.colorHud.saturation")}
            min="0"
            max="100"
            value={hsv.s * 100}
            oninput={(event) => setHsv({ ...hsv, s: +event.currentTarget.value / 100 })}
          />
          <output>{Math.round(hsv.s * 100)}%</output>
        </label>
        <label
          >{t("page.colorHud.brightness")}
          <input
            type="range"
            aria-label={t("page.colorHud.brightness")}
            min="0"
            max="100"
            value={hsv.v * 100}
            oninput={(event) => setHsv({ ...hsv, v: +event.currentTarget.value / 100 })}
          />
          <output>{Math.round(hsv.v * 100)}%</output>
        </label>
      </div>
      <form
        class="hex-entry"
        onsubmit={(event) => {
          event.preventDefault();
          applyHex();
        }}
      >
        <label for="color-hex">HEX</label>
        <input
          id="color-hex"
          bind:value={hexDraft}
          maxlength="7"
          spellcheck="false"
          placeholder="#RRGGBB"
        />
        <button type="submit">{t("page.colorHud.apply")}</button>
      </form>

      <div class="ticks">
        {#each ROSE_HUES as hue (hue)}
          {@const swatch = roseSwatch(hue)}
          <button
            type="button"
            class="tick"
            style:background={rgbToHex(swatch)}
            aria-label="{Math.round(hue)}°"
            onclick={() => {
              setHsv({ h: hue, s: 1, v: 1 });
            }}
          ></button>
        {/each}
      </div>

      {#if recent.length > 0}
        <div class="recent">
          <span class="recent-label">{t("page.colorHud.recent")}</span>
          <div class="ticks">
            {#each recent as item (item)}
              <button
                type="button"
                class="tick"
                style:background={item}
                aria-label={item}
                onclick={() => {
                  const parsed = parseHex(item);
                  if (parsed) {
                    setRgb(parsed);
                  }
                }}
              ></button>
            {/each}
          </div>
        </div>
      {/if}

      <button
        type="button"
        class="copy"
        disabled={!ready || copying || changingMode}
        onclick={() => void commit()}
      >
        {copying ? t("page.colorHud.copying") : t("page.colorHud.copy", { value })}
      </button>
    </div>
  {/if}
    <button
      type="button"
      class="cancel"
      onclick={() =>
        void stopColorPicker(session).catch((cause) => (error = String(cause)))}
    >
      {t("page.colorHud.cancel")}
    </button>
  </div>
</div>

<style>
  /* La ventana es transparente: lo único que se ve es la gota. */
  :global(html),
  :global(body) {
    margin: 0;
    background: transparent;
    overflow: hidden;
  }

  .stage {
    position: relative;
    box-sizing: border-box;
    /* El aire donde cae la sombra de la gota. Lo reserva `PAD` en Rust. */
    padding: 24px;
    min-height: 100vh;
    color: var(--text);
    font:
      12px/1.4 "Segoe UI",
      sans-serif;
    user-select: none;
  }

  /*
   * El contenido, encima de la piel.
   *
   * `position: relative` no es decorativo: la piel es un hijo absoluto, y sin
   * posicionar este bloque la silueta se pintaría sobre el texto.
   *
   * Sin fondo propio a propósito. El cuerpo ES la silueta; pintarlo otra vez
   * dejaría un rectángulo dentro de la gota.
   */
  .body {
    position: relative;
    padding: 10px 14px 10px 30px;
  }

  /*
   * Nace y se despide. Rust espera la salida antes de esconder la ventana.
   *
   * El reparto importa: **la escala va en `.body`, la opacidad en `.stage`**.
   * La piel se dibuja midiendo el body cuadro a cuadro, así que escalar el
   * body hace que la gota se deforme con él —que es justo el efecto—, mientras
   * que escalar el escenario entero escalaría la piel dos veces (una al
   * medirla ya transformada y otra al pintarla dentro del transform) y la
   * silueta se despegaría del contenido durante toda la animación.
   */
  .stage.is-in {
    animation: loupe-fade-in 220ms ease-out both;
  }

  .stage.is-in .body {
    animation: loupe-grow 260ms var(--ease-smooth-out, cubic-bezier(0.2, 0.8, 0.2, 1))
      both;
  }

  .stage.is-out {
    animation: loupe-fade-out 200ms cubic-bezier(0.4, 0, 1, 1) both;
  }

  .stage.is-out .body {
    animation: loupe-shrink 200ms cubic-bezier(0.4, 0, 1, 1) both;
  }

  /* Copiado: la gota se hincha y destella el color antes de irse. */
  .stage.is-out.is-copied {
    animation: loupe-flash 200ms cubic-bezier(0.4, 0, 1, 1) both;
  }

  .stage.is-out.is-copied .body {
    animation: loupe-pop 200ms cubic-bezier(0.4, 0, 1, 1) both;
  }

  @keyframes loupe-fade-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  @keyframes loupe-grow {
    from {
      transform: scale(0.84);
    }
    to {
      transform: none;
    }
  }

  @keyframes loupe-fade-out {
    from {
      opacity: 1;
    }
    to {
      opacity: 0;
    }
  }

  @keyframes loupe-shrink {
    from {
      transform: none;
    }
    to {
      transform: scale(0.9);
    }
  }

  @keyframes loupe-flash {
    0% {
      opacity: 1;
      filter: none;
    }
    35% {
      opacity: 1;
      filter: brightness(1.3);
    }
    100% {
      opacity: 0;
      filter: none;
    }
  }

  @keyframes loupe-pop {
    0% {
      transform: none;
    }
    35% {
      transform: scale(1.05);
    }
    100% {
      transform: scale(0.94);
    }
  }

  /* El morph nace del cuadro de muestra, que es donde mira el usuario. */
  .body {
    transform-origin: 32px 32px;
  }

  @media (prefers-reduced-motion: reduce) {
    .stage.is-in .body,
    .stage.is-out .body,
    .stage.is-out.is-copied .body,
    .grid.is-ready {
      animation: none;
    }
  }

  .preview {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .preview-content {
    flex: 1;
    min-width: 0;
  }

  .is-rose .grid,
  .is-rose .read {
    display: none;
  }

  .is-rose .bar {
    margin-top: 0;
  }

  .grid {
    display: block;
    width: 64px;
    height: 64px;
    flex: 0 0 64px;
    /*
     * El ojo asoma por la izquierda del cuerpo.
     *
     * No es un capricho de composición: el sistema líquido funde dos formas
     * cuando una **sale** de la otra, así que dibujado por dentro no habría
     * cuello que fundir y la lupa sería una cápsula con una miniatura pegada.
     */
    margin-left: -26px;
    image-rendering: pixelated;
    /* Redondo como el lóbulo que lo abriga, no como una miniatura. */
    border-radius: 15px;
    outline: 1px solid color-mix(in sRGB, var(--text) 14%, transparent);
    transition: opacity 180ms ease;
  }

  /* Empezó a leer: el ojo se abre. */
  .grid.is-ready {
    animation: sample-open 320ms var(--ease-smooth-out, cubic-bezier(0.2, 0.8, 0.2, 1))
      both;
  }

  @keyframes sample-open {
    from {
      opacity: 0.35;
      transform: scale(0.86);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }

  /* Sin leer todavía: el cuadro está apagado, no en negro sólido. */
  .grid:not(.is-ready) {
    opacity: 0.45;
  }

  button,
  input {
    font: inherit;
  }

  button {
    color: inherit;
    cursor: pointer;
  }

  button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  button:focus-visible,
  input:focus-visible,
  .ring:focus-visible {
    outline: 2px solid var(--text);
    outline-offset: 2px;
  }

  .read {
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 100%;
    padding: 5px 8px;
    border: 0;
    border-radius: 6px;
    font: 600 12px/1.4 var(--rb-mono, monospace);
    overflow-wrap: anywhere;
  }

  .swatch {
    width: 12px;
    height: 12px;
    flex: 0 0 12px;
    border-radius: 3px;
    outline: 1px solid currentColor;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
  }

  .help {
    margin: 0;
    color: var(--muted);
    font-size: 10px;
    text-wrap: pretty;
  }

  .rose-btn {
    border: 0;
    border-radius: 5px;
    padding: 3px 6px;
    background: color-mix(in sRGB, var(--text) 8%, transparent);
    font-size: 11px;
  }

  .rose-btn:hover,
  .rose-btn[aria-pressed="true"] {
    background: color-mix(in sRGB, var(--text) 15%, transparent);
  }

  .rose {
    margin-top: 14px;
    padding-top: 12px;
    border-top: 1px solid var(--line);
  }

  .rose-head {
    display: flex;
    gap: 10px;
    align-items: center;
    margin-bottom: 12px;
  }

  .rose-swatch {
    width: 40px;
    height: 40px;
    flex: 0 0 40px;
    border: 0;
    border-radius: 8px;
    outline: 1px solid var(--line);
  }

  .meta {
    display: flex;
    flex: 1;
    min-width: 0;
    flex-direction: column;
  }

  .code {
    padding: 1px 0;
    border: 0;
    text-align: left;
    background: transparent;
    color: var(--muted);
    font: 11px/1.5 var(--rb-mono, monospace);
  }

  .code.is-on {
    color: var(--text);
    font-weight: 600;
  }

  .wheel {
    position: relative;
    width: 176px;
    height: 176px;
    margin: 0 auto 12px;
  }

  .ring {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    cursor: crosshair;
    touch-action: none;
  }

  .ring-fill {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: conic-gradient(from 0deg, red, yellow, lime, cyan, blue, magenta, red);
    mask: radial-gradient(
      farthest-side,
      transparent calc(100% - 14px),
      black calc(100% - 13px)
    );
  }

  .hue-knob {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 10px;
    height: 10px;
    margin: -5px 0 0 -5px;
    border-radius: 50%;
    background: white;
    box-shadow: 0 0 0 1px black;
    transform: rotate(var(--hue)) translateY(-81px);
    pointer-events: none;
  }

  .sv {
    position: absolute;
    inset: 36px;
    width: 104px;
    height: 104px;
    border-radius: 6px;
    cursor: crosshair;
    touch-action: none;
  }

  .sv-knob {
    position: absolute;
    width: 10px;
    height: 10px;
    margin: -5px 0 0 -5px;
    border-radius: 50%;
    background: white;
    box-shadow: 0 0 0 1px black;
    pointer-events: none;
  }

  .channels {
    margin: 10px 0;
  }

  .channels label {
    display: grid;
    grid-template-columns: 72px minmax(0, 1fr) 36px;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--muted);
  }

  .channels input {
    width: 100%;
    accent-color: var(--text);
  }

  .channels output {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .hex-entry {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
    font-size: 11px;
  }

  .hex-entry input {
    min-width: 0;
    width: 100%;
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: 5px;
    padding: 5px 7px;
    font-family: var(--rb-mono, monospace);
  }

  .hex-entry button {
    border: 0;
    border-radius: 5px;
    padding: 6px 8px;
    background: color-mix(in sRGB, var(--text) 8%, transparent);
  }

  .ticks {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .tick {
    width: 17px;
    height: 17px;
    border: 0;
    border-radius: 50%;
    outline: 1px solid var(--line);
  }

  .recent {
    margin-top: 10px;
  }

  .recent-label {
    display: block;
    margin-bottom: 5px;
    font-size: 10px;
    color: var(--muted);
  }

  .copy {
    display: block;
    width: 100%;
    margin-top: 12px;
    border: 0;
    border-radius: 6px;
    padding: 8px 10px;
    background: var(--text);
    color: var(--bg);
    font-weight: 600;
  }

  .copy:hover {
    opacity: 0.9;
  }

  .cancel {
    display: block;
    margin: 5px 0 0 auto;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--muted);
    font-size: 10px;
  }

  .is-rose .cancel {
    margin: 10px auto 0;
  }

  .error {
    margin: 8px 0;
    color: var(--text);
    font-size: 11px;
    overflow-wrap: anywhere;
  }
</style>
