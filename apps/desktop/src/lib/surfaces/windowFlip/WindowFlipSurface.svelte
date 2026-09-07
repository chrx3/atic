<script lang="ts">
  /**
   * Tapa 3D sobre una ventana ajena: el frente es una captura, el reverso un bloc.
   *
   * El overlay es más grande que la ventana y transparente. La tarjeta ocupa
   * solo el marco visible; al girar se ve el escritorio a los lados.
   */
  import { onMount } from "svelte";
  import { t } from "$domain/i18n.svelte";
  import {
    closeWindowFlip,
    concealWindowFlip,
    onWindowFlipOpen,
    onWindowFlipRequestClose,
    presentWindowFlip,
    saveWindowFlipBlocks,
    windowFlipFocusIsForeign,
    windowFlipPreviewSrc,
    windowFlipState,
    type NoteBlock,
    type WindowFlipView,
  } from "$ipc/windowFlip";
  import FlipBoard from "./FlipBoard.svelte";
  import { colocarSiHaceFalta } from "./flipLayout";

  // Ida más lenta que vuelta: al abrir se descubre algo, al volver ya se
  // sabe a dónde va. Los tokens de la app llegan hasta 250 ms; la tarjeta es
  // la superficie más grande que hay, así que pide un poco más.
  const IDA_MS = 400;
  const VUELTA_MS = 320;
  const RESPALDO_MS = VUELTA_MS + 180;
  const FOTO_ESPERA_MS = 600;
  // Cámara y hundido, los dos en anchos de tarjeta.
  //
  // La cámara cerca exagera la mitad que se acerca: a 90° una se ve 1.5 veces
  // la otra y el giro parece pasar por la izquierda en vez de por el centro.
  // El hundido de medio ancho deja el borde cercano justo en z = 0, así nunca
  // crece por encima de su tamaño real (importa en ventana maximizada, donde
  // el overlay no tiene aire de sobra) y las dos mitades se parecen mucho más.
  const CAMARA = 4.5;
  const HUNDIDO = 0.5;

  let view = $state<WindowFlipView | null>(null);
  let bloques = $state<NoteBlock[]>([]);
  let showBack = $state(false);
  let cardReady = $state(false);
  let giro = $state<"" | "reverso" | "frente">("");
  let previewSrc = $state("");
  let guardado = $state(false);
  let anchoVentana = $state(0);
  let altoVentana = $state(0);
  let tarjetaEl: HTMLDivElement | undefined;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let avisoTimer: ReturnType<typeof setTimeout> | undefined;
  let closeTimer: ReturnType<typeof setTimeout> | undefined;
  let closing = false;
  // Corta un `apply` viejo si el atajo se pulsa dos veces seguidas.
  let generacion = 0;

  const anchoTarjeta = $derived((view?.cardWidth ?? 1) * anchoVentana);
  // Ventana chica: el encabezado y el pie se comen el bloc.
  const compacta = $derived((view?.cardHeight ?? 1) * altoVentana < 260);
  const focoX = $derived(((view?.cardLeft ?? 0) + (view?.cardWidth ?? 1) / 2) * 100);
  const focoY = $derived(((view?.cardTop ?? 0) + (view?.cardHeight ?? 1) / 2) * 100);

  function pct(value: number | undefined, fallback: number): string {
    return `${((value ?? fallback) * 100).toFixed(4)}%`;
  }

  function srcDe(path: string): string {
    // `GEN` vuelve a cero al reiniciar la app y la ruta se repite: sin esto
    // el webview puede servir la foto cacheada de la sesión anterior.
    return path ? `${windowFlipPreviewSrc(path)}?t=${Date.now()}` : "";
  }

  // `false` = la foto no llegó a tiempo; girar así muestra el escritorio.
  function esperarFoto(src: string): Promise<boolean> {
    if (!src) return Promise.resolve(true);
    return new Promise((resolve) => {
      const img = new Image();
      let done = false;
      const finish = (ok: boolean) => {
        if (done) return;
        done = true;
        resolve(ok);
      };
      img.onload = () => finish(true);
      img.onerror = () => finish(false);
      img.src = src;
      setTimeout(() => finish(false), FOTO_ESPERA_MS);
    });
  }

  async function apply(next: WindowFlipView) {
    const mio = ++generacion;
    closing = false;
    if (closeTimer) clearTimeout(closeTimer);
    view = next;
    bloques = colocarSiHaceFalta(next.blocks);
    cajonAbierto = false;
    previewSrc = srcDe(next.previewPath);
    showBack = false;
    giro = "";
    cardReady = false;
    guardado = false;
    if (avisoTimer) clearTimeout(avisoTimer);
    if (!(await esperarFoto(previewSrc))) {
      // Sin foto, la tapa transparente deja ver el escritorio en cuanto se
      // esconde la ventana viva. Mejor no voltear nada.
      if (mio === generacion) void closeWindowFlip();
      return;
    }
    if (vencido(mio)) return;
    // `ready` promueve la capa: el compositor rasteriza la foto antes de
    // que arranque el giro, no en su primer frame.
    cardReady = true;
    await siguienteFrame();
    if (vencido(mio)) return;
    try {
      await presentWindowFlip();
    } catch {
      // Sin tapa nativa igual se gira la foto.
    }
    if (vencido(mio)) return;
    await siguienteFrame();
    await siguienteFrame();
    try {
      await concealWindowFlip();
    } catch {
      // La ventana viva queda debajo; el giro igual corre.
    }
    if (vencido(mio)) return;
    // `conceal` cloakea y hace dos DwmFlush con el hilo principal tomado.
    // Sin un par de frames de aire, el giro pierde los primeros.
    await siguienteFrame();
    await siguienteFrame();
    if (vencido(mio)) return;
    giro = "reverso";
    showBack = true;
    if (sinMovimiento()) asentarGiro();
  }

  function vencido(mio: number): boolean {
    return closing || mio !== generacion;
  }

  function siguienteFrame(): Promise<void> {
    return new Promise((resolve) => requestAnimationFrame(() => resolve()));
  }

  function persist() {
    if (saveTimer) clearTimeout(saveTimer);
    const copia = $state.snapshot(bloques) as NoteBlock[];
    saveTimer = setTimeout(() => {
      void saveWindowFlipBlocks(copia)
        .then(avisarGuardado)
        .catch(() => {
          // Sin aviso: decir "guardado" cuando no se guardó es peor que callar.
        });
    }, 350);
  }

  let cajonAbierto = $state(false);

  // El placeholder promete que se guardan solas; esto lo confirma.
  function avisarGuardado() {
    guardado = true;
    if (avisoTimer) clearTimeout(avisoTimer);
    avisoTimer = setTimeout(() => {
      guardado = false;
    }, 1600);
  }

  function sinMovimiento(): boolean {
    return (
      typeof window !== "undefined" &&
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true
    );
  }

  async function beginClose() {
    if (closing) return;
    closing = true;
    generacion += 1;
    if (saveTimer) clearTimeout(saveTimer);
    void saveWindowFlipBlocks($state.snapshot(bloques) as NoteBlock[]);

    if (sinMovimiento() || !cardReady) {
      terminarCierre();
      return;
    }
    showBack = false;
    // Si todavía está yendo al reverso (Esc apenas se abrió), se devuelve
    // desde donde va. Cambiar de animación la haría saltar a 180° primero.
    const enVuelo = tarjetaEl
      ?.getAnimations()
      .find((animacion) => animacion.playState === "running");
    if (enVuelo) {
      enVuelo.reverse();
    } else {
      giro = "frente";
    }
    closeTimer = setTimeout(terminarCierre, RESPALDO_MS);
  }

  function terminarCierre() {
    if (!closing) return;
    if (closeTimer) clearTimeout(closeTimer);
    closeTimer = undefined;
    void closeWindowFlip();
  }

  function asentarGiro() {
    giro = "";
  }

  // La tapa cubre la ventana más un poco de aire y se come esos clics. Que
  // el clic en el aire cierre es la salida obvia: la ventana de abajo vuelve
  // justo donde el usuario apuntó.
  function alClicAfuera(event: MouseEvent) {
    if ((event.target as HTMLElement).closest(".card")) return;
    void beginClose();
  }

  // Otra app se llevó el foco (alt-tab, clic en una ventana de al lado).
  // Solo una vez asentado el giro: durante la apertura el foco va y viene.
  //
  // Abrir una flotante de Atic (el portapapeles, el launcher) también quita el
  // foco, y ahí cerrar es lo contrario de lo que el usuario pidió: se pregunta
  // de qué ventana se trata antes de decidir. El respiro es porque al momento
  // del `blur` Windows todavía puede no haber actualizado el primer plano.
  function alPerderFoco() {
    if (!(cardReady && showBack && giro === "")) return;
    setTimeout(() => {
      if (!(cardReady && showBack && giro === "")) return;
      void windowFlipFocusIsForeign()
        .then((ajena) => {
          if (ajena) void beginClose();
        })
        .catch(() => void beginClose());
    }, 150);
  }

  function alTerminarGiro(event: AnimationEvent) {
    if (!(event.target as HTMLElement).classList.contains("card")) return;
    if (closing && !showBack) {
      giro = "";
      void siguienteFrame()
        .then(siguienteFrame)
        .then(() => terminarCierre());
      return;
    }
    asentarGiro();
  }

  onMount(() => {
    void windowFlipState().then((current) => {
      if (current) apply(current);
    });
    const stopOpen = onWindowFlipOpen(apply);
    const stopClose = onWindowFlipRequestClose(() => {
      void beginClose();
    });
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        void beginClose();
      }
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", alPerderFoco);
    return () => {
      void stopOpen.then((unlisten) => unlisten());
      void stopClose.then((unlisten) => unlisten());
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", alPerderFoco);
      if (saveTimer) clearTimeout(saveTimer);
      if (avisoTimer) clearTimeout(avisoTimer);
      if (closeTimer) clearTimeout(closeTimer);
    };
  });
