<script lang="ts">
  /**
   * Tablero del reverso: un papel centrado, zoom, y objetos que se mueven.
   *
   * La tinta vive en un SVG a tamaño del papel. En modo lápiz captura el
   * puntero; en el resto se deja clicar lo que hay encima.
   */
  import { onMount } from "svelte";
  import { t } from "$domain/i18n.svelte";
  import Icon from "$ui/Icon.svelte";
  import {
    ListChecks,
    Minus,
    MousePointer2,
    Pencil,
    Plus,
    Trash2,
    Type,
  } from "$lib/icons";
  import { listClipboardHistory } from "$ipc/clipboard";
  import type { CheckItem, ClipboardItem, NoteBlock } from "$core/types";
  import {
    importWindowFlipImage,
    pasteWindowFlipImage,
    windowFlipAssetSrc,
    windowFlipPreviewSrc,
  } from "$ipc/windowFlip";
  import {
    altoPapel,
    centrar,
    clampZoom,
    CLIP_MIME,
    type Herramienta,
    marcoDe,
    PAPEL_ANCHO,
    puntoEnPapel,
    tamanoImagen,
  } from "./flipLayout";

  let {
    bloques = $bindable(),
    cajonAbierto = $bindable(false),
    assetsDir = "",
    compacta = false,
    onpersist,
  }: {
    bloques: NoteBlock[];
    cajonAbierto?: boolean;
    assetsDir?: string;
    compacta?: boolean;
    onpersist: () => void;
  } = $props();

  const LAPICES = [
    "#e5483f",
    "#d6b48a",
    "#3f7355",
    "#526d83",
    "#946718",
    "#7a5ea8",
    "#1c1917",
  ];

  let herramienta = $state<Herramienta>("select");
  let tinta = $state(LAPICES[0]);
  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let seleccion = $state("");
  let portapapeles = $state<ClipboardItem[]>([]);
  let papelEl: HTMLDivElement | undefined;
  let trazoVivo = $state<[number, number][] | null>(null);

  type Arrastre =
    | { tipo: "mover" | "se"; id: string; ox: number; oy: number; ow: number; oh: number; px: number; py: number }
    | { tipo: "pan"; px: number; py: number; ox: number; oy: number }
    | { tipo: "trazo" };

  let arrastre: Arrastre | null = null;

  const objetos = $derived(bloques.filter((b) => b.kind !== "ink"));
  const trazos = $derived(bloques.find((b) => b.kind === "ink")?.strokes ?? []);
  const alto = $derived(altoPapel(bloques));
  const vacio = $derived(objetos.length === 0 && trazos.length === 0);

  const herramientas = $derived(
    [
      { id: "select" as const, icon: MousePointer2, label: t("overlay.windowFlip.toolSelect") },
      { id: "draw" as const, icon: Pencil, label: t("overlay.windowFlip.toolDraw") },
      { id: "text" as const, icon: Type, label: t("overlay.windowFlip.toolText") },
      { id: "check" as const, icon: ListChecks, label: t("overlay.windowFlip.toolCheck") },
    ],
  );

  $effect(() => {
    if (!cajonAbierto) return;
    void listClipboardHistory()
      .then((items) => {
        portapapeles = items;
      })
      .catch(() => {
        portapapeles = [];
      });
  });

  function nuevoId(): string {
    return crypto.randomUUID();
  }

  function tocar() {
    onpersist();
  }

  function papelRect(): DOMRect | null {
    return papelEl?.getBoundingClientRect() ?? null;
  }

  function enPapel(clientX: number, clientY: number): { x: number; y: number } {
    const r = papelRect();
    if (!r) return { x: PAPEL_ANCHO / 2, y: 80 };
    return puntoEnPapel(r, clientX, clientY, PAPEL_ANCHO);
  }

  function alFrente(id: string) {
    const i = bloques.findIndex((b) => b.id === id);
    if (i < 0 || i === bloques.length - 1) return;
    const copia = [...bloques];
    const [item] = copia.splice(i, 1);
    if (!item) return;
    copia.push(item);
    bloques = copia;
  }

  export function soltarSeleccion(): boolean {
    if (!seleccion && herramienta === "select") return false;
    if (!seleccion) {
      herramienta = "select";
      return true;
    }
    seleccion = "";
    return true;
  }

  function elegir(id: string) {
    seleccion = id;
    alFrente(id);
  }

  function quitar(id: string) {
    bloques = bloques.filter((b) => b.id !== id);
    if (seleccion === id) seleccion = "";
    tocar();
  }

  function asegurarTinta(): NoteBlock {
    const hay = bloques.find((b) => b.kind === "ink");
    if (hay) return hay;
    const capa: NoteBlock = {
      kind: "ink",
      id: nuevoId(),
      strokes: [],
      height: 0,
      x: 0,
      y: 0,
      w: PAPEL_ANCHO,
      h: alto,
    };
    bloques = [...bloques, capa];
    return capa;
  }

  function poner(bloque: NoteBlock) {
    bloques = [...bloques, bloque];
    seleccion = bloque.id;
    tocar();
  }

  function alClicPapel(event: PointerEvent) {
    if (event.button !== 0) return;
    const dest = event.target as HTMLElement;
    if (dest !== event.currentTarget && !dest.classList.contains("tinta")) return;
    const p = enPapel(event.clientX, event.clientY);
    if (herramienta === "select") {
      seleccion = "";
      return;
    }
    if (herramienta === "text") {
      poner({
        kind: "text",
        id: nuevoId(),
        body: "",
        ...centrar(280, 96, p.x, p.y),
        w: 280,
        h: 96,
      });
      herramienta = "select";
      return;
    }
    if (herramienta === "check") {
      poner({
        kind: "check",
        id: nuevoId(),
        items: [{ id: nuevoId(), text: "", done: false }],
        ...centrar(260, 120, p.x, p.y),
        w: 260,
        h: 120,
      });
      herramienta = "select";
    }
  }

  function empezarTrazo(event: PointerEvent) {
    if (herramienta !== "draw" || event.button !== 0) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    const p = enPapel(event.clientX, event.clientY);
    trazoVivo = [[p.x, p.y]];
    arrastre = { tipo: "trazo" };
  }

  function extenderTrazo(event: PointerEvent) {
    if (!trazoVivo) return;
    const p = enPapel(event.clientX, event.clientY);
    const last = trazoVivo[trazoVivo.length - 1];
    if (last && Math.hypot(p.x - last[0], p.y - last[1]) < 1.2) return;
    trazoVivo = [...trazoVivo, [p.x, p.y]];
  }

  function cerrarTrazo() {
    if (!trazoVivo || trazoVivo.length < 2) {
      trazoVivo = null;
      arrastre = null;
      return;
    }
    const capa = asegurarTinta();
    if (capa.kind !== "ink") return;
    capa.strokes = [...capa.strokes, { color: tinta, width: 2.6, points: trazoVivo }];
    capa.h = altoPapel(bloques);
    trazoVivo = null;
    arrastre = null;
    tocar();
  }

  function empezarMover(event: PointerEvent, id: string) {
    if (herramienta !== "select" || event.button !== 0) return;
    const dest = event.target as HTMLElement;
    if (dest.closest("textarea, input, button, .asa")) return;
    event.preventDefault();
    elegir(id);
    const bloque = bloques.find((b) => b.id === id);
    if (!bloque) return;
    const m = marcoDe(bloque);
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    arrastre = {
      tipo: "mover",
      id,
      ox: m.x,
      oy: m.y,
      ow: m.w,
      oh: m.h,
      px: event.clientX,
      py: event.clientY,
    };
  }

  function empezarResize(event: PointerEvent, id: string) {
    if (event.button !== 0) return;
    event.stopPropagation();
    event.preventDefault();
    elegir(id);
    const bloque = bloques.find((b) => b.id === id);
    if (!bloque) return;
    const m = marcoDe(bloque);
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    arrastre = {
      tipo: "se",
      id,
      ox: m.x,
      oy: m.y,
      ow: m.w,
      oh: m.h,
      px: event.clientX,
      py: event.clientY,
    };
  }

  function empezarPan(event: PointerEvent) {
    if (event.button !== 0 || herramienta === "draw") return;
    if (event.target !== event.currentTarget) return;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    arrastre = { tipo: "pan", px: event.clientX, py: event.clientY, ox: panX, oy: panY };
  }

  function alMover(event: PointerEvent) {
    const drag = arrastre;
    if (!drag) return;
    if (drag.tipo === "trazo") {
      extenderTrazo(event);
      return;
    }
    if (drag.tipo === "pan") {
      panX = drag.ox + (event.clientX - drag.px);
      panY = drag.oy + (event.clientY - drag.py);
      return;
    }
    const r = papelRect();
    if (!r) return;
    const unidad = r.width / PAPEL_ANCHO;
    const dx = (event.clientX - drag.px) / unidad;
    const dy = (event.clientY - drag.py) / unidad;
    const bloque = bloques.find((b) => b.id === drag.id);
    if (!bloque || bloque.kind === "ink") return;
    if (drag.tipo === "mover") {
      bloque.x = drag.ox + dx;
      bloque.y = drag.oy + dy;
      return;
    }
    let w = Math.max(80, drag.ow + dx);
    let h = Math.max(48, drag.oh + dy);
    if (bloque.kind === "image") {
      const ratio = drag.oh / Math.max(1, drag.ow);
      h = Math.max(48, w * ratio);
    }
    bloque.w = w;
    bloque.h = h;
  }

  function alSoltarPuntero() {
    if (arrastre?.tipo === "trazo") {
      cerrarTrazo();
      return;
    }
    if (arrastre?.tipo === "mover" || arrastre?.tipo === "se") tocar();
    arrastre = null;
  }

  function alRueda(event: WheelEvent) {
    if (!(event.ctrlKey || event.metaKey)) return;
    event.preventDefault();
    zoom = clampZoom(zoom * (event.deltaY > 0 ? 0.92 : 1.08));
  }

  function puntosSvg(points: [number, number][]): string {
    return points.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
  }

  function escribirTexto(id: string, body: string) {
    const bloque = bloques.find((b) => b.id === id);
    if (bloque?.kind === "text") {
      bloque.body = body;
      tocar();
    }
  }

  function escribirCheck(id: string, itemId: string, patch: Partial<CheckItem>) {
    const bloque = bloques.find((b) => b.id === id);
    if (bloque?.kind !== "check") return;
    const item = bloque.items.find((i) => i.id === itemId);
    if (!item) return;
    Object.assign(item, patch);
    tocar();
  }

  function sumarCheck(id: string) {
    const bloque = bloques.find((b) => b.id === id);
    if (bloque?.kind !== "check") return;
    bloque.items = [...bloque.items, { id: nuevoId(), text: "", done: false }];
    bloque.h = Math.max(bloque.h ?? 0, 48 + bloque.items.length * 28);
    tocar();
  }

  function insertarTexto(texto: string, x: number, y: number) {
    const w = 280;
    const h = 96;
    poner({ kind: "text", id: nuevoId(), body: texto, ...centrar(w, h, x, y), w, h });
  }

  function insertarImagen(
    datos: { asset: string; width: number; height: number },
    x: number,
    y: number,
  ) {
    const { w, h } = tamanoImagen(datos.width, datos.height);
    poner({
      kind: "image",
      id: nuevoId(),
      asset: datos.asset,
      width: datos.width,
      height: datos.height,
      ...centrar(w, h, x, y),
      w,
      h,
    });
  }

  export async function insertarDelPortapapeles(item: ClipboardItem, x?: number, y?: number) {
    const cx = x ?? PAPEL_ANCHO / 2;
    const cy = y ?? 120;
    if (item.kind === "image" && item.imagePath) {
      try {
        insertarImagen(await importWindowFlipImage(item.imagePath), cx, cy);
      } catch {
        // El historial puede haber limpiado el archivo.
      }
      return;
    }
    if (item.text) insertarTexto(item.text, cx, cy);
  }

  function alArrancarClip(event: DragEvent, item: ClipboardItem) {
    event.dataTransfer?.setData(CLIP_MIME, item.id);
    if (item.text) event.dataTransfer?.setData("text/plain", item.text);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "copy";
  }

  function alArrastrarSobre(event: DragEvent) {
    if (!event.dataTransfer?.types.includes(CLIP_MIME)) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "copy";
  }

  async function alSoltarClip(event: DragEvent) {
    const id = event.dataTransfer?.getData(CLIP_MIME);
    if (!id) return;
    event.preventDefault();
    const item = portapapeles.find((i) => i.id === id);
    if (!item) return;
    const p = enPapel(event.clientX, event.clientY);
    await insertarDelPortapapeles(item, p.x, p.y);
  }

  async function alPegar(event: ClipboardEvent) {
    const tag = (event.target as HTMLElement)?.tagName;
    if (tag === "TEXTAREA" || tag === "INPUT") {
      const tieneImagen = Array.from(event.clipboardData?.items ?? []).some((item) =>
        item.type.startsWith("image/"),
      );
      if (!tieneImagen) return;
    }
    const tieneImagen = Array.from(event.clipboardData?.items ?? []).some((item) =>
      item.type.startsWith("image/"),
    );
    const p = { x: PAPEL_ANCHO / 2, y: 140 };
    if (tieneImagen) {
      event.preventDefault();
      try {
        insertarImagen(await pasteWindowFlipImage(), p.x, p.y);
      } catch {
        return;
      }
      return;
    }
    const texto = event.clipboardData?.getData("text/plain")?.trim();
    if (!texto || tag === "TEXTAREA" || tag === "INPUT") return;
    event.preventDefault();
    insertarTexto(texto, p.x, p.y);
  }

  onMount(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && (seleccion || herramienta !== "select")) {
        event.stopImmediatePropagation();
        seleccion = "";
        herramienta = "select";
        return;
      }
      const tag = (event.target as HTMLElement)?.tagName;
      if (tag === "TEXTAREA" || tag === "INPUT") return;
      if ((event.key === "Delete" || event.key === "Backspace") && seleccion) {
        event.preventDefault();
        quitar(seleccion);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
</script>

<div class="tablero">
  <div class="barra">
    <div class="grupo">
      {#each herramientas as item (item.id)}
        <button
          type="button"
          class="rb-btn rb-btn-ghost ico"
          class:activa={herramienta === item.id}
          title={item.label}
          aria-label={item.label}
          aria-pressed={herramienta === item.id}
          onclick={() => (herramienta = item.id)}
        >
          <Icon icon={item.icon} size={15} />
          {#if !compacta}<span>{item.label}</span>{/if}
        </button>
      {/each}
    </div>
    <div class="grupo">
      <button
        type="button"
        class="rb-btn rb-btn-ghost ico"
        title={t("overlay.windowFlip.zoomOut")}
        aria-label={t("overlay.windowFlip.zoomOut")}
        onclick={() => (zoom = clampZoom(zoom / 1.15))}
      >
        <Icon icon={Minus} size={15} />
      </button>
      <button
        type="button"
        class="rb-btn rb-btn-ghost zoom"
        title={t("overlay.windowFlip.zoomReset")}
        aria-label={t("overlay.windowFlip.zoomReset")}
        onclick={() => {
          zoom = 1;
          panX = 0;
          panY = 0;
        }}
      >
        {Math.round(zoom * 100)}%
      </button>
      <button
        type="button"
        class="rb-btn rb-btn-ghost ico"
        title={t("overlay.windowFlip.zoomIn")}
        aria-label={t("overlay.windowFlip.zoomIn")}
        onclick={() => (zoom = clampZoom(zoom * 1.15))}
      >
        <Icon icon={Plus} size={15} />
      </button>
      {#if seleccion}
        <button
          type="button"
          class="rb-btn rb-btn-ghost ico"
          title={t("overlay.windowFlip.removeBlock")}
          aria-label={t("overlay.windowFlip.removeBlock")}
          onclick={() => quitar(seleccion)}
        >
          <Icon icon={Trash2} size={15} />
        </button>
      {/if}
    </div>
  </div>

  <div class="cuerpo">
    <div
      class="vista"
      class:lapiz={herramienta === "draw"}
      role="application"
      aria-label={t("overlay.windowFlip.board")}
      onwheel={alRueda}
      onpointerdown={empezarPan}
      onpointermove={alMover}
      onpointerup={alSoltarPuntero}
      onpointercancel={alSoltarPuntero}
      onpaste={alPegar}
      ondragover={alArrastrarSobre}
      ondrop={(event) => void alSoltarClip(event)}
    >
      <div
        class="escena"
        style:transform={`translate(${panX}px, ${panY}px) scale(${zoom})`}
      >
        <div
          bind:this={papelEl}
          class="papel"
          role="group"
          aria-label={t("overlay.windowFlip.board")}
          style:width={`${PAPEL_ANCHO}px`}
          style:height={`${alto}px`}
          onpointerdown={alClicPapel}
        >
          {#if vacio}
            <p class="vacio">{t("overlay.windowFlip.boardEmpty")}</p>
          {/if}

          <svg
            class="tinta"
            class:captura={herramienta === "draw"}
            role="img"
            aria-hidden="true"
            viewBox={`0 0 ${PAPEL_ANCHO} ${alto}`}
            onpointerdown={empezarTrazo}
            onpointermove={alMover}
            onpointerup={alSoltarPuntero}
            onpointercancel={alSoltarPuntero}
          >
            {#each trazos as trazo, i (i)}
              <polyline
                fill="none"
                stroke={trazo.color}
                stroke-width={trazo.width}
                stroke-linecap="round"
                stroke-linejoin="round"
                points={puntosSvg(trazo.points)}
              />
            {/each}
            {#if trazoVivo}
              <polyline
                fill="none"
                stroke={tinta}
                stroke-width="2.6"
                stroke-linecap="round"
                stroke-linejoin="round"
                points={puntosSvg(trazoVivo)}
              />
            {/if}
          </svg>

          {#each objetos as bloque (bloque.id)}
            {@const m = marcoDe(bloque)}
            <div
              class="objeto"
              class:seleccionado={seleccion === bloque.id}
              class:lista={bloque.kind === "check"}
              role="group"
              style:left={`${m.x}px`}
              style:top={`${m.y}px`}
              style:width={`${m.w}px`}
              style:height={`${m.h}px`}
              onpointerdown={(event) => empezarMover(event, bloque.id)}
              onpointermove={alMover}
              onpointerup={alSoltarPuntero}
              onpointercancel={alSoltarPuntero}
            >
              {#if bloque.kind === "text"}
                <textarea
                  value={bloque.body}
                  placeholder={t("overlay.windowFlip.placeholder")}
                  spellcheck="false"
                  onfocus={() => elegir(bloque.id)}
                  oninput={(event) => escribirTexto(bloque.id, event.currentTarget.value)}
                ></textarea>
              {:else if bloque.kind === "image"}
                <img
                  src={windowFlipAssetSrc(assetsDir, bloque.asset)}
                  alt=""
                  draggable="false"
                />
              {:else if bloque.kind === "check"}
                <ul>
                  {#each bloque.items as item (item.id)}
                    <li>
                      <input
                        type="checkbox"
                        checked={item.done}
                        aria-label={t("overlay.windowFlip.checkItem")}
                        onchange={(event) =>
                          escribirCheck(bloque.id, item.id, {
                            done: event.currentTarget.checked,
                          })}
                      />
                      <input
                        type="text"
                        value={item.text}
                        placeholder={t("overlay.windowFlip.checkItem")}
                        onfocus={() => elegir(bloque.id)}
                        oninput={(event) =>
                          escribirCheck(bloque.id, item.id, {
                            text: event.currentTarget.value,
                          })}
                      />
                    </li>
                  {/each}
                </ul>
                <button type="button" class="mas" onclick={() => sumarCheck(bloque.id)}>
                  {t("overlay.windowFlip.addCheck")}
                </button>
              {/if}
              {#if seleccion === bloque.id}
                <button
                  type="button"
                  class="asa"
                  aria-label={t("overlay.windowFlip.resize")}
                  onpointerdown={(event) => empezarResize(event, bloque.id)}
                  onpointermove={alMover}
                  onpointerup={alSoltarPuntero}
                  onpointercancel={alSoltarPuntero}
                ></button>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    </div>

    <aside class="cajon" class:abierto={cajonAbierto} inert={!cajonAbierto} aria-hidden={!cajonAbierto}>
      <p class="titulo">{t("overlay.windowFlip.pens")}</p>
      <div class="lapices">
        {#each LAPICES as lapiz (lapiz)}
          <button
            type="button"
            class="lapiz"
            class:elegido={tinta === lapiz}
            style:background={lapiz}
            title={lapiz}
            aria-label={lapiz}
            aria-pressed={tinta === lapiz}
            onclick={() => {
              tinta = lapiz;
              herramienta = "draw";
            }}
          ></button>
        {/each}
      </div>

      <p class="titulo">{t("overlay.windowFlip.clipboard")}</p>
      <div class="pila">
        {#each portapapeles.slice(0, 40) as item (item.id)}
          <button
            type="button"
            class="recorte"
            class:imagen={item.kind === "image"}
            title={item.preview}
            draggable="true"
            ondragstart={(event) => alArrancarClip(event, item)}
            onclick={() => void insertarDelPortapapeles(item)}
          >
            {#if item.kind === "image" && item.imagePath}
              <img src={windowFlipPreviewSrc(item.imagePath)} alt="" />
            {:else}
              <span>{item.text || item.preview}</span>
            {/if}
          </button>
        {:else}
          <p class="hueco">{t("overlay.windowFlip.clipboardEmpty")}</p>
        {/each}
      </div>
    </aside>
  </div>
</div>

<style>
  .tablero {
    display: flex;
    flex: 1;
    min-height: 0;
    flex-direction: column;
  }

  .barra {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 0 var(--pad) 8px;
  }

  .grupo {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .ico {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
  }

  .ico.activa {
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
  }

  .zoom {
    min-width: 3.2rem;
    font-variant-numeric: tabular-nums;
  }

  .cuerpo {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .vista {
    position: relative;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    background: var(--rb-surface-2);
    cursor: grab;
    touch-action: none;
  }

  .vista.lapiz {
    cursor: crosshair;
  }

  .escena {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    transform-origin: center center;
    pointer-events: none;
  }

  .papel {
    position: relative;
    pointer-events: auto;
    background: var(--rb-surface);
    box-shadow:
      0 1px 0 var(--rb-hairline),
      0 18px 40px rgb(0 0 0 / 18%);
    cursor: default;
  }

  .vista.lapiz .papel {
    cursor: crosshair;
  }

  .vacio {
    position: absolute;
    inset: 28% 18% auto;
    margin: 0;
    color: var(--rb-faint);
    font-size: 13px;
    line-height: 1.45;
    text-align: center;
    pointer-events: none;
  }

  .tinta {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    overflow: visible;
  }

  .tinta.captura {
    pointer-events: auto;
    touch-action: none;
  }

  .objeto {
    position: absolute;
    box-sizing: border-box;
    overflow: hidden;
    border-radius: var(--rb-radius-xs);
    background: var(--rb-surface);
    cursor: grab;
    outline: 1px solid transparent;
  }

  .objeto.seleccionado {
    outline: 1.5px solid color-mix(in sRGB, var(--rb-text) 45%, transparent);
    z-index: 2;
  }

  .objeto textarea {
    display: block;
    width: 100%;
    height: 100%;
    padding: 10px 12px;
    resize: none;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 13.5px;
    line-height: 1.5;
    outline: none;
    box-sizing: border-box;
  }

  .objeto img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: fill;
    pointer-events: none;
  }

  .objeto.lista {
    display: flex;
    flex-direction: column;
    padding: 8px 10px 6px;
  }

  .objeto ul {
    margin: 0;
    padding: 0;
    list-style: none;
    overflow: auto;
    flex: 1;
  }

  .objeto li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 26px;
  }

  .objeto li input[type="text"] {
    flex: 1;
    min-width: 0;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 13px;
    outline: none;
  }

  .objeto li input[type="checkbox"]:checked + input {
    color: var(--rb-faint);
    text-decoration: line-through;
  }

  .mas {
    align-self: start;
    margin-top: 4px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--rb-faint);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }

  .asa {
    position: absolute;
    right: 3px;
    bottom: 3px;
    width: 12px;
    height: 12px;
    padding: 0;
    border: 0;
    border-radius: 2px;
    background: var(--rb-text);
    cursor: nwse-resize;
  }

  .cajon {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 0;
    padding: 0;
    overflow: hidden auto;
    border-left: 0 solid var(--rb-hairline);
    transition:
      width var(--duration-slow) var(--ease-smooth-out),
      padding var(--duration-slow) var(--ease-smooth-out);
  }

  .cajon.abierto {
    width: 168px;
    padding: 0 var(--pad) 0 10px;
    border-left-width: 1px;
  }

  .cajon .titulo {
    position: sticky;
    top: 0;
    margin: 0;
    padding: 2px 0;
    background: var(--rb-surface);
    color: var(--rb-faint);
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .lapices {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .lapiz {
    width: 20px;
    height: 20px;
    border: 0;
    border-radius: 999px;
    cursor: pointer;
    box-shadow: inset 0 0 0 1px rgb(128 128 128 / 35%);
    transition: transform var(--duration-fast) var(--ease-smooth-out);
  }

  .lapiz.elegido {
    transform: scale(1.14);
    box-shadow: inset 0 0 0 2px var(--rb-text);
  }

  .lapiz:hover {
    transform: scale(1.12);
  }

  .pila {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-bottom: 8px;
  }

  .recorte {
    display: block;
    width: 100%;
    padding: 5px 7px;
    border: 0;
    border-radius: var(--rb-radius-xs);
    background: var(--rb-surface-2);
    color: inherit;
    font: inherit;
    font-size: 11.5px;
    text-align: left;
    cursor: grab;
    transition: background var(--duration-fast) var(--ease-smooth-out);
  }

  .recorte:hover {
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
  }

  .recorte span {
    display: -webkit-box;
    overflow: hidden;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  .recorte.imagen {
    padding: 4px;
  }

  .recorte img {
    display: block;
    width: 100%;
    height: auto;
    max-height: 70px;
    border-radius: 3px;
    object-fit: cover;
  }

  .hueco {
    margin: 0;
    color: var(--rb-faint);
    font-size: 11.5px;
  }

  :global(.back.compacta) .cajon.abierto {
    width: 132px;
  }
</style>
