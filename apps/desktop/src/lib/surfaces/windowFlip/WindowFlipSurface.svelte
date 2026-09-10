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
  import { AppWindow, Check } from "$lib/icons";
  import { emerge } from "$lib/motion";
  import Icon from "$ui/Icon.svelte";
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
  /** Atajo vigente para el hint del pie: se lee de la config al montar. */
  /** True mientras se exporta: el diálogo nativo no debe voltear el reverso. */
  let ocupado = $state(false);
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
  const altoTarjeta = $derived((view?.cardHeight ?? 1) * altoVentana);

  /**
   * Tres niveles de espacio, de más a menos.
   *
   * La barra tiene trece botones y, con etiquetas en los dos grupos, necesita
   * unos 1300px de ancho. Con sólo las herramientas nombradas baja a ~950, y con
   * todo en icono a ~470. De ahí salen los cortes: en 855 —una ventana chica
   * normal— la barra partida en dos filas se comía el bloc.
   *
   * El pie es lo último que sobra: el atajo del encabezado está en Ajustes.
   */
  const compacta = $derived(anchoTarjeta < 1000 || altoTarjeta < 430);
  const accionesSoloIcono = $derived(anchoTarjeta < 1400 || compacta);
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
    if (ocupado || closing) return;
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
        if (!ocupado) void beginClose();
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
        <FlipBoard
          bind:bloques
          bind:cajonAbierto
          assetsDir={view?.assetsDir ?? ""}
          {compacta}
          {accionesSoloIcono}
          notaKey={view?.key ?? ""}
          nombreArchivo={view?.title || view?.exe || "tablero"}
          onpersist={persist}
          onclose={() => void beginClose()}
          onocupado={(v) => (ocupado = v)}
        >
          {#snippet encabezado()}
            <div class="titulos">
              {#if view?.icon}
                <img class="app-icon" src={view.icon} alt="" width="20" height="20" />
              {:else}
                <span class="app-icon hueco" aria-hidden="true">
                  <Icon icon={AppWindow} size={13} />
                </span>
              {/if}
              <p class="kicker">
                {t("overlay.windowFlip.kicker", { exe: view?.exe ?? "" })}
              </p>
              <h1>{view?.title || t("overlay.windowFlip.untitled")}</h1>
            </div>
          {/snippet}
        </FlipBoard>
        {#if guardado}
          <p class="guardado-flote" aria-live="polite" transition:emerge>
            <Icon icon={Check} size={12} />
            {t("overlay.windowFlip.saved")}
          </p>
        {/if}
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

  /*
   * El aviso de guardado flota sobre el tablero en vez de ocupar una fila:
   * el pie se comía 36px para decir una frase que se lee una vez.
   */
  .guardado-flote {
    position: absolute;
    bottom: 14px;
    left: var(--pad);
    z-index: 4;
    display: flex;
    align-items: center;
    gap: 4px;
    margin: 0;
    padding: 4px 9px;
    border-radius: 999px;
    background: var(--rb-surface);
    box-shadow: 0 2px 10px rgb(0 0 0 / 20%);
    color: var(--rb-ok);
    font-size: 11.5px;
    pointer-events: none;
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
    background: var(--rb-bg1);
    color: var(--rb-text);
    transform: rotateY(180deg) translateZ(1px);
    box-sizing: border-box;
  }

  .back.compacta {
    --pad: 12px;
  }

  .back.compacta :global(.barra) {
    gap: 6px;
  }

  .back.compacta :global(.barra .rb-btn) {
    padding: 0.3rem 0.55rem;
    font-size: 0.78rem;
  }

  .app-icon {
    display: grid;
    width: 20px;
    height: 20px;
    flex: none;
    place-items: center;
    overflow: hidden;
    border-radius: 6px;
    background: var(--rb-surface-elevated);
    outline: 1px solid rgb(0 0 0 / 12%);
    outline-offset: -1px;
    object-fit: cover;
  }

  :global(:root[data-theme-base="dark"]) .app-icon {
    outline: 1px solid rgb(255 255 255 / 14%);
    outline-offset: -1px;
  }

  .app-icon.hueco {
    color: var(--rb-text);
  }

  /* Vive en la fila de la barra: el ancho que sobra se lo come el título. */
  .titulos {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 6px;
  }

  .kicker {
    margin: 0;
    font-size: 11px;
    font-weight: 500;
    color: color-mix(in sRGB, var(--rb-text) 72%, transparent);
  }

  .titulos h1 {
    min-width: 0;
    margin: 0;
    font-family: var(--rb-display);
    font-size: 13px;
    font-weight: 600;
    line-height: 1.3;

    /* El título es de la ventana ajena: puede ser una frase entera. */
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .back.compacta .kicker {
    display: none;
  }

  .back :global(.tablero) {
    flex: 1;
    min-height: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    .guardado-flote {
      animation: none;
    }
  }
</style>