</script>

<svelte:window bind:innerWidth={anchoVentana} bind:innerHeight={altoVentana} />

<div class="root" role="presentation" onmousedown={alClicAfuera}>
  <div
    class="stage"
    style:--foco-x={`${focoX.toFixed(4)}%`}
    style:--foco-y={`${focoY.toFixed(4)}%`}
    style:--camara={`${(CAMARA * anchoTarjeta).toFixed(2)}px`}
    style:--hundido={`${(-HUNDIDO * anchoTarjeta).toFixed(2)}px`}
    style:--giro-ida={`${IDA_MS}ms`}
    style:--giro-vuelta={`${VUELTA_MS}ms`}
  >
    <div
      bind:this={tarjetaEl}
      class="card"
      class:ready={cardReady}
      class:flipped={showBack}
      class:al-reverso={giro === "reverso"}
      class:al-frente={giro === "frente"}
      style:left={pct(view?.cardLeft, 0)}
      style:top={pct(view?.cardTop, 0)}
      style:width={pct(view?.cardWidth, 1)}
      style:height={pct(view?.cardHeight, 1)}
      onanimationend={alTerminarGiro}
    >
      <div class="face front">
        {#if previewSrc}
          <img src={previewSrc} alt="" />
        {:else}
          <div class="blank"></div>
        {/if}
      </div>
      <div class="face back" class:compacta>
        <header>
          <p class="kicker">
            {t("overlay.windowFlip.kicker", { exe: view?.exe ?? "" })}
          </p>
          <h1>{view?.title || t("overlay.windowFlip.untitled")}</h1>
        </header>
        <FlipBoard
          bind:bloques
          bind:cajonAbierto
          assetsDir={view?.assetsDir ?? ""}
          {compacta}
          onpersist={persist}
        />
        <footer>
          <p class="estado">
            <span class:apagada={guardado}>{t("overlay.windowFlip.hint")}</span>
            <span class="guardado" class:visible={guardado} aria-live="polite">
              {guardado ? t("overlay.windowFlip.saved") : ""}
            </span>
          </p>
          <div class="acciones">
            <button
              type="button"
              class="rb-btn rb-btn-ghost"
              aria-pressed={cajonAbierto}
              onclick={() => (cajonAbierto = !cajonAbierto)}
            >
              {t("overlay.windowFlip.drawer")}
            </button>
            <button
              type="button"
              class="rb-btn rb-btn-soft"
              onclick={() => void beginClose()}
            >
              {t("overlay.windowFlip.close")}
            </button>
          </div>
        </footer>
      </div>
    </div>
  </div>
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    height: 100%;
    overflow: hidden;
    background: transparent;
  }

  .root {
    width: 100%;
    height: 100%;
    background: transparent;
  }

  .stage {
    /* `--ease-smooth-out` y compañía son de una sola pieza; estas dos son
       `cubic-bezier(0.45, 0, 0.55, 1)` cortada al medio (ver los keyframes). */
    --giro-entrada: cubic-bezier(0.45, 0, 0.725, 0.5);
    --giro-salida: cubic-bezier(0.275, 0.5, 0.55, 1);

    position: relative;
    width: 100%;
    height: 100%;

    /* Perspectiva proporcional a la tarjeta: fija, en una ventana grande el
       borde que se acerca crece tanto que se sale del overlay y se corta. */
    perspective: max(1200px, var(--camara, 3000px));

    /* El punto de fuga va en el centro de la tarjeta, no del overlay: la
       tarjeta rara vez está centrada y el giro sale torcido. */
    perspective-origin: var(--foco-x, 50%) var(--foco-y, 50%);
    background: transparent;
  }

  .card {
    position: absolute;
    transform-style: preserve-3d;
    transform: rotateY(0deg);
    transform-origin: center center;
    visibility: hidden;
    border-radius: 8px;
  }

  .card.ready {
    visibility: visible;

    /* La capa se rasteriza al marcar `ready`, no en el primer frame del
       giro: si no, la foto entera se textura con la animación ya corriendo. */
    will-change: transform;
  }

  .card.ready.flipped {
    transform: rotateY(180deg);
  }

  .card.ready.al-reverso {
    animation: girar-al-reverso var(--giro-ida, 400ms) linear both;
  }

  .card.ready.al-frente {
    animation: girar-al-frente var(--giro-vuelta, 320ms) linear both;
  }

  /* El `translateZ` va ANTES del `rotateY`: al revés la rotación se aplica
     primero al sistema de coordenadas y a 90° el eje Z local ya apunta al X
     de la pantalla, así que en vez de alejarse la tarjeta se va de paseo
     hacia la izquierda.
     La curva, además, va por tramo: con una sola en el atajo cada mitad la
     aplica de nuevo y el giro frena al pasar por los 90°. */
  @keyframes girar-al-reverso {
    0% {
      transform: translateZ(0) rotateY(0deg);
      animation-timing-function: var(--giro-entrada);
    }

    50% {
      transform: translateZ(var(--hundido, -240px)) rotateY(90deg);
      animation-timing-function: var(--giro-salida);
    }

    100% {
      transform: translateZ(0) rotateY(180deg);
    }
  }

  @keyframes girar-al-frente {
    0% {
      transform: translateZ(0) rotateY(180deg);
      animation-timing-function: var(--giro-entrada);
    }

    50% {
      transform: translateZ(var(--hundido, -240px)) rotateY(90deg);
      animation-timing-function: var(--giro-salida);
    }

    100% {
      transform: translateZ(0) rotateY(0deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .card.ready.al-reverso,
    .card.ready.al-frente {
      animation: none;
    }
  }

  .face {
    position: absolute;
    inset: 0;
    overflow: hidden;
    border-radius: 8px;
    -webkit-backface-visibility: hidden;
    backface-visibility: hidden;

    /* La sombra va en cada cara y no en la tarjeta: en el contenedor 3D
       gira con ella y se proyecta como una plancha en perspectiva. */
    box-shadow:
      0 22px 48px rgb(0 0 0 / 32%),
      0 2px 8px rgb(0 0 0 / 18%);
  }

  .front {
    transform: rotateY(0deg) translateZ(1px);
  }

  .front img,
  .blank {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: fill;
    image-rendering: auto;
    background: var(--rb-surface-2);
  }

  .back {
    --pad: 18px;

    display: flex;
    flex-direction: column;
    background: var(--rb-surface);
    color: var(--rb-text);
    transform: rotateY(180deg) translateZ(1px);
    box-sizing: border-box;
  }

  .back.compacta {
    --pad: 12px;
  }

  header {
    display: grid;
    gap: 3px;
    padding: var(--pad) var(--pad) 10px;
  }

  .kicker {
    margin: 0;
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.04em;
    text-transform: lowercase;
    color: var(--rb-faint);
  }

  header h1 {
    margin: 0;
    font-family: var(--rb-display);
    font-size: 16px;
    font-weight: 600;
    line-height: 1.3;

    /* El título es de la ventana ajena: puede ser una frase entera. */
    text-wrap: balance;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .back.compacta .kicker {
    display: none;
  }

  .back.compacta header h1 {
    font-size: 14px;
    -webkit-line-clamp: 1;
    line-clamp: 1;
  }

  .back :global(.tablero) {
    flex: 1;
    min-height: 0;
  }

  .acciones {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 10px var(--pad) var(--pad);
    margin-top: 10px;
    box-shadow: inset 0 1px 0 var(--rb-hairline);
  }

  /* Las dos líneas comparten celda: el aviso entra sin mover el pie. */
  .estado {
    display: grid;
    min-width: 0;
    margin: 0;
    font-size: 11.5px;
    color: var(--rb-faint);
  }

  .estado > span {
    grid-area: 1 / 1;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    transition: opacity var(--duration-fast) var(--ease-smooth-out);
  }

  .estado > span.apagada {
    opacity: 0;
  }

  .guardado {
    opacity: 0;
    color: var(--rb-ok);
  }

  .guardado.visible {
    opacity: 1;
  }

  .back.compacta .estado {
    display: none;
  }
</style>
