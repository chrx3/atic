<script lang="ts">
  /**
   * Polvo de fondo del picker: motas tenues detrás de la piel líquida.
   *
   * Misma filosofía que el campo de `ParticleWheel`: **el campo no guarda
   * velocidades ni posiciones**. Cada mota tiene un ancla en coordenadas
   * normalizadas del panel; de ahí sale su radio orbital respecto al centro
   * de la rueda —el mismo que dibuja el arco de la izquierda, fuera de
   * pantalla— y lo único que avanza es su recorrido a lo largo de esa órbita.
   * Así un resize nunca «rehace» el campo (el ancla se re-proyecta y ya) y
   * nada puede quedarse pegado al cursor.
   *
   * Por eso las motas no caen en vertical: van sobre círculos concéntricos
   * con la rueda, se curvan hacia ella arriba y abajo, y las de más adentro
   * se meten detrás del arco. El recorrido lo manda el giro real: cada mota
   * avanza lo mismo que las cards (con paralaje por profundidad) y se
   * envuelve, así que girar la rueda es pasar por un anillo de estrellas.
   *
   * Es decoración: `pointer-events: none` y `aria-hidden`. El puntero lo
   * escucha en su propio padre (`<nav class="wheel">`) en vez de que la rueda
   * le pase los eventos, para que no exista forma de que este efecto altere el
   * arrastre, el clic o la navegación.
   */
  import { untrack } from "svelte";
  import { MOTION, ms, prefersReducedMotion } from "$lib/motion";

  let {
    /**
     * Cuánto se ha corrido el contenido de la rueda, en px (negativo = subió).
     *
     * Es una función y no un número porque el bucle la lee **cada frame**: así
     * el campo viaja pegado al giro real —rueda, arrastre o teclado— sin
     * re-renderizar este componente en cada paso.
     */
    travel = () => 0,
    /**
     * Centro de las órbitas en X, relativo al panel. Queda muy a la izquierda
     * y fuera de pantalla: es el centro del círculo del que sale el arco.
     */
    centerX = 0,
  }: { travel?: () => number; centerX?: number } = $props();

  /** Alcance del campo del puntero (px). */
  const REACH = 150;
  /** Cuánto aparta el puntero en el centro del campo (px). */
  const PUSH = 26;
  /** Componente tangencial: el empujón gira un poco en vez de ser radial puro. */
  const SWIRL = 9;
  /**
   * Un área por mota (px²). Con ~1000×600 da ~87 motas.
   *
   * Más densa que un campo suelto a propósito: al ir sobre órbitas, las de
   * radio corto pasan buena parte del recorrido detrás del arco y no se ven.
   */
  const AREA_PER_MOTE = 7000;
  /** Colchón del envoltorio vertical: órbita + empujón + radio del sprite. */
  const WRAP_MARGIN = 64;
  /** Tope de la estela (px). Sin él, un envión pinta barras de lado a lado. */
  const STREAK_MAX = 80;
  /** Cadencia en reposo. El campo solo respira: a 60 Hz no se ve distinto. */
  const IDLE_FRAME = 1 / 30;
  /** Por debajo de esto la mota no se ve: no vale el coste de componerla. */
  const ALPHA_FLOOR = 0.004;

  let canvas = $state<HTMLCanvasElement | null>(null);

  // `$effect` y no `onMount`: depende de que `bind:this` ya esté asignado.
  $effect(() => {
    const el = canvas;
    const host = el?.parentElement;
    const ctx = el?.getContext("2d");
    if (!el || !host || !ctx) return;
    // `untrack` obligatorio: el primer `draw()` sincrónico lee `travel()`, y
    // con él el giro de la rueda. Sin esto el efecto se suscribía al giro y
    // cada paso rehacía el campo entero —motas nuevas y trabajo de más—.
    return untrack(() => runDust(host, ctx));
  });

  /** La simulación vive aparte para trabajar con referencias ya no nulas. */
  function runDust(host: HTMLElement, ctx: CanvasRenderingContext2D) {
    const canvasEl = ctx.canvas;
    const reduceMotion = prefersReducedMotion();

    type Mote = {
      /** Ancla en el panel, normalizada: sobrevive a cualquier resize. */
      u: number;
      v: number;
      /** Recorrido propio sobre la órbita (px de arco) y su velocidad (px/s). */
      arc: number;
      vs: number;
      /** 0 = lejos (chica y apagada), 1 = cerca (grande y brillante). */
      depth: number;
      /** Fracción del viaje de las cards que recorre esta mota. */
      parallax: number;
      size: number;
      /** Fase y velocidad del titileo. */
      tw: number;
      tws: number;
    };

    // Constante de tiempo del puntero (s), leída del token de motion para que
    // siga a la escala del CSS. El viaje vertical no se suaviza acá: ya viene
    // amortiguado por el muelle de la rueda, y cualquier retardo extra
    // despegaría las estrellas de las cards.
    const followS = ms(MOTION.slow) / 1000;

    let motes: Mote[] = [];
    let raf = 0;
    let running = false;
    let cssW = 0;
    let cssH = 0;
    let prev = 0;
    /** Escala del lienzo. La guarda `draw` para poder girar con `setTransform`. */
    let dpr = 1;
    /** Tiempo acumulado sin dibujar, para la cadencia reducida en reposo. */
    let pending = 0;
    /** Viaje del cuadro anterior. `null` = no hay estela que calcular todavía. */
    let lastShift: number | null = null;
    /** Rect cacheado: un `getBoundingClientRect` por pointermove es un reflow. */
    let hostRect: DOMRect | null = null;

    // Puntero suavizado + peso con inercia: la interacción entra y sale en
    // fundido en vez de a saltos.
    let px = 0;
    let py = 0;
    let tx = 0;
    let ty = 0;
    let inf = 0;
    let infTarget = 0;

    let ink: [number, number, number] = [23, 23, 20];
    let sprite: HTMLCanvasElement | null = null;

    /** Normaliza cualquier color CSS a RGB usando el propio canvas. */
    function resolveRgb(value: string, fallback: [number, number, number]) {
      ctx.fillStyle = "#000";
      ctx.fillStyle = value.trim();
      const normalized = ctx.fillStyle;
      if (typeof normalized !== "string") return fallback;
      if (normalized.startsWith("#") && normalized.length === 7) {
        return [
          parseInt(normalized.slice(1, 3), 16),
          parseInt(normalized.slice(3, 5), 16),
          parseInt(normalized.slice(5, 7), 16),
        ] as [number, number, number];
      }
      const channels = normalized.match(/[\d.]+/g);
      if (!channels || channels.length < 3) return fallback;
      return [Number(channels[0]), Number(channels[1]), Number(channels[2])] as [
        number,
        number,
        number,
      ];
    }

    /** Sprite de degradado radial: bruma, no círculos duros. */
    function buildSprite() {
      const side = 48;
      const off = document.createElement("canvas");
      off.width = side;
      off.height = side;
      const g = off.getContext("2d");
      if (!g) return;
      const rgb = `${ink[0]}, ${ink[1]}, ${ink[2]}`;
      const half = side / 2;
      const grad = g.createRadialGradient(half, half, 0, half, half, half);
      grad.addColorStop(0, `rgba(${rgb}, 0.85)`);
      grad.addColorStop(0.25, `rgba(${rgb}, 0.42)`);
      grad.addColorStop(0.55, `rgba(${rgb}, 0.12)`);
      grad.addColorStop(0.8, `rgba(${rgb}, 0.03)`);
      grad.addColorStop(1, `rgba(${rgb}, 0)`);
      g.fillStyle = grad;
      g.fillRect(0, 0, side, side);
      sprite = off;
    }

    function readPalette() {
      const next = resolveRgb(
        getComputedStyle(host).getPropertyValue("--rb-text"),
        ink,
      );
      if (sprite && next[0] === ink[0] && next[1] === ink[1] && next[2] === ink[2]) {
        return;
      }
      ink = next;
      buildSprite();
      if (reduceMotion) draw(0, 0, travel(), 0);
    }

    function makeMote(): Mote {
      const depth = Math.random();
      return {
        u: Math.random(),
        v: Math.random(),
        arc: 0,
        // Todas en el mismo sentido: es lo que se lee como anillo girando en
        // vez de como polvo revuelto. 1.5–5.5 px/s, un cruce en varios minutos.
        vs: 1.5 + Math.random() * 4,
        depth,
        // Alrededor de 1: la mota media acompaña a la card. Las de arriba y
        // abajo de ese rango son las que dan la sensación de profundidad.
        parallax: 0.4 + depth * 0.9,
        size: 0.7 + depth * 1.7,
        tw: Math.random() * Math.PI * 2,
        tws: 0.15 + Math.random() * 0.3,
      };
    }

    /** Ajusta la densidad sin tocar las motas existentes. */
    function fitCount() {
      if (cssW <= 0 || cssH <= 0) return;
      const want = Math.round(
        Math.min(96, Math.max(24, (cssW * cssH) / AREA_PER_MOTE)),
      );
      while (motes.length < want) motes.push(makeMote());
      if (motes.length > want * 1.4) motes.length = want;
    }

    function resize() {
      hostRect = null;
      cssW = host.clientWidth;
      cssH = host.clientHeight;
      if (cssW === 0 || cssH === 0) return;
      dpr = Math.min(window.devicePixelRatio || 1, 2);
      canvasEl.width = Math.round(cssW * dpr);
      canvasEl.height = Math.round(cssH * dpr);
      canvasEl.style.width = `${cssW}px`;
      canvasEl.style.height = `${cssH}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      readPalette();
      fitCount();
      if (reduceMotion) draw(0, 0, travel(), 0);
    }

    function draw(t: number, dt: number, shift: number, dShift: number) {
      ctx.clearRect(0, 0, cssW, cssH);
      if (!sprite) return;

      // Franja infinita: al envolver por el alto (más colchón) el campo nunca
      // se agota por mucho que se gire la rueda, y no hace falta reciclar
      // motas —que es justo lo que reintroduciría estado en el campo—.
      const band = cssH + WRAP_MARGIN * 2;
      const pointerLive = inf > 0.002;
      /** La matriz girada de la estela queda puesta; hay que deshacerla. */
      let rotated = false;
      const cx = centerX;
      // Mismo centro vertical que el arco de la rueda (`geometry.wheelCy`).
      const cy = Math.max(cssH, 320) / 2;

      for (const p of motes) {
        p.arc += p.vs * dt;
        const anchorX = p.u * cssW;
        const anchorY = p.v * cssH;
        // El radio sale del ancla, no de dónde esté ahora: es lo que fija la
        // órbita de esta mota y sobrevive al giro y al resize.
        const r = Math.hypot(anchorX - cx, anchorY - cy);
        const raw = anchorY + p.arc + shift * p.parallax + WRAP_MARGIN;
        let y = (((raw % band) + band) % band) - WRAP_MARGIN;
        const dy = y - cy;
        // La X no es libre: la mota va sobre su círculo, así que al alejarse
        // del centro vertical se curva hacia la rueda. Fuera del círculo no
        // hay punto —la mota está pasando por detrás del arco—; ahí `cx` la
        // deja fuera de pantalla y el descarte de abajo se la salta.
        const radial = Math.sqrt(Math.max(0, r * r - dy * dy));
        let x = cx + radial;
        // Techo ~0.31: por encima deja de ser polvo y se lee como ruido.
        let alpha =
          (0.11 + p.depth * 0.2) * (0.7 + 0.3 * Math.sin(t * p.tws + p.tw));

        // Campo del puntero: aparta sin alterar la órbita ni acumular estado.
        if (pointerLive) {
          const toX = x - px;
          const toY = y - py;
          const d = Math.hypot(toX, toY);
          if (d < REACH && d > 0.001) {
            const k = (1 - d / REACH) ** 2 * inf;
            const ux = toX / d;
            const uy = toY / d;
            x += (ux * PUSH - uy * SWIRL) * k;
            y += (uy * PUSH + ux * SWIRL) * k;
            alpha *= 1 + k * 0.9;
          }
        }

        const radius = p.size * 3.2;
        if (x + radius < 0 || x - radius > cssW) continue;

        // Estela: el sprite se estira hasta donde estaba la mota el cuadro
        // anterior. Es un solo `drawImage` igual que sin estela —estirar sale
        // gratis; dibujar la traza punto a punto no—. Al alargarse se apaga,
        // si no una mota rápida se leería como una barra sólida.
        const streak = Math.min(STREAK_MAX, Math.abs(dShift * p.parallax));
        if (streak > 0.5 && r > 1) {
          alpha /= Math.sqrt(1 + streak / (radius * 2));
          if (alpha < ALPHA_FLOOR) continue;
          ctx.globalAlpha = Math.min(0.55, alpha);
          // La estela va sobre la tangente de la órbita, no en vertical: es lo
          // que hace que al girar se lea un anillo y no lluvia.
          //
          // La tangente es el radio girado 90°, así que su seno y su coseno ya
          // están calculados: nada de `atan2` ni `rotate`. Y una matriz suelta
          // en vez de `save`/`restore` — son 96 motas por cuadro y esto se
          // paga en cada una.
          const cos = radial / r;
          const sin = dy / r;
          ctx.setTransform(dpr * cos, dpr * sin, -dpr * sin, dpr * cos, dpr * x, dpr * y);
          const top = dShift < 0 ? 0 : -streak;
          ctx.drawImage(sprite, -radius, top - radius, radius * 2, radius * 2 + streak);
          rotated = true;
          continue;
        }
        if (rotated) {
          ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
          rotated = false;
        }

        if (alpha < ALPHA_FLOOR) continue;
        ctx.globalAlpha = Math.min(0.55, alpha);
        ctx.drawImage(sprite, x - radius, y - radius, radius * 2, radius * 2);
      }
      if (rotated) ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.globalAlpha = 1;
    }

    function loop(now: number) {
      if (!running) return;
      const t = now / 1000;
      const dt = prev ? Math.min(0.05, t - prev) : 0.016;
      prev = t;
      // Con `followS` = 0 (movimiento reducido) el bucle ni arranca; el
      // guardia es solo para no dividir por cero si el token no resuelve.
      const follow = followS > 0 ? Math.min(1, dt / followS) : 1;
      px += (tx - px) * follow;
      py += (ty - py) * follow;
      inf += (infTarget - inf) * follow;
      pending += dt;

      const shift = travel();
      const dShift = lastShift === null ? 0 : shift - lastShift;

      // Quieto no hace falta ir a 60 Hz: este webview comparte proceso con el
      // overlay y la transcripción, y el cuadro es de ellos primero.
      if (inf > 0.002 || Math.abs(dShift) > 0.05 || pending >= IDLE_FRAME) {
        lastShift = shift;
        draw(t, pending, shift, dShift);
        pending = 0;
      }
      raf = requestAnimationFrame(loop);
    }

    /** Solo se dibuja cuando el rail se ve de verdad y la ventana está viva. */
    function shouldRun(): boolean {
      if (document.hidden) return false;
      if (cssW === 0 || cssH === 0) return false;
      if (typeof host.checkVisibility === "function") {
        // `visibility: hidden` en un ancestro (`.rail--away`) no quita el
        // layout, así que ni `offsetParent` ni IntersectionObserver lo ven.
        return host.checkVisibility({
          visibilityProperty: true,
          checkVisibilityCSS: true,
        });
      }
      return host.offsetParent !== null;
    }

    function sync() {
      const want = !reduceMotion && shouldRun();
      if (want === running) return;
      running = want;
      if (running) {
        prev = 0;
        pending = 0;
        // Volver de estar oculto no es movimiento: sin esto, el viaje
        // acumulado mientras no se veía saldría como un estelazo.
        lastShift = null;
        raf = requestAnimationFrame(loop);
      } else {
        cancelAnimationFrame(raf);
        raf = 0;
      }
    }

    function onPointerEnter() {
      hostRect = null;
    }

    function onPointerMove(event: PointerEvent) {
      // El rect se mide una vez por hover, no por evento: medirlo en cada
      // pointermove fuerza un reflow justo mientras el Skin recalcula su SDF.
      const rect = (hostRect ??= host.getBoundingClientRect());
      tx = event.clientX - rect.left;
      ty = event.clientY - rect.top;
      // Primer contacto: partir del cursor, no barrer desde (0,0).
      if (inf < 0.01) {
        px = tx;
        py = ty;
      }
      infTarget = 1;
    }

    function onPointerLeave() {
      infTarget = 0;
    }

    const sizeObserver = new ResizeObserver(() => {
      resize();
      sync();
    });
    sizeObserver.observe(host);

    // El tema vive en <html data-theme>; al cambiar hay que releer los tokens.
    const themeObserver = new MutationObserver(() => readPalette());
    themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme", "data-theme-base"],
    });

    // El rail se tapa con `visibility`/`inert` en un ancestro, no en el canvas:
    // hay que mirar la cadena entera para saber cuándo dejar de dibujar.
    const gateObserver = new MutationObserver(() => sync());
    for (
      let node: HTMLElement | null = host;
      node && node !== document.body;
      node = node.parentElement
    ) {
      gateObserver.observe(node, {
        attributes: true,
        attributeFilter: ["class", "style", "inert", "hidden"],
      });
    }

    host.addEventListener("pointerenter", onPointerEnter, { passive: true });
    host.addEventListener("pointermove", onPointerMove, { passive: true });
    host.addEventListener("pointerleave", onPointerLeave, { passive: true });
    document.addEventListener("visibilitychange", sync);

    resize();
    sync();

    return () => {
      running = false;
      cancelAnimationFrame(raf);
      sizeObserver.disconnect();
      themeObserver.disconnect();
      gateObserver.disconnect();
      host.removeEventListener("pointerenter", onPointerEnter);
      host.removeEventListener("pointermove", onPointerMove);
      host.removeEventListener("pointerleave", onPointerLeave);
      document.removeEventListener("visibilitychange", sync);
    };
  }
</script>

<canvas class="rail-dust" bind:this={canvas} aria-hidden="true"></canvas>

<style>
  /*
   * z-index 0 + primero en el DOM: queda bajo el `Skin` (z-index auto) y bajo
   * `.ink-layer` (z-index 1). La piel es opaca, así que el polvo solo asoma
   * en el fondo vacío, que es donde tiene que verse.
   */
  .rail-dust {
    position: absolute;
    inset: 0;
    z-index: 0;
    display: block;
    pointer-events: none;
  }
</style>
