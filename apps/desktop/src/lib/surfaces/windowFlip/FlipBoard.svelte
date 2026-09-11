<script lang="ts">
  /**
   * Tablero del reverso: un papel centrado, zoom, y objetos que se mueven.
   *
   * La tinta vive en un SVG a tamaño del papel. En modo lápiz captura el
   * puntero; en el resto se deja clicar lo que hay encima.
   */
  import { onMount, tick, untrack, type Snippet as SvelteSnippet } from "svelte";
  import { t } from "$domain/i18n.svelte";
  import {
    duration as tokenMs,
    emerge,
    prefersReducedMotion,
    tabPanel,
  } from "$lib/motion";
  import Icon from "$ui/Icon.svelte";
  import {
    ListChecks,
    Minus,
    MousePointer2,
    Pencil,
    Highlighter,
    Plus,
    Redo2,
    Trash2,
    Type,
    Undo2,
    Eraser,
    X,
    GripVertical,
    Download,
    ArrowLeft,
    Camera,
    FileImage,
    FileText,
    FileType,
    LoaderCircle,
    PanelRightClose,
    PanelRightOpen,
    Presentation,
  } from "$lib/icons";
  import type { IconNode } from "morphicons/svelte";
  import { listClipboardHistory } from "$ipc/clipboard";
  import { listRecentCaptures, captureSrc } from "$ipc/captures";
  import { listRecordings } from "$ipc/recordings";
  import { listSnippets } from "$ipc/snippets";
  import { getSummary } from "$ipc/summaries";
  import type {
    CaptureItem,
    CheckItem,
    ClipboardItem,
    NoteBlock,
    Recording,
    Snippet,
  } from "$core/types";
  import {
    importWindowFlipImage,
    pasteWindowFlipImage,
    exportWindowFlip,
    windowFlipAssetData,
    windowFlipAssetSrc,
    windowFlipPreviewSrc,
  } from "$ipc/windowFlip";
  import { pickBoardExportPath, safeFileName } from "$ipc/dialogs";
  import {
    ajustarFuente,
    altoEnvolvente,
    anchoParaBloques,
    bloqueEnPagina,
    cantidadPaginas,
    BORRAR_RADIO,
    borrarLineaCercana,
    borrarPuntos,
    clampZoom,
    conAlfa,
    FLIP_MIME,
    type FlipFuente,
    type Herramienta,
    leerPayloadFlip,
    lineasPagina,
    marcoDe,
    PAGINA_H,
    PAGINA_W,
    payloadFlip,
    puntoEnPapel,
    TABLERO_MAX,
    tamanoImagen,
    TEXTO_ALTO_MIN,
    TEXTO_FUENTE,
    TEXTO_H,
    TEXTO_W,
    FUENTE_TABLERO,
  } from "./flipLayout";
  import { bloquesDeResumen, etiquetaResumen } from "./flipSummary";
  import FlipInsertPreview, { type FlipPreview } from "./FlipInsertPreview.svelte";
  import { armarExport, FORMATOS_TABLERO, type FormatoTablero } from "./flipExport";

  let {
    bloques = $bindable(),
    cajonAbierto = $bindable(false),
    assetsDir = "",
    compacta = false,
    accionesSoloIcono = false,
    encabezado,
    notaKey = "",
    onpersist,
    onclose,
    nombreArchivo = "tablero",
    onocupado,
  }: {
    bloques: NoteBlock[];
    cajonAbierto?: boolean;
    assetsDir?: string;
    compacta?: boolean;
    /** Sin lugar para el texto de las acciones: quedan sólo los iconos. */
    accionesSoloIcono?: boolean;
    /** Icono y título de la ventana: comparten la fila de la barra. */
    encabezado?: SvelteSnippet;
    /** `exe|título`: cada reverso es independiente del resto. */
    notaKey?: string;
    onpersist: () => void;
    onclose?: () => void;
    nombreArchivo?: string;
    /** El reverso no se cierra mientras el diálogo nativo tiene el foco. */
    onocupado?: (v: boolean) => void;
  } = $props();

  const LAPICES = [
    { color: "#e5483f", name: "penRed" },
    { color: "#d9622b", name: "penOrange" },
    { color: "#d6b48a", name: "penSand" },
    { color: "#946718", name: "penGold" },
    { color: "#3f7355", name: "penGreen" },
    { color: "#2f8f83", name: "penTeal" },
    { color: "#526d83", name: "penBlue" },
    { color: "#7a5ea8", name: "penViolet" },
    { color: "#c14a7a", name: "penPink" },
    { color: "#1c1917", name: "penBlack" },
  ] as const;

  let herramienta = $state<Herramienta>("select");
  let tinta = $state<string>(LAPICES[0].color);
  const esResaltador = $derived(herramienta === "highlight");
  const trazoColor = $derived(esResaltador ? conAlfa(tinta) : tinta);
  const trazoAncho = $derived(esResaltador ? 12 : 2.6);
  const esTintaLibre = $derived(!LAPICES.some((l) => l.color === tinta));
  /** Paleta flotante de colores: segundo clic en el lápiz activo la abre. */
  let paletaAbierta = $state(false);
  let menuExport = $state(false);
  let exportando = $state(false);
  let avisoExport = $state("");

  const ETIQUETA_EXPORT: Record<FormatoTablero, string> = {
    png: "exportPng",
    jpeg: "exportJpeg",
    pdf: "exportPdf",
    docx: "exportDocx",
    pptx: "exportPptx",
  };

  /** Cada formato con su icono: imagen, foto, documento, texto y diapositivas. */
  const ICONO_EXPORT: Record<FormatoTablero, IconNode> = {
    png: FileImage,
    jpeg: Camera,
    pdf: FileType,
    docx: FileText,
    pptx: Presentation,
  };

  async function exportar(formato: FormatoTablero) {
    if (exportando) return;
    exportando = true;
    onocupado?.(true);
    menuExport = false;
    avisoExport = "";
    try {
      // Armar páginas ANTES del diálogo: el save nativo roba el foco.
      const { paginas, fallos } = await armarExport(
        bloques,
        papelW,
        "#f4f1ea",
        windowFlipAssetData,
        formato,
      );
      const multi = (formato === "png" || formato === "jpeg") && paginas.length > 1;
      const path = await pickBoardExportPath(
        safeFileName(nombreArchivo, "tablero"),
        multi ? "zip" : formato,
      );
      if (!path) return;
      await exportWindowFlip(formato, path, paginas, PAGINA_W, PAGINA_H);
      avisoExport = fallos.length
        ? t("overlay.windowFlip.exportMissing", { n: String(fallos.length) })
        : t("overlay.windowFlip.exportOk");
    } catch (err) {
      avisoExport = t("overlay.windowFlip.exportFail", {
        error: err instanceof Error ? err.message : String(err),
      });
    } finally {
      exportando = false;
      onocupado?.(false);
    }
  }

  function elegirHerramienta(id: Herramienta) {
    // Los lápices abren la paleta al primer clic; otro clic la alterna.
    if ((id === "draw" || id === "highlight") && herramienta === id) {
      paletaAbierta = !paletaAbierta;
      return;
    }
    herramienta = id;
    paletaAbierta = id === "draw" || id === "highlight";
  }

  function elegirColor(color: string) {
    tinta = color;
    paletaAbierta = false;
  }
  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let seleccion = $state("");
  let fuente = $state<FlipFuente>("clip");
  let portapapeles = $state.raw<ClipboardItem[]>([]);
  let textos = $state.raw<Snippet[]>([]);
  let capturas = $state.raw<CaptureItem[]>([]);
  let reuniones = $state.raw<Recording[]>([]);
  let preview = $state<FlipPreview | null>(null);
  let papelEl: HTMLDivElement | undefined;
  let vistaEl: HTMLDivElement | undefined;
  let herramientasEl: HTMLDivElement | undefined;
  let escenaEl: HTMLDivElement | undefined;
  /** La vista se está moviendo por un salto y no por la mano del usuario. */
  let animandoVista = $state(false);
  let timerVista: ReturnType<typeof setTimeout> | undefined;
  let vivo = $state(false);
  let pastillaX = $state(0);
  let pastillaW = $state(0);
  let pastillaLista = $state(false);
  let soltando = $state(false);

  let papelW = $state(PAGINA_W);
  /**
   * Tira horizontal de una sola fila: el alto y el origen van fijos y el
   * tablero solo crece a la derecha, una celda por clic.
   */
  const papelH = PAGINA_H;
  const origenX = 0;
  const origenY = 0;
  let trazoVivo = $state<[number, number][] | null>(null);

  type Arrastre =
    | {
        tipo: "mover" | "se";
        id: string;
        ox: number;
        oy: number;
        ow: number;
        oh: number;
        px: number;
        py: number;
        registrado: boolean;
      }
    | { tipo: "pan"; px: number; py: number; ox: number; oy: number }
    | { tipo: "trazo" }
    | { tipo: "borrar"; px: number; py: number; movido: boolean };

  let arrastre: Arrastre | null = null;

  // Historial de estados ANTES de cada mutación, de atrás para adelante.
  // Las entradas son copias profundas: los bloques mutan en el lugar y sin
  // esto una mutación posterior corrompería lo guardado.
  const HISTORIA_MAX = 50;
  let pasado = $state<NoteBlock[][]>([]);
  let futuro = $state<NoteBlock[][]>([]);
  /** Referencia del estado antes de editar con el teclado dentro de un bloque. */
  let sesionAntes: NoteBlock[] | null = null;

  const puedeDeshacer = $derived(pasado.length > 0);
  const puedeRehacer = $derived(futuro.length > 0);

  function clonar(lista: NoteBlock[]): NoteBlock[] {
    return $state.snapshot(lista);
  }

  function empujarAntes(previo: NoteBlock[]) {
    pasado = [...pasado, previo].slice(-HISTORIA_MAX);
    futuro = [];
  }

  /** Llamar justo antes de cada mutación del tablero. */
  function registrar() {
    empujarAntes(clonar(bloques));
  }

  function cancelarGestos() {
    arrastre = null;
    trazoVivo = null;
    sesionAntes = null;
  }

  function deshacer() {
    cancelarGestos();
    const previo = pasado[pasado.length - 1];
    if (!previo) return;
    pasado = pasado.slice(0, -1);
    futuro = [...futuro, clonar(bloques)];
    bloques = previo;
    seleccion = "";
  }

  function rehacer() {
    cancelarGestos();
    const siguiente = futuro[futuro.length - 1];
    if (!siguiente) return;
    futuro = futuro.slice(0, -1);
    pasado = [...pasado, clonar(bloques)];
    bloques = siguiente;
    seleccion = "";
  }

  const objetos = $derived(bloques.filter((b) => b.kind !== "ink"));
  const trazos = $derived(bloques.find((b) => b.kind === "ink")?.strokes ?? []);
  const vacio = $derived(objetos.length === 0 && trazos.length === 0 && !trazoVivo);

  const herramientas = $derived([
    {
      id: "select" as const,
      icon: MousePointer2,
      label: t("overlay.windowFlip.toolSelect"),
      key: "V",
    },
    {
      id: "draw" as const,
      icon: Pencil,
      label: t("overlay.windowFlip.toolDraw"),
      key: "P",
    },
    {
      id: "highlight" as const,
      icon: Highlighter,
      label: t("overlay.windowFlip.toolHighlighter"),
      key: "H",
    },
    {
      id: "eraser" as const,
      icon: Eraser,
      label: t("overlay.windowFlip.toolEraser"),
      key: "E",
    },
    {
      id: "text" as const,
      icon: Type,
      label: t("overlay.windowFlip.toolText"),
      key: "T",
    },
    {
      id: "check" as const,
      icon: ListChecks,
      label: t("overlay.windowFlip.toolCheck"),
      key: "L",
    },
  ]);

  // La paleta solo vive sobre los lápices: cualquier otro destino la cierra.
  $effect(() => {
    if (herramienta !== "draw" && herramienta !== "highlight") paletaAbierta = false;
  });

  // Cada reverso es independiente: al cambiar de nota se sueltan páginas,
  // historial, selección y vista. Sin esto, las páginas (y el undo) de una
  // ventana se filtraban a la siguiente. La tinta se conserva.
  // Solo notaKey dispara el reset: el resto va en untrack para que paginar
  // (que cambia papelW, leído por encuadrar) no lo reactive al instante.
  $effect(() => {
    void notaKey;
    untrack(() => {
      papelW = PAGINA_W;
      pasado = [];
      futuro = [];
      sesionAntes = null;
      seleccion = "";
      herramienta = "select";
      paletaAbierta = false;
      trazoVivo = null;
      arrastre = null;
      destello = null;
      vistaTocada = false;
      encuadrar();
    });
  });

  /**
   * Destello en el borde afectado al paginar: como la vista queda quieta a
   * propósito, sin esto parece que el [+] no hizo nada cuando la página
   * nueva cae fuera de lo visible.
   */
  let destello = $state<{ x: number; marca: number } | null>(null);
  let destelloTimer: ReturnType<typeof setTimeout> | undefined;

  /** Cuántas celdas hay: la tira dibuja una miniatura por cada una. */
  const paginasTotales = $derived(cantidadPaginas(papelW));

  /**
   * Qué celda está mirando el centro de la vista.
   *
   * Sale de la misma cuenta que el pan: el papel está centrado por el flex, así
   * que su coordenada en el centro de la vista es `papelW/2 - panX/zoom`.
   */
  const paginaActual = $derived(
    Math.max(
      0,
      Math.min(paginasTotales - 1, Math.floor((papelW / 2 - panX / zoom) / PAGINA_W)),
    ),
  );

  const tintaSuelta = $derived(
    bloques.find((b): b is Extract<NoteBlock, { kind: "ink" }> => b.kind === "ink") ??
      null,
  );

  /**
   * Lo mínimo para dibujar cada miniatura: cajas en % de la celda.
   *
   * Texto y listas van como barritas y no como texto: a 84px de ancho no se lee
   * nada, y lo que hace reconocible una página es la foto y el dibujo.
   */
  const miniaturas = $derived.by(() => {
    const tinta = tintaSuelta;
    return Array.from({ length: paginasTotales }, (_, indice) => {
      const piezas = bloques
        .filter((b) => b.kind !== "ink" && bloqueEnPagina(b, indice))
        .map((b) => {
          const m = marcoDe(b);
          const x0 = indice * PAGINA_W;
          const caja = {
            id: b.id,
            tipo: b.kind,
            x: ((m.x - x0) / PAGINA_W) * 100,
            y: (m.y / PAGINA_H) * 100,
            w: (m.w / PAGINA_W) * 100,
            h: (m.h / PAGINA_H) * 100,
          };
          if (b.kind === "check") {
            return {
              ...caja,
              filas: Math.min(4, b.items.length),
              asset: "",
              renglones: 0,
            };
          }
          if (b.kind === "image") {
            return { ...caja, filas: 0, asset: b.asset, renglones: 0 };
          }
          return {
            ...caja,
            filas: 0,
            asset: "",
            renglones: Math.min(4, Math.max(1, Math.round(m.h / 24))),
          };
        });
      const trazos =
        tinta && bloqueEnPagina(tinta, indice)
          ? tinta.strokes.map((trazo) => ({
              puntos: trazo.points.map(([x, y]) => `${x},${y}`).join(" "),
              color: trazo.color.length === 9 ? trazo.color.slice(0, 7) : trazo.color,
              ancho: trazo.width * 2,
            }))
          : [];
      return { indice, piezas, trazos };
    });
  });

  /**
   * Mueve la vista con transición.
   *
   * El navegador no anima un cambio que llega en el mismo recálculo de estilo que
   * la `transition`, así que se marca la clase, se fuerza un reflow leyendo
   * `offsetWidth` y recién ahí se mueve. Es el mismo truco que usa el repo para
   * volver a disparar una animación.
   *
   * Arrastrar y la rueda NO pasan por acá: ahí el movimiento es continuo y una
   * transición lo haría sentir pegajoso.
   */
  async function moverVista(mover: () => void) {
    if (prefersReducedMotion()) {
      mover();
      return;
    }
    animandoVista = true;
    await tick();
    void escenaEl?.offsetWidth;
    mover();
    if (timerVista) clearTimeout(timerVista);
    timerVista = setTimeout(
      () => {
        animandoVista = false;
      },
      tokenMs("--duration-fast", 125) + 40,
    );
  }

  /** Corta la animación: el usuario tomó el control. */
  function sinAnimacion() {
    if (timerVista) clearTimeout(timerVista);
    animandoVista = false;
  }

  /** Trae una celda al medio de la vista, sin alejar más de lo que ya está. */
  function irAPagina(indice: number) {
    vistaTocada = true;
    const vw = vistaEl?.clientWidth ?? 0;
    const vh = vistaEl?.clientHeight ?? 0;
    const entra = Math.min((vw - 24) / PAGINA_W, (vh - 24) / PAGINA_H);
    void moverVista(() => {
      if (entra > 0) zoom = clampZoom(Math.min(zoom, entra));
      // El papel está centrado por el flex: centrar la celda es correr el pan la
      // distancia entre su centro y el del papel.
      panX = zoom * (papelW / 2 - (indice + 0.5) * PAGINA_W);
      panY = 0;
    });
  }

  /** Agrega una celda y lleva la vista a ella: si no, el clic no se ve. */
  function agregarYVer() {
    agregarPagina();
    irAPagina(cantidadPaginas(papelW) - 1);
  }

  function marcarPagina(x: number) {
    if (destelloTimer) clearTimeout(destelloTimer);
    destello = { x, marca: Date.now() };
    const marca = destello.marca;
    destelloTimer = setTimeout(() => {
      if (destello?.marca === marca) destello = null;
    }, 1400);
  }

  /** Encuadra el tablero entero en la vista sin pasarse del 100%. */
  let vistaAncho = $state(0);
  let vistaAlto = $state(0);
  /** ¿El usuario movió o acercó la vista? Entonces el resize no la toca. */
  let vistaTocada = $state(false);

  /**
   * El alto de la vista cambia con la ventana, con el cajón y con la tira: sin
   * esto el encuadre quedaba calculado para un tamaño que ya no existe, y la
   * página terminaba recortada contra el borde.
   */
  $effect(() => {
    const el = vistaEl;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      vistaAncho = el.clientWidth;
      vistaAlto = el.clientHeight;
    });
    ro.observe(el);
    return () => ro.disconnect();
  });

  $effect(() => {
    // Se leen para que el effect dependa del tamaño de la vista.
    void vistaAncho;
    void vistaAlto;
    if (vistaAncho === 0 || vistaAlto === 0 || vistaTocada) return;
    untrack(() => encuadrar());
  });

  function encuadrar() {
    const vw = vistaEl?.clientWidth ?? 0;
    const vh = vistaEl?.clientHeight ?? 0;
    const z = Math.min(
      1,
      (vw - 24) / Math.max(1, papelW),
      (vh - 24) / Math.max(1, papelH),
    );
    zoom = z > 0 ? z : 1;
    panX = 0;
    panY = 0;
  }

  async function cargarFuentes() {
    const [clips, snips, caps, recs] = await Promise.all([
      listClipboardHistory().catch(() => [] as ClipboardItem[]),
      listSnippets().catch(() => [] as Snippet[]),
      listRecentCaptures().catch(() => [] as CaptureItem[]),
      listRecordings().catch(() => [] as Recording[]),
    ]);
    portapapeles = clips;
    textos = snips;
    capturas = caps;
    reuniones = recs.filter((r) => r.status === "summarized");
  }

  function alternarCajon() {
    cajonAbierto = !cajonAbierto;
    if (cajonAbierto) void cargarFuentes();
  }

  $effect(() => {
    void herramienta;
    void compacta;
    const root = herramientasEl;
    if (!root) return;
    const medir = () => {
      const activa = root.querySelector<HTMLElement>(":scope > .ico.activa");
      if (!activa) return;
      pastillaX = activa.offsetLeft;
      pastillaW = activa.offsetWidth;
    };
    medir();
    const ro = new ResizeObserver(medir);
    ro.observe(root);
    const wake = requestAnimationFrame(() => {
      pastillaLista = true;
    });
    return () => {
      cancelAnimationFrame(wake);
      ro.disconnect();
    };
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
    if (!r) return { x: origenX + papelW / 2, y: origenY + 80 };
    return puntoEnPapel(r, clientX, clientY, papelW, origenX, origenY);
  }

  function ubicar(
    w: number,
    h: number,
    x: number,
    y: number,
  ): { x: number; y: number } {
    return {
      x: Math.max(origenX + 8, Math.min(origenX + papelW - w - 8, x - w / 2)),
      y: Math.max(origenY + 8, Math.min(origenY + papelH - 24, y - h / 2)),
    };
  }

  /** Recorta un marco al papel: lo arrastrado no se pierde por el borde. */
  function limitar(
    x: number,
    y: number,
    w: number,
    h: number,
  ): { x: number; y: number } {
    return {
      x: Math.max(origenX, Math.min(Math.max(origenX, origenX + papelW - w), x)),
      y: Math.max(origenY, Math.min(Math.max(origenY, origenY + papelH - h), y)),
    };
  }

  /**
   * Agrega una celda a la derecha. El flex recentra el papel al crecer y
   * eso corre el contenido media página (en pantalla, o sea por el zoom):
   * se compensa con el pan —que va en píxeles de pantalla, sin escalar—
   * para que lo que se miraba quede clavado donde estaba. Las páginas
   * vacías viven en la sesión: lo que persiste es el contenido.
   */
  function agregarPagina() {
    const antes = papelW;
    papelW = Math.min(TABLERO_MAX, papelW + PAGINA_W);
    if (papelW === antes) return;
    panX += (PAGINA_W / 2) * zoom;
    marcarPagina(papelW - PAGINA_W);
  }

  const paginas = $derived(
    lineasPagina({ ox: origenX, oy: origenY, w: papelW, h: papelH }),
  );

  /**
   * La última celda no tiene nada: se puede quitar. Con contenido o siendo
   * la única, el [−] se deshabilita.
   */
  function franjaVacia(): boolean {
    if (papelW <= PAGINA_W) return false;
    const x0 = origenX + papelW - PAGINA_W;
    const x1 = origenX + papelW;
    const y0 = origenY;
    const y1 = origenY + papelH;
    for (const bloque of bloques) {
      if (bloque.kind === "ink") {
        for (const trazo of bloque.strokes) {
          for (const [px, py] of trazo.points) {
            if (px >= x0 && px < x1 && py >= y0 && py < y1) return false;
          }
        }
        continue;
      }
      const m = marcoDe(bloque);
      if (m.x < x1 && m.x + m.w > x0 && m.y < y1 && m.y + m.h > y0) return false;
    }
    return true;
  }

  function quitarPagina() {
    if (!franjaVacia()) return;
    // Al achicar, el recentrado también corre el contenido: misma
    // compensación en sentido contrario.
    papelW -= PAGINA_W;
    panX -= (PAGINA_W / 2) * zoom;
    marcarPagina(papelW);
  }

  /** Punto del papel que hoy está al centro de la vista: donde cae lo pegado. */
  function centroVisible(): { x: number; y: number } {
    const r = papelRect();
    const v = vistaEl?.getBoundingClientRect();
    if (!r || !v) return { x: origenX + papelW / 2, y: origenY + 120 };
    return puntoEnPapel(
      r,
      v.left + v.width / 2,
      v.top + v.height / 2,
      papelW,
      origenX,
      origenY,
    );
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

  function elegir(id: string) {
    seleccion = id;
    alFrente(id);
  }

  function quitar(id: string) {
    registrar();
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
      w: papelW,
      h: papelH,
    };
    bloques = [...bloques, capa];
    return capa;
  }

  function poner(bloque: NoteBlock) {
    registrar();
    bloques = [...bloques, bloque];
    seleccion = bloque.id;
    tocar();
    // Al crear se escribe de inmediato: sin esto hay que clicar dos veces.
    if (bloque.kind === "text" || bloque.kind === "check") {
      void enfocarEditor(bloque.id);
    }
  }

  async function enfocarEditor(id: string) {
    await tick();
    const raiz = papelEl?.querySelector(`[data-id="${id}"]`);
    const campo = raiz?.querySelector<HTMLElement>("textarea, li input[type=text]");
    campo?.focus();
  }

  function alClicPapel(event: PointerEvent) {
    if (event.button !== 0) return;
    const dest = event.target as HTMLElement;
    if (dest !== event.currentTarget && !dest.classList.contains("tinta")) return;
    const p = enPapel(event.clientX, event.clientY);
    if (herramienta === "select") {
      // Con select no se coloca nada: el papel vacío mueve la vista al arrastrar
      // y un clic limpio deselecciona. Antes el papel tapaba la vista y solo se
      // podía girar mirando el canto, que era más fácil vararse en el zoom.
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
      arrastre = {
        tipo: "pan",
        px: event.clientX,
        py: event.clientY,
        ox: panX,
        oy: panY,
      };
      panSinMovimiento = true;
      return;
    }
    if (herramienta === "text") {
      poner({
        kind: "text",
        id: nuevoId(),
        body: "",
        ...ubicar(280, 96, p.x, p.y),
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
        ...ubicar(260, 120, p.x, p.y),
        w: 260,
        h: 120,
      });
      herramienta = "select";
    }
  }

  function empezarTrazo(event: PointerEvent) {
    if ((herramienta !== "draw" && herramienta !== "highlight") || event.button !== 0)
      return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    const p = enPapel(event.clientX, event.clientY);
    trazoVivo = [[p.x, p.y]];
    arrastre = { tipo: "trazo" };
  }

  function empezarTinta(event: PointerEvent) {
    if (herramienta === "eraser") empezarBorrado(event);
    else empezarTrazo(event);
  }

  function extenderTrazo(event: PointerEvent) {
    if (!trazoVivo) return;
    const crudo = enPapel(event.clientX, event.clientY);
    // Las páginas son explícitas: el trazo no se escapa del papel.
    const p = limitar(crudo.x, crudo.y, 0, 0);
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
    registrar();
    const capa = asegurarTinta();
    if (capa.kind !== "ink") return;
    capa.strokes = [
      ...capa.strokes,
      { color: trazoColor, width: trazoAncho, points: trazoVivo },
    ];
    trazoVivo = null;
    arrastre = null;
    tocar();
  }

  // El borrador llega a la tinta por el mismo camino que el lápiz; el registro
  // en el historial se hace en el primer borrado real, no en el primer evento.
  let borradoRegistrado = false;

  function empezarBorrado(event: PointerEvent) {
    if (herramienta !== "eraser" || event.button !== 0) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    arrastre = { tipo: "borrar", px: event.clientX, py: event.clientY, movido: false };
    borradoRegistrado = false;
  }

  function borrarTramo(
    drag: { tipo: "borrar"; px: number; py: number; movido: boolean },
    x: number,
    y: number,
  ) {
    // Sin moverse de verdad es un clic, no un arrastre: no se toca nada y al
    // soltar se borra el trazo entero. El jitter del puntero ya no parte líneas.
    if (!drag.movido && Math.hypot(x - drag.px, y - drag.py) <= 5) return;
    drag.movido = true;
    const capa = bloques.find((b) => b.kind === "ink");
    if (!capa || capa.kind !== "ink") return;
    if (!borradoRegistrado) {
      borradoRegistrado = true;
      registrar();
    }
    const desde = enPapel(drag.px, drag.py);
    const hasta = enPapel(x, y);
    let trazos = capa.strokes;
    const pasos = Math.max(
      1,
      Math.ceil(Math.hypot(hasta.x - desde.x, hasta.y - desde.y) / (BORRAR_RADIO / 2)),
    );
    for (let i = 1; i <= pasos; i++) {
      const t = i / pasos;
      trazos = borrarPuntos(
        trazos,
        desde.x + (hasta.x - desde.x) * t,
        desde.y + (hasta.y - desde.y) * t,
        BORRAR_RADIO,
      );
    }
    capa.strokes = trazos;
    drag.px = x;
    drag.py = y;
  }

  function soltarBorrado() {
    const drag = arrastre;
    if (!drag || drag.tipo !== "borrar") return;
    if (!borradoRegistrado) {
      // Clic limpio: se borra la línea entera más cercana.
      const p = enPapel(drag.px, drag.py);
      const capa = bloques.find((b) => b.kind === "ink");
      if (capa?.kind === "ink") {
        const res = borrarLineaCercana(capa.strokes, p.x, p.y, BORRAR_RADIO);
        if (res.borro) {
          registrar();
          capa.strokes = res.trazos;
          tocar();
        }
      }
    } else {
      tocar();
    }
    borradoRegistrado = false;
    arrastre = null;
  }

  function empezarMover(event: PointerEvent, id: string) {
    if (herramienta !== "select" || event.button !== 0) return;
    const dest = event.target as HTMLElement;
    if (dest.closest(".asa, .mas, .quitar-bloque")) return;
    if (dest.closest("input[type=checkbox]")) return;
    // El textarea ocupa el recuadro entero: si ya está seleccionado, se escribe.
    // Si no, el clic arrastra. El agarre de arriba mueve siempre.
    if (
      dest.closest("textarea, input[type=text]") &&
      seleccion === id &&
      !dest.closest(".agarre")
    ) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    elegir(id);
    const bloque = bloques.find((b) => b.id === id);
    if (!bloque) return;
    const m = marcoDe(bloque);
    const raiz = dest.closest(".objeto") as HTMLElement | null;
    (raiz ?? (event.currentTarget as HTMLElement)).setPointerCapture(event.pointerId);
    arrastre = {
      tipo: "mover",
      id,
      ox: m.x,
      oy: m.y,
      ow: m.w,
      oh: m.h,
      px: event.clientX,
      py: event.clientY,
      registrado: false,
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
      registrado: false,
    };
  }

  function empezarPan(event: PointerEvent) {
    if (event.button !== 0 && event.button !== 1) return;
    if (event.button === 0 && (herramienta === "draw" || herramienta === "highlight"))
      return;
    if (event.button === 0 && event.target !== event.currentTarget) return;
    if (event.button === 1) event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    arrastre = {
      tipo: "pan",
      px: event.clientX,
      py: event.clientY,
      ox: panX,
      oy: panY,
    };
    // Clic limpio (botón izquierdo) = deseleccionar; el botón medio nunca.
    panSinMovimiento = event.button === 0;
  }

  // Pan con clic sobre el papel vacío: un clic limpio deselecciona, un arrastre
  // mueve la vista. El umbral distingue una cosa de la otra.
  let panSinMovimiento = false;
  const PAN_UMBRAL = 4;

  function alMover(event: PointerEvent) {
    const drag = arrastre;
    if (!drag) return;
    if (drag.tipo === "trazo") {
      extenderTrazo(event);
      return;
    }
    if (drag.tipo === "borrar") {
      borrarTramo(drag, event.clientX, event.clientY);
      return;
    }
    if (drag.tipo === "pan") {
      if (
        panSinMovimiento &&
        Math.hypot(event.clientX - drag.px, event.clientY - drag.py) > PAN_UMBRAL
      ) {
        panSinMovimiento = false;
      }
      vistaTocada = true;
      sinAnimacion();
      panX = drag.ox + (event.clientX - drag.px);
      panY = drag.oy + (event.clientY - drag.py);
      return;
    }
    const r = papelRect();
    if (!r) return;
    const unidad = r.width / Math.max(1, papelW);
    const dx = (event.clientX - drag.px) / unidad;
    const dy = (event.clientY - drag.py) / unidad;
    if (!drag.registrado) {
      drag.registrado = true;
      registrar();
    }
    const bloque = bloques.find((b) => b.id === drag.id);
    if (!bloque || bloque.kind === "ink") return;
    if (drag.tipo === "mover") {
      const dentro = limitar(drag.ox + dx, drag.oy + dy, drag.ow, drag.oh);
      bloque.x = dentro.x;
      bloque.y = dentro.y;
      return;
    }
    // El resize tampoco se escapa del papel: el tablero mantiene su tamaño.
    const maxW = Math.max(80, origenX + papelW - drag.ox);
    const maxH = Math.max(48, origenY + papelH - drag.oy);
    let w = Math.min(maxW, Math.max(80, drag.ow + dx));
    let h = Math.min(maxH, Math.max(48, drag.oh + dy));
    if (bloque.kind === "image") {
      const ratio = drag.oh / Math.max(1, drag.ow);
      h = Math.min(maxH, Math.max(48, w * ratio));
    }
    bloque.w = w;
    bloque.h = h;
  }

  function alSoltarPuntero() {
    if (arrastre?.tipo === "trazo") {
      cerrarTrazo();
      return;
    }
    if (arrastre?.tipo === "borrar") {
      soltarBorrado();
      return;
    }
    if (arrastre?.tipo === "pan") {
      if (panSinMovimiento && herramienta === "select") seleccion = "";
      arrastre = null;
      panSinMovimiento = false;
      return;
    }
    if (arrastre?.tipo === "mover" || arrastre?.tipo === "se") tocar();
    arrastre = null;
  }

  function alRueda(event: WheelEvent) {
    if (!(event.ctrlKey || event.metaKey)) return;
    event.preventDefault();
    vistaTocada = true;
    sinAnimacion();
    zoom = clampZoom(zoom * (event.deltaY > 0 ? 0.92 : 1.08));
  }

  function puntosSvg(points: [number, number][]): string {
    return points.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
  }

  /**
   * Mide texto con la tipografía real del tablero.
   *
   * La familia se lee de `--rb-font` en vez de repetirla: si el tema la cambia,
   * el ajuste la sigue. El canvas se crea una sola vez y no toca el layout.
   */
  let lienzoMedir: CanvasRenderingContext2D | null = null;
  let familiaTexto = "";

  function medirTexto(texto: string, tamano: number): number {
    lienzoMedir ??= document.createElement("canvas").getContext("2d");
    if (!lienzoMedir) return texto.length * tamano * 0.5;
    familiaTexto ||=
      getComputedStyle(document.documentElement).getPropertyValue("--rb-font").trim() ||
      FUENTE_TABLERO;
    lienzoMedir.font = `${tamano}px ${familiaTexto}`;
    return lienzoMedir.measureText(texto).width;
  }

  /**
   * Le da al bloque el alto que el texto pide, hasta el fondo del papel.
   *
   * Sólo crece: si achicás la caja a mano queda como la dejaste y el texto se
   * ajusta encogiéndose. Sin esto, escribir más de tres renglones en una caja de
   * 96px dejaba el resto detrás de un scroll que ni se podía tocar — porque el
   * bloque sin seleccionar tiene `pointer-events: none`.
   */
  function crecerTexto(bloque: Extract<NoteBlock, { kind: "text" }>) {
    const w = bloque.w ?? TEXTO_W;
    const h = bloque.h ?? TEXTO_H;
    const necesario = Math.ceil(
      altoEnvolvente(bloque.body, w, TEXTO_FUENTE, medirTexto),
    );
    const y = bloque.y ?? origenY;
    const tope = Math.max(h, origenY + papelH - y);
    const alto = Math.min(tope, Math.max(h, Math.max(necesario, TEXTO_ALTO_MIN)));
    if (alto > h) bloque.h = alto;
  }

  function escribirTexto(id: string, body: string) {
    const bloque = bloques.find((b) => b.id === id);
    if (bloque?.kind !== "text") return;
    bloque.body = body;
    crecerTexto(bloque);
    tocar();
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
    registrar();
    const bloque = bloques.find((b) => b.id === id);
    if (bloque?.kind !== "check") return;
    bloque.items = [...bloque.items, { id: nuevoId(), text: "", done: false }];
    // La lista crece hasta el borde del papel y ahí scrollea por dentro.
    const tope = Math.max(bloque.h ?? 0, origenY + papelH - (bloque.y ?? 0));
    bloque.h = Math.min(tope, Math.max(bloque.h ?? 0, 48 + bloque.items.length * 28));
    tocar();
  }

  // Sesión de tipeo dentro de un bloque: se guarda el estado ANTES y se
  // empuja al historial al salir, así una frase entera deshace de una vez y
  // los keystrokes no inundan el stack.
  function alEntrarEdicion(event: FocusEvent) {
    if (sesionAntes) return;
    const raiz = (event.target as HTMLElement).closest(".objeto");
    if (!raiz) return;
    sesionAntes = clonar(bloques);
  }

  function alSalirEdicion(event: FocusEvent) {
    if (!sesionAntes) return;
    const raiz = (event.target as HTMLElement).closest(".objeto");
    const relacionado = event.relatedTarget as Node | null;
    if (raiz && relacionado && raiz.contains(relacionado)) return;
    const actual = clonar(bloques);
    if (JSON.stringify(actual) !== JSON.stringify(sesionAntes)) {
      empujarAntes(sesionAntes);
    }
    sesionAntes = null;
  }

  function alTeclaObjeto(event: KeyboardEvent, id: string) {
    if (event.target !== event.currentTarget) return;
    const bloque = bloques.find((b) => b.id === id);
    if (!bloque) return;
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      const raiz = event.currentTarget as HTMLElement;
      if (bloque.kind === "text") {
        raiz.querySelector<HTMLTextAreaElement>("textarea")?.focus();
      } else if (bloque.kind === "check") {
        raiz.querySelector<HTMLInputElement>('li input[type="text"]')?.focus();
      }
      return;
    }
    if (bloque.kind === "ink") return;
    const dir = {
      ArrowLeft: [-1, 0],
      ArrowRight: [1, 0],
      ArrowUp: [0, -1],
      ArrowDown: [0, 1],
    }[event.key];
    if (!dir) return;
    event.preventDefault();
    registrar();
    const paso = event.shiftKey ? 1 : 8;
    const m = marcoDe(bloque);
    const dentro = limitar(
      (bloque.x ?? 0) + dir[0] * paso,
      (bloque.y ?? 0) + dir[1] * paso,
      m.w,
      m.h,
    );
    bloque.x = dentro.x;
    bloque.y = dentro.y;
    tocar();
  }

  function quitarItem(id: string, itemId: string): number {
    const bloque = bloques.find((b) => b.id === id);
    if (bloque?.kind !== "check") return -1;
    // La última fila se vacía pero no se quita: el bloque se borra entero.
    if (bloque.items.length <= 1) return -1;
    registrar();
    const idx = bloque.items.findIndex((i) => i.id === itemId);
    bloque.items = bloque.items.filter((i) => i.id !== itemId);
    bloque.h = Math.max(96, 24 + bloque.items.length * 28);
    tocar();
    return idx;
  }

  function camposDe(id: string): NodeListOf<HTMLInputElement> | undefined {
    const raiz = papelEl?.querySelector(`[data-id="${id}"]`);
    return raiz?.querySelectorAll<HTMLInputElement>('li input[type="text"]');
  }

  async function alTeclaItem(event: KeyboardEvent, id: string, itemId: string) {
    if (event.key === "Enter") {
      event.preventDefault();
      sumarCheck(id);
      await tick();
      const campos = camposDe(id);
      campos?.[campos.length - 1]?.focus();
      return;
    }
    // Backspace en fila vacía al inicio: quita la fila, no el carácter.
    const campo = event.currentTarget as HTMLInputElement;
    if (
      event.key === "Backspace" &&
      campo.value === "" &&
      (campo.selectionStart ?? 0) === 0
    ) {
      event.preventDefault();
      const idx = quitarItem(id, itemId);
      if (idx < 0) return;
      await tick();
      const campos = camposDe(id);
      const anterior = campos?.[Math.max(0, idx - 1)];
      if (anterior) anterior.focus();
      else papelEl?.querySelector<HTMLElement>(`[data-id="${id}"]`)?.focus();
    }
  }

  function insertarTexto(texto: string, x: number, y: number) {
    const w = TEXTO_W;
    const h = TEXTO_H;
    poner({ kind: "text", id: nuevoId(), body: texto, ...ubicar(w, h, x, y), w, h });
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
      ...ubicar(w, h, x, y),
      w,
      h,
    });
  }

  export async function insertarDelPortapapeles(
    item: ClipboardItem,
    x?: number,
    y?: number,
  ) {
    const centro = centroVisible();
    const cx = x ?? centro.x;
    const cy = y ?? centro.y;
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

  function insertarSnippet(item: Snippet, x?: number, y?: number) {
    const centro = centroVisible();
    insertarTexto(item.body, x ?? centro.x, y ?? centro.y);
  }

  function insertarBloques(nuevos: NoteBlock[]) {
    if (nuevos.length === 0) return;
    registrar();
    bloques = [...bloques, ...nuevos];
    papelW = Math.max(papelW, anchoParaBloques(bloques));
    const ultimo = nuevos[nuevos.length - 1];
    if (ultimo) seleccion = ultimo.id;
    tocar();
  }

  async function insertarResumen(item: Recording, x?: number, y?: number) {
    const sum = await getSummary(item.id);
    if (!sum?.body.trim()) return;
    const etiqueta = etiquetaResumen(item.started_at);
    const centro = centroVisible();
    // El resumen decide solo dónde arranca (margen de la celda, y al techo si no
    // entra): anclarlo después era lo que lo partía a una celda de más.
    insertarBloques(
      bloquesDeResumen(sum.body, etiqueta, nuevoId, {
        x: x ?? centro.x,
        y: y ?? centro.y,
      }),
    );
  }

  function previsualizarClip(item: ClipboardItem) {
    preview = { tipo: "clip", item };
  }

  function previsualizarSnippet(item: Snippet) {
    preview = { tipo: "snip", item };
  }

  function previsualizarCaptura(item: CaptureItem) {
    preview = { tipo: "cap", item };
  }

  async function previsualizarResumen(item: Recording) {
    preview = {
      tipo: "meet",
      item,
      etiqueta: etiquetaResumen(item.started_at),
      body: null,
    };
    const sum = await getSummary(item.id);
    if (preview?.tipo !== "meet" || preview.item.id !== item.id) return;
    preview = { ...preview, body: sum?.body ?? "" };
  }

  async function confirmarPreview() {
    const actual = preview;
    if (!actual) return;
    preview = null;
    if (actual.tipo === "clip") await insertarDelPortapapeles(actual.item);
    else if (actual.tipo === "snip") insertarSnippet(actual.item);
    else if (actual.tipo === "cap") await insertarCaptura(actual.item);
    else await insertarResumen(actual.item);
  }

  async function insertarCaptura(item: CaptureItem, x?: number, y?: number) {
    const centro = centroVisible();
    try {
      insertarImagen(
        await importWindowFlipImage(item.path),
        x ?? centro.x,
        y ?? centro.y,
      );
    } catch {
      // El archivo puede haberse borrado del shelf.
    }
  }

  function alArrancarFuente(
    event: DragEvent,
    tipo: FlipFuente,
    id: string,
    texto?: string | null,
  ) {
    event.dataTransfer?.setData(FLIP_MIME, payloadFlip(tipo, id));
    if (texto) event.dataTransfer?.setData("text/plain", texto);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "copy";
  }

  function alArrastrarSobre(event: DragEvent) {
    if (!event.dataTransfer?.types.includes(FLIP_MIME)) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "copy";
    soltando = true;
  }

  function alDejarDrag(event: DragEvent) {
    const dest = event.relatedTarget as Node | null;
    if (dest && (event.currentTarget as HTMLElement).contains(dest)) return;
    soltando = false;
  }

  async function alSoltarFuente(event: DragEvent) {
    soltando = false;
    const payload = leerPayloadFlip(event.dataTransfer?.getData(FLIP_MIME) ?? "");
    if (!payload) return;
    event.preventDefault();
    const p = enPapel(event.clientX, event.clientY);
    if (payload.tipo === "clip") {
      const item = portapapeles.find((i) => i.id === payload.id);
      if (item) await insertarDelPortapapeles(item, p.x, p.y);
      return;
    }
    if (payload.tipo === "snip") {
      const item = textos.find((i) => i.id === payload.id);
      if (item) insertarSnippet(item, p.x, p.y);
      return;
    }
    if (payload.tipo === "meet") {
      const item = reuniones.find((i) => i.id === payload.id);
      if (item) await insertarResumen(item, p.x, p.y);
      return;
    }
    const item = capturas.find((i) => i.id === payload.id);
    if (item) await insertarCaptura(item, p.x, p.y);
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
    const p = centroVisible();
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
    vivo = true;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && preview) {
        event.stopImmediatePropagation();
        preview = null;
        return;
      }
      if (event.key === "Escape" && paletaAbierta) {
        event.stopImmediatePropagation();
        paletaAbierta = false;
        return;
      }
      if (event.key === "Escape" && (seleccion || herramienta !== "select")) {
        event.stopImmediatePropagation();
        seleccion = "";
        herramienta = "select";
        return;
      }
      const tag = (event.target as HTMLElement)?.tagName;
      const esEdicion = tag === "TEXTAREA" || tag === "INPUT";
      const mod = event.ctrlKey || event.metaKey;
      // Dentro de un editor se deja el deshacer nativo del campo; fuera,
      // Ctrl+Z es el historial del tablero.
      if (mod && event.key.toLowerCase() === "z" && !esEdicion) {
        event.preventDefault();
        if (event.shiftKey) rehacer();
        else deshacer();
        return;
      }
      if (mod && event.key.toLowerCase() === "y" && !esEdicion) {
        event.preventDefault();
        rehacer();
        return;
      }
      if (esEdicion) return;
      const tecla = event.key.toLowerCase();
      if (tecla === "v") {
        herramienta = "select";
        return;
      }
      if (tecla === "p" || tecla === "d") {
        herramienta = "draw";
        paletaAbierta = true;
        return;
      }
      if (tecla === "h") {
        herramienta = "highlight";
        paletaAbierta = true;
        return;
      }
      if (tecla === "e") {
        herramienta = "eraser";
        return;
      }
      if (tecla === "t") {
        herramienta = "text";
        return;
      }
      if (tecla === "l") {
        herramienta = "check";
        return;
      }
      if ((event.key === "Delete" || event.key === "Backspace") && seleccion) {
        event.preventDefault();
        quitar(seleccion);
      }
    };
    window.addEventListener("keydown", onKey);
    const onDown = (event: PointerEvent) => {
      const el = event.target as HTMLElement;
      if (paletaAbierta && !el.closest(".paleta-colores, .grupo.herramientas")) {
        paletaAbierta = false;
      }
      if (menuExport && !el.closest(".export-wrap")) {
        menuExport = false;
      }
    };
    window.addEventListener("pointerdown", onDown);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onDown);
    };
  });
</script>

<div class="tablero">
  {#if preview}
    <FlipInsertPreview
      {preview}
      oncancel={() => (preview = null)}
      onconfirm={() => void confirmarPreview()}
    />
  {/if}
  <div class="barra" role="toolbar" aria-label={t("overlay.windowFlip.tools")}>
    {#if encabezado}
      <div class="encabezado">{@render encabezado()}</div>
    {/if}
    <div class="grupo herramientas" bind:this={herramientasEl}>
      <span
        class="pastilla"
        class:lista={pastillaLista}
        style:transform={`translateX(${pastillaX}px)`}
        style:width={`${pastillaW}px`}
        aria-hidden="true"
      ></span>
      {#each herramientas as item (item.id)}
        {@const esLapiz = item.id === "draw" || item.id === "highlight"}
        <button
          type="button"
          class="rb-btn rb-btn-ghost ico"
          class:activa={herramienta === item.id}
          title={`${item.label} (${item.key})`}
          aria-label={item.label}
          aria-keyshortcuts={item.key}
          aria-pressed={herramienta === item.id}
          aria-expanded={esLapiz && herramienta === item.id ? paletaAbierta : undefined}
          onclick={() => elegirHerramienta(item.id)}
        >
          <span class="tool-icon">
            <Icon icon={item.icon} size={15} />
            {#if esLapiz}
              <span class="tinta-punto" style:background={tinta} aria-hidden="true"
              ></span>
            {/if}
          </span>
          {#if !compacta}<span>{item.label}</span>{/if}
        </button>
        {#if esLapiz && herramienta === item.id && paletaAbierta}
          <div
            class="paleta-colores"
            role="radiogroup"
            aria-label={t("overlay.windowFlip.pens")}
            style:left={`${pastillaX}px`}
            transition:emerge={{ duration: vivo ? undefined : 0 }}
          >
            {#each LAPICES as lapiz (lapiz.color)}
              <button
                type="button"
                class="lapiz"
                class:elegido={tinta === lapiz.color}
                style:--tinta={lapiz.color}
                role="radio"
                aria-checked={tinta === lapiz.color}
                title={t(`overlay.windowFlip.${lapiz.name}`)}
                aria-label={t(`overlay.windowFlip.${lapiz.name}`)}
                onclick={() => elegirColor(lapiz.color)}
              ></button>
            {/each}
            <label
              class="lapiz lapiz-libre"
              class:elegido={esTintaLibre}
              style:--tinta={esTintaLibre ? tinta : "transparent"}
              title={t("overlay.windowFlip.penCustom")}
            >
              <input
                type="color"
                class="sr-only"
                value={tinta}
                aria-label={t("overlay.windowFlip.penCustom")}
                oninput={(event) => (tinta = event.currentTarget.value)}
                onchange={() => (paletaAbierta = false)}
              />
              <span aria-hidden="true">+</span>
            </label>
          </div>
        {/if}
      {/each}
    </div>
    <div class="grupo">
      <button
        type="button"
        class="rb-btn rb-btn-ghost ico"
        disabled={!puedeDeshacer}
        title={t("overlay.windowFlip.undoTitle")}
        aria-label={t("overlay.windowFlip.undo")}
        onclick={deshacer}
      >
        <Icon icon={Undo2} size={15} />
      </button>
      <button
        type="button"
        class="rb-btn rb-btn-ghost ico"
        disabled={!puedeRehacer}
        title={t("overlay.windowFlip.redoTitle")}
        aria-label={t("overlay.windowFlip.redo")}
        onclick={rehacer}
      >
        <Icon icon={Redo2} size={15} />
      </button>
      <button
        type="button"
        class="rb-btn rb-btn-ghost ico"
        title={t("overlay.windowFlip.zoomOut")}
        aria-label={t("overlay.windowFlip.zoomOut")}
        onclick={() => {
          vistaTocada = true;
          void moverVista(() => (zoom = clampZoom(zoom / 1.15)));
        }}
      >
        <Icon icon={Minus} size={15} />
      </button>
      <button
        type="button"
        class="rb-btn rb-btn-ghost zoom"
        title={t("overlay.windowFlip.zoomReset")}
        aria-label={t("overlay.windowFlip.zoomReset")}
        onclick={() => {
          vistaTocada = false;
          void moverVista(() => encuadrar());
        }}
      >
        {Math.round(zoom * 100)}%
      </button>
      <button
        type="button"
        class="rb-btn rb-btn-ghost ico"
        title={t("overlay.windowFlip.zoomIn")}
        aria-label={t("overlay.windowFlip.zoomIn")}
        onclick={() => {
          vistaTocada = true;
          void moverVista(() => (zoom = clampZoom(zoom * 1.15)));
        }}
      >
        <Icon icon={Plus} size={15} />
      </button>
      {#if seleccion}
        <button
          type="button"
          class="rb-btn rb-btn-ghost rb-btn-danger ico"
          title={t("overlay.windowFlip.removeBlock")}
          aria-label={t("overlay.windowFlip.removeBlock")}
          transition:emerge
          onclick={() => quitar(seleccion)}
        >
          <Icon icon={Trash2} size={15} />
        </button>
      {/if}
      <div class="export-wrap">
        <button
          type="button"
          class="rb-btn rb-btn-ghost ico"
          disabled={exportando}
          title={t("overlay.windowFlip.exportAria")}
          aria-label={t("overlay.windowFlip.exportAria")}
          aria-expanded={menuExport}
          aria-haspopup="menu"
          onclick={() => (menuExport = !menuExport)}
        >
          <span class="gira" class:girando={exportando} aria-hidden="true">
            <Icon icon={exportando ? LoaderCircle : Download} size={15} />
          </span>
          {#if !accionesSoloIcono}<span>{t("overlay.windowFlip.export")}</span>{/if}
        </button>
        {#if menuExport}
          <div
            class="menu-export"
            transition:emerge
            role="menu"
            aria-label={t("overlay.windowFlip.export")}
          >
            {#each FORMATOS_TABLERO as formato (formato)}
              <button
                type="button"
                class="menu-export-item"
                role="menuitem"
                disabled={exportando}
                onclick={() => void exportar(formato)}
              >
                <span class="mi-icono" aria-hidden="true">
                  <Icon icon={ICONO_EXPORT[formato]} size={14} />
                </span>
                {t(`overlay.windowFlip.${ETIQUETA_EXPORT[formato]}`)}
              </button>
            {/each}
            <p class="menu-export-hint">{t("overlay.windowFlip.exportPages")}</p>
          </div>
        {/if}
      </div>
      <button
        type="button"
        class="rb-btn rb-btn-ghost"
        aria-pressed={cajonAbierto}
        aria-expanded={cajonAbierto}
        title={t("overlay.windowFlip.drawer")}
        aria-label={t("overlay.windowFlip.drawer")}
        onclick={alternarCajon}
      >
        <Icon icon={cajonAbierto ? PanelRightClose : PanelRightOpen} size={15} />
        {#if !accionesSoloIcono}<span>{t("overlay.windowFlip.drawer")}</span>{/if}
      </button>
      <button
        type="button"
        class="rb-btn rb-btn-soft ico"
        title={t("overlay.windowFlip.close")}
        aria-label={t("overlay.windowFlip.close")}
        onclick={() => onclose?.()}
      >
        <Icon icon={ArrowLeft} size={15} />
        {#if !accionesSoloIcono}<span>{t("overlay.windowFlip.close")}</span>{/if}
      </button>
    </div>
  </div>
  {#if avisoExport}
    <p class="aviso-export" aria-live="polite">{avisoExport}</p>
  {/if}

  <div class="cuerpo" class:con-cajon={cajonAbierto}>
    <div
      bind:this={vistaEl}
      class="vista"
      class:dibujando={herramienta === "draw" ||
        herramienta === "highlight" ||
        herramienta === "eraser"}
      class:soltando
      role="application"
      aria-label={t("overlay.windowFlip.board")}
      onwheel={alRueda}
      onpointerdown={empezarPan}
      onpointermove={alMover}
      onpointerup={alSoltarPuntero}
      onpointercancel={alSoltarPuntero}
      onpaste={alPegar}
      ondragover={alArrastrarSobre}
      ondragleave={alDejarDrag}
      ondrop={(event) => void alSoltarFuente(event)}
    >
      <div
        bind:this={escenaEl}
        class="escena"
        class:animando={animandoVista}
        style:transform={`translate(${panX}px, ${panY}px) scale(${zoom})`}
      >
        <div class="marco-paginas" style:--inv={1 / zoom}>
          <div
            bind:this={papelEl}
            class="papel"
            role="listbox"
            aria-label={t("overlay.windowFlip.board")}
            tabindex="-1"
            style:width={`${papelW}px`}
            style:height={`${papelH}px`}
            onpointerdown={alClicPapel}
            onfocusin={alEntrarEdicion}
            onfocusout={alSalirEdicion}
          >
            {#if vacio}
              <div class="vacio" transition:emerge={{ duration: vivo ? undefined : 0 }}>
                <p>{t("overlay.windowFlip.boardEmpty")}</p>
                {#if herramienta !== "draw" && herramienta !== "highlight" && herramienta !== "eraser"}
                  <button
                    type="button"
                    class="rb-btn rb-btn-soft"
                    onclick={() =>
                      insertarTexto("", origenX + papelW / 2, origenY + papelH * 0.42)}
                  >
                    {t("overlay.windowFlip.addText")}
                  </button>
                {/if}
              </div>
            {/if}

            {#each paginas.verticales as x (x)}
              <div
                class="linea-pagina-v"
                style:left={`${x}px`}
                aria-hidden="true"
              ></div>
            {/each}
            {#if destello}
              {#key destello.marca}
                <div
                  class="linea-nueva v"
                  style:left={`${destello.x}px`}
                  aria-hidden="true"
                ></div>
              {/key}
            {/if}

            <div
              class="tinta"
              class:captura={herramienta === "draw" ||
                herramienta === "highlight" ||
                herramienta === "eraser"}
              role="img"
              aria-hidden="true"
              onpointerdown={empezarTinta}
              onpointermove={alMover}
              onpointerup={alSoltarPuntero}
              onpointercancel={alSoltarPuntero}
            >
              <svg
                viewBox={`${origenX} ${origenY} ${papelW} ${papelH}`}
                preserveAspectRatio="none"
                aria-hidden="true"
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
                    stroke={trazoColor}
                    stroke-width={trazoAncho}
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    points={puntosSvg(trazoVivo)}
                  />
                {/if}
              </svg>
            </div>

            {#each objetos as bloque (bloque.id)}
              {@const m = marcoDe(bloque)}
              <div
                class="objeto"
                class:seleccionado={seleccion === bloque.id}
                class:lista={bloque.kind === "check"}
                role="option"
                aria-selected={seleccion === bloque.id}
                tabindex="0"
                data-id={bloque.id}
                style:left={`${m.x - origenX}px`}
                style:top={`${m.y - origenY}px`}
                style:width={`${m.w}px`}
                style:height={`${m.h}px`}
                transition:emerge={{ duration: vivo ? undefined : 0 }}
                onfocus={() => elegir(bloque.id)}
                onkeydown={(event) => alTeclaObjeto(event, bloque.id)}
                onpointerdown={(event) => empezarMover(event, bloque.id)}
                onpointermove={alMover}
                onpointerup={alSoltarPuntero}
                onpointercancel={alSoltarPuntero}
              >
                <button
                  type="button"
                  class="agarre"
                  tabindex="-1"
                  aria-label={t("overlay.windowFlip.dragBlock")}
                  onpointerdown={(event) => empezarMover(event, bloque.id)}
                >
                  <Icon icon={GripVertical} size={12} />
                </button>
                {#if bloque.kind === "text"}
                  {@const fuente = ajustarFuente(
                    bloque.body,
                    bloque.w ?? TEXTO_W,
                    bloque.h ?? TEXTO_H,
                    medirTexto,
                  )}
                  <textarea
                    tabindex="-1"
                    value={bloque.body}
                    placeholder={t("overlay.windowFlip.placeholder")}
                    spellcheck="false"
                    style:font-size={`${fuente}px`}
                    onfocus={() => elegir(bloque.id)}
                    oninput={(event) =>
                      escribirTexto(bloque.id, event.currentTarget.value)}></textarea>
                {:else if bloque.kind === "image"}
                  <img
                    src={windowFlipAssetSrc(assetsDir, bloque.asset)}
                    alt=""
                    draggable="false"
                  />
                {:else if bloque.kind === "check"}
                  <ul>
                    {#each bloque.items as item, i (item.id)}
                      <li transition:emerge={{ duration: vivo ? undefined : 0 }}>
                        <input
                          type="checkbox"
                          checked={item.done}
                          aria-label={t("overlay.windowFlip.checkItemN", { n: i + 1 })}
                          onfocus={() => elegir(bloque.id)}
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
                          onkeydown={(event) => alTeclaItem(event, bloque.id, item.id)}
                          oninput={(event) =>
                            escribirCheck(bloque.id, item.id, {
                              text: event.currentTarget.value,
                            })}
                        />
                      </li>
                    {/each}
                  </ul>
                  <button
                    type="button"
                    class="mas"
                    onclick={() => sumarCheck(bloque.id)}
                  >
                    {t("overlay.windowFlip.addCheck")}
                  </button>
                {/if}
                <button
                  type="button"
                  class="quitar-bloque"
                  tabindex="-1"
                  aria-hidden={seleccion !== bloque.id}
                  aria-label={t("overlay.windowFlip.removeBlock")}
                  onclick={() => quitar(bloque.id)}
                >
                  <Icon icon={X} size={11} />
                </button>
                <button
                  type="button"
                  class="asa"
                  class:visible={seleccion === bloque.id}
                  tabindex="-1"
                  aria-hidden={seleccion !== bloque.id}
                  aria-label={t("overlay.windowFlip.resize")}
                  onpointerdown={(event) => empezarResize(event, bloque.id)}
                  onpointermove={alMover}
                  onpointerup={alSoltarPuntero}
                  onpointercancel={alSoltarPuntero}
                ></button>
              </div>
            {/each}
          </div>
        </div>
      </div>
    </div>

    <aside
      class="cajon"
      class:abierto={cajonAbierto}
      inert={!cajonAbierto}
      aria-hidden={!cajonAbierto}
      aria-label={t("overlay.windowFlip.drawer")}
    >
      <div class="cajon-cuerpo">
        <div
          class="fuentes"
          role="tablist"
          aria-label={t("overlay.windowFlip.drawerSources")}
        >
          <button
            type="button"
            class="fuente"
            class:activa={fuente === "clip"}
            role="tab"
            aria-selected={fuente === "clip"}
            title={t("overlay.windowFlip.clipboard")}
            aria-label={t("overlay.windowFlip.clipboard")}
            onclick={() => (fuente = "clip")}
          >
            {t("overlay.windowFlip.clipTab")}
          </button>
          <button
            type="button"
            class="fuente"
            class:activa={fuente === "snip"}
            role="tab"
            aria-selected={fuente === "snip"}
            title={t("overlay.windowFlip.snippets")}
            aria-label={t("overlay.windowFlip.snippets")}
            onclick={() => (fuente = "snip")}
          >
            {t("overlay.windowFlip.snipTab")}
          </button>
          <button
            type="button"
            class="fuente"
            class:activa={fuente === "cap"}
            role="tab"
            aria-selected={fuente === "cap"}
            title={t("overlay.windowFlip.captures")}
            aria-label={t("overlay.windowFlip.captures")}
            onclick={() => (fuente = "cap")}
          >
            {t("overlay.windowFlip.capTab")}
          </button>
          <button
            type="button"
            class="fuente"
            class:activa={fuente === "meet"}
            role="tab"
            aria-selected={fuente === "meet"}
            title={t("overlay.windowFlip.meetings")}
            aria-label={t("overlay.windowFlip.meetings")}
            onclick={() => (fuente = "meet")}
          >
            {t("overlay.windowFlip.meetTab")}
          </button>
        </div>
        <div class="pila">
          {#if fuente === "clip"}
            <div class="hoja" in:tabPanel|local out:tabPanel|local>
              {#each portapapeles.slice(0, 40) as item (item.id)}
                <button
                  type="button"
                  class="recorte"
                  class:imagen={item.kind === "image"}
                  title={item.preview}
                  aria-label={item.text ||
                    item.preview ||
                    t("overlay.windowFlip.clipboardItem")}
                  draggable="true"
                  ondragstart={(event) =>
                    alArrancarFuente(event, "clip", item.id, item.text)}
                  onclick={() => previsualizarClip(item)}
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
          {:else if fuente === "snip"}
            <div class="hoja" in:tabPanel|local out:tabPanel|local>
              {#each textos as item (item.id)}
                <button
                  type="button"
                  class="recorte"
                  title={item.body}
                  aria-label={t("overlay.windowFlip.snippetItem", { name: item.name })}
                  draggable="true"
                  ondragstart={(event) =>
                    alArrancarFuente(event, "snip", item.id, item.body)}
                  onclick={() => previsualizarSnippet(item)}
                >
                  <span class="recorte-nombre">{item.name}</span>
                  <span>{item.body}</span>
                </button>
              {:else}
                <p class="hueco">{t("overlay.windowFlip.snippetsEmpty")}</p>
              {/each}
            </div>
          {:else if fuente === "cap"}
            <div class="hoja" in:tabPanel|local out:tabPanel|local>
              {#each capturas.slice(0, 40) as item (item.id)}
                <button
                  type="button"
                  class="recorte imagen"
                  title={item.label}
                  aria-label={t("overlay.windowFlip.captureItem", {
                    label: item.label,
                  })}
                  draggable="true"
                  ondragstart={(event) => alArrancarFuente(event, "cap", item.id)}
                  onclick={() => previsualizarCaptura(item)}
                >
                  <img src={captureSrc(item.path)} alt="" />
                </button>
              {:else}
                <p class="hueco">{t("overlay.windowFlip.capturesEmpty")}</p>
              {/each}
            </div>
          {:else}
            <div class="hoja" in:tabPanel|local out:tabPanel|local>
              {#each reuniones as item (item.id)}
                <button
                  type="button"
                  class="recorte"
                  title={item.title}
                  aria-label={etiquetaResumen(item.started_at)}
                  draggable="true"
                  ondragstart={(event) => alArrancarFuente(event, "meet", item.id)}
                  onclick={() => void previsualizarResumen(item)}
                >
                  <span class="recorte-nombre">{etiquetaResumen(item.started_at)}</span>
                  <span>{item.title}</span>
                </button>
              {:else}
                <p class="hueco">{t("overlay.windowFlip.meetingsEmpty")}</p>
              {/each}
            </div>
          {/if}
        </div>
      </div>
    </aside>
  </div>

  <div class="tira" class:compacta>
    <div class="tira-paginas" role="tablist" aria-label={t("overlay.windowFlip.pages")}>
      {#each miniaturas as pagina (pagina.indice)}
        <div class="tira-item" transition:emerge={{ duration: vivo ? undefined : 0 }}>
          <button
            type="button"
            class="mini"
            class:activa={paginaActual === pagina.indice}
            role="tab"
            aria-selected={paginaActual === pagina.indice}
            title={t("overlay.windowFlip.pageN", { n: String(pagina.indice + 1) })}
            aria-label={t("overlay.windowFlip.pageN", {
              n: String(pagina.indice + 1),
            })}
            onclick={() => irAPagina(pagina.indice)}
          >
            <span class="mini-hoja" aria-hidden="true">
              {#each pagina.piezas as pieza (pieza.id)}
                {#if pieza.tipo === "image"}
                  <img
                    class="mini-foto"
                    src={windowFlipAssetSrc(assetsDir, pieza.asset)}
                    alt=""
                    draggable="false"
                    style:left={`${pieza.x}%`}
                    style:top={`${pieza.y}%`}
                    style:width={`${pieza.w}%`}
                    style:height={`${pieza.h}%`}
                  />
                {:else if pieza.tipo === "check"}
                  <span
                    class="mini-lista"
                    style:left={`${pieza.x}%`}
                    style:top={`${pieza.y}%`}
                    style:width={`${pieza.w}%`}
                    style:height={`${pieza.h}%`}
                  >
                    {#each Array.from({ length: pieza.filas }, (_, i) => i) as fila (fila)}
                      <span class="mini-fila"></span>
                    {/each}
                  </span>
                {:else}
                  <span
                    class="mini-texto"
                    style:left={`${pieza.x}%`}
                    style:top={`${pieza.y}%`}
                    style:width={`${pieza.w}%`}
                    style:height={`${pieza.h}%`}
                  >
                    {#each Array.from({ length: pieza.renglones }, (_, i) => i) as renglon (renglon)}
                      <span class="mini-renglon"></span>
                    {/each}
                  </span>
                {/if}
              {/each}
              {#if pagina.trazos.length > 0}
                <svg
                  class="mini-tinta"
                  viewBox={`${pagina.indice * PAGINA_W} 0 ${PAGINA_W} ${PAGINA_H}`}
                  preserveAspectRatio="none"
                >
                  {#each pagina.trazos as trazo, k (k)}
                    <polyline
                      points={trazo.puntos}
                      fill="none"
                      stroke={trazo.color}
                      stroke-width={trazo.ancho}
                      stroke-linecap="round"
                      stroke-linejoin="round"
                    />
                  {/each}
                </svg>
              {/if}
            </span>
            <span class="mini-num">{pagina.indice + 1}</span>
          </button>
          {#if pagina.indice === paginasTotales - 1 && paginasTotales > 1 && franjaVacia()}
            <button
              type="button"
              class="mini-x"
              transition:emerge
              title={t("overlay.windowFlip.removePage")}
              aria-label={t("overlay.windowFlip.removePage")}
              onclick={() => quitarPagina()}
            >
              <Icon icon={X} size={10} />
            </button>
          {/if}
        </div>
      {/each}
    </div>
    <button
      type="button"
      class="mini mini-mas"
      transition:emerge
      title={t("overlay.windowFlip.addPage")}
      aria-label={t("overlay.windowFlip.addPage")}
      onclick={agregarYVer}
    >
      <Icon icon={Plus} size={15} />
    </button>
  </div>
</div>

<style>
  .tablero {
    /* Los dos anchos que se pedían fijos y en ventana chica se comían el bloc. */
    --cajon-w: clamp(124px, 18vw, 168px);
    --mini-w: clamp(52px, 7vw, 84px);

    position: relative;
    display: flex;
    flex: 1;
    min-height: 0;
    flex-direction: column;
  }

  /*
   * El icono y el título de la ventana comparten la fila con la barra: antes
   * tenían una fila propia que en ventana chica se comía 56px del bloc. Cede
   * ancho —y se recorta el título— antes que partir la barra en dos filas.
   */
  .encabezado {
    display: flex;
    max-width: 34%;
    min-width: 0;
    flex: 0 1 auto;
    align-items: center;
  }

  .barra {
    display: flex;
    flex-shrink: 0;
    flex-wrap: wrap;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 0 var(--pad) 8px;
  }

  .grupo {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px;
  }

  .export-wrap {
    position: relative;
  }

  .menu-export {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    z-index: 10;
    display: flex;
    min-width: 148px;
    flex-direction: column;
    padding: 6px;
    border: 1px solid var(--rb-hairline);
    border-radius: var(--rb-radius-sm);
    background: var(--rb-surface);
    box-shadow: 0 8px 24px rgb(0 0 0 / 22%);
  }

  /* El icono gira mientras se arma el archivo; el morph lo cambia solo. */
  .gira {
    display: grid;
    place-items: center;
  }

  .girando {
    animation: girar 900ms linear infinite;
  }

  @keyframes girar {
    to {
      transform: rotate(360deg);
    }
  }

  .menu-export-item {
    display: flex;
    align-items: center;
    gap: 8px;
    transition: background-color var(--duration-quick) var(--ease-smooth-out);
    padding: 6px 10px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  .menu-export-item:hover {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .mi-icono {
    display: grid;
    width: 16px;
    place-items: center;
    color: var(--rb-muted);
  }

  .menu-export-hint {
    margin: 4px 10px 2px;
    color: var(--rb-muted);
    font-size: 10.5px;
    line-height: 1.35;
  }

  .aviso-export {
    margin: 0 var(--pad) 6px;
    color: var(--rb-muted);
    font-size: 11.5px;
  }

  .herramientas {
    position: relative;
    flex-wrap: nowrap;
  }

  .pastilla {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    z-index: 0;
    border-radius: 999px;
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
    opacity: 0;
    pointer-events: none;
  }

  .pastilla.lista {
    opacity: 1;
    transition:
      transform var(--duration-medium) var(--ease-smooth-out),
      width var(--duration-medium) var(--ease-smooth-out),
      opacity var(--duration-fast) var(--ease-smooth-out);
  }

  .ico {
    position: relative;
    z-index: 1;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    background: transparent;
  }

  .ico.activa {
    background: transparent;
    color: var(--rb-text);
  }

  .tool-icon {
    position: relative;
    display: inline-flex;
  }

  .tinta-punto {
    position: absolute;
    right: -3px;
    bottom: -3px;
    width: 7px;
    height: 7px;
    border-radius: 999px;
    box-shadow: 0 0 0 1.5px var(--rb-surface);
    pointer-events: none;
    transition: background-color var(--duration-fast) var(--ease-smooth-out);
  }

  .zoom {
    min-width: 3.2rem;
    font-variant-numeric: tabular-nums;
  }

  .cuerpo {
    display: grid;
    flex: 1;
    min-height: 0;
    grid-template-columns: minmax(0, 1fr) 0fr;
    transition: grid-template-columns var(--duration-slow) var(--ease-smooth-out);
  }

  .cuerpo.con-cajon {
    grid-template-columns: minmax(0, 1fr) var(--cajon-w);
  }

  .vista {
    position: relative;
    min-width: 0;
    overflow: hidden;
    background: var(--rb-bg1);
    cursor: grab;
    touch-action: none;
  }

  .vista.dibujando {
    cursor: crosshair;
  }

  .vista.soltando .papel {
    outline: 1.5px solid color-mix(in sRGB, var(--rb-text) 35%, transparent);
    outline-offset: -2px;
    transition: outline-color var(--duration-fast) var(--ease-smooth-out);
  }

  /*
   * Salto de vista (tira de páginas, zoom, encuadre): `--duration-fast` con
   * smooth-out, el mismo par que usa el repo para «cambio de posición». Arrastrar
   * y la rueda no llevan la clase: ahí el movimiento es continuo.
   */
  .escena.animando {
    transition: transform var(--duration-fast) var(--ease-smooth-out);
  }

  .escena {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
    transform-origin: center center;
  }

  .papel {
    --hoja: var(--rb-surface-elevated);
    --punto: color-mix(in sRGB, var(--rb-text) 18%, transparent);

    position: relative;
    flex: none;
    box-sizing: border-box;
    overflow: hidden;
    pointer-events: auto;
    border-radius: var(--rb-radius-sm);
    background-color: var(--hoja);
    background-image: radial-gradient(circle, var(--punto) 1px, transparent 1.25px);
    background-size: 24px 24px;
    background-position: 12px 12px;
    box-shadow: 0 2px 8px rgb(0 0 0 / 28%);
    cursor: grab;
    transform: translateZ(0);
  }

  :global(:root[data-theme-base="dark"]) .papel {
    --hoja: color-mix(in sRGB, var(--rb-surface-elevated) 70%, var(--rb-text));
    --punto: color-mix(in sRGB, var(--rb-text) 22%, transparent);
  }

  /* Separadores de página de la tira: línea leve, no decoración. */
  .linea-pagina-v {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    z-index: 0;
    background: var(--rb-hairline);
    opacity: 0.8;
    pointer-events: none;
  }

  .linea-nueva {
    position: absolute;
    z-index: 3;
    background: var(--rb-accent, var(--rb-text));
    pointer-events: none;
    animation: destello-pagina 1.2s ease-out 2;
  }

  .linea-nueva.v {
    top: 0;
    bottom: 0;
    width: 2px;
  }

  @keyframes destello-pagina {
    0%,
    100% {
      opacity: 0;
    }

    50% {
      opacity: 1;
    }
  }

  .vista.dibujando .papel {
    cursor: crosshair;
  }

  /* El marco envuelve justo el papel: el grupo [+]/[−] vive pegado a su
     borde derecho y viaja con él. El `--inv` compensa el zoom para que no
     cambie de tamaño en pantalla. */
  .marco-paginas {
    position: relative;
    flex: none;
  }

  /*
   * Tira de páginas: fija abajo, una miniatura por celda y el «+» al final.
   *
   * Es la forma de moverse cuando hay varias celdas: sin esto, con el tablero
   * ancho hay que arrastrar a ciegas hasta encontrar la página.
   */
  .tira {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 6px var(--pad) 10px;
    overflow: auto hidden;
    scrollbar-width: thin;
  }

  .tira-paginas {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .tira-item {
    position: relative;
    flex: none;
  }

  .mini {
    position: relative;
    display: block;
    width: var(--mini-w);
    aspect-ratio: 4 / 3;
    padding: 0;
    border: 1px solid var(--rb-hairline);
    border-radius: var(--rb-radius-xs);
    background: var(--rb-surface-elevated);
    cursor: pointer;
    overflow: hidden;
    transition:
      border-color var(--duration-fast) var(--ease-smooth-out),
      box-shadow var(--duration-fast) var(--ease-smooth-out),
      transform var(--duration-fast) var(--ease-smooth-out);
  }

  /* Un dedo señalando: se levanta 2px y vuelve, sin ceremonia. La tira deja
     lugar arriba para que el levantamiento y la «×» no se recorten. */
  .mini:hover {
    transform: translateY(-2px);
  }

  .mini.activa {
    border-color: var(--rb-accent);
    box-shadow: 0 0 0 1px var(--rb-accent);
  }

  .mini:focus-visible,
  .mini-x:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .mini-hoja {
    position: absolute;
    inset: 0;
  }

  .mini-foto,
  .mini-texto,
  .mini-lista {
    position: absolute;
  }

  .mini-foto {
    border-radius: 1px;
    object-fit: fill;
  }

  .mini-texto,
  .mini-lista {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 2px;
    overflow: hidden;
  }

  .mini-renglon,
  .mini-fila {
    display: block;
    height: 2px;
    flex: none;
    border-radius: 1px;
    background: color-mix(in sRGB, var(--rb-text) 38%, transparent);
  }

  .mini-renglon:last-child {
    width: 55%;
  }

  .mini-fila {
    position: relative;
    margin-left: 6px;
    background: color-mix(in sRGB, var(--rb-text) 30%, transparent);
  }

  /* La casilla del ítem, dibujada con el pseudo para no meter otro elemento. */
  .mini-fila::before {
    position: absolute;
    top: -1px;
    left: -6px;
    width: 4px;
    height: 4px;
    border: 1px solid color-mix(in sRGB, var(--rb-text) 38%, transparent);
    border-radius: 1px;
    content: "";
  }

  .mini-tinta {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .mini-num {
    position: absolute;
    right: 3px;
    bottom: 1px;
    color: var(--rb-muted);
    font-size: 9px;
    font-weight: 600;
    line-height: 1;
    text-shadow: 0 0 3px var(--rb-surface-elevated);
  }

  .mini-mas {
    display: grid;
    width: var(--mini-w);
    flex: none;
    place-items: center;
    border-style: dashed;
    color: var(--rb-muted);
    transition:
      color var(--duration-fast) var(--ease-smooth-out),
      border-color var(--duration-fast) var(--ease-smooth-out);
  }

  .mini-mas:hover {
    border-color: var(--rb-text);
    color: var(--rb-text);
  }

  /* La «×» sólo aparece si la última celda está vacía. */
  .mini-x {
    position: absolute;
    top: -5px;
    right: -5px;
    display: grid;
    width: 16px;
    height: 16px;
    place-items: center;
    padding: 0;
    border: 1px solid var(--rb-hairline);
    border-radius: 999px;
    background: var(--rb-surface);
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      color var(--duration-fast) var(--ease-smooth-out),
      border-color var(--duration-fast) var(--ease-smooth-out);
  }

  .mini-x:hover {
    border-color: var(--rb-text);
    color: var(--rb-text);
  }

  .vacio {
    position: absolute;
    inset: 26% 15% auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    margin: 0;
    color: var(--rb-text);
    font-size: 13px;
    line-height: 1.45;
    text-align: center;
    text-wrap: pretty;
    pointer-events: none;
  }

  .vacio p {
    margin: 0;
    max-width: 36ch;
  }

  .vacio button {
    pointer-events: auto;
  }

  .tinta {
    position: absolute;
    inset: 0;
    pointer-events: none;
    overflow: hidden;
    z-index: 0;
  }

  .tinta.captura {
    pointer-events: auto;
    touch-action: none;
    z-index: 4;
  }

  .tinta svg {
    display: block;
    width: 100%;
    height: 100%;
  }

  .objeto {
    position: absolute;
    box-sizing: border-box;
    overflow: hidden;
    border-radius: var(--rb-radius-xs);
    background: var(--hoja, var(--rb-surface-elevated));
    cursor: grab;
    outline: 1px solid transparent;
    z-index: 1;
    transition: outline-color var(--duration-fast) var(--ease-smooth-out);
  }

  .objeto:hover:not(.seleccionado) {
    outline-color: color-mix(in sRGB, var(--rb-text) 22%, transparent);
  }

  .objeto.seleccionado {
    outline: 1.5px solid color-mix(in sRGB, var(--rb-text) 45%, transparent);
    overflow: visible;
    z-index: 2;
  }

  .objeto:not(.seleccionado) textarea {
    pointer-events: none;
  }

  /* Este WebView no vive bajo `.atic-root`; los rings globales no aplican y
     los inputs metían `outline: none` sin reemplazo. */
  .objeto:focus-visible,
  .objeto textarea:focus-visible,
  .objeto li input:focus-visible,
  .mas:focus-visible,
  .lapiz:focus-visible,
  .recorte:focus-visible,
  .quitar-bloque:focus-visible,
  .asa:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .agarre {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 3;
    display: flex;
    align-items: center;
    justify-content: center;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: var(--rb-radius-xs) var(--rb-radius-xs) 0 0;
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-muted);
    cursor: grab;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--duration-fast) var(--ease-smooth-out);
  }

  .objeto:hover .agarre,
  .objeto.seleccionado .agarre,
  .objeto:focus-within .agarre {
    opacity: 1;
    pointer-events: auto;
  }

  .quitar-bloque {
    /* Dentro del bloque: fuera lo recorta el `overflow: hidden` de los
       bloques sin seleccionar. */
    position: absolute;
    right: 3px;
    top: 3px;
    z-index: 6;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 1px solid var(--rb-hairline);
    border-radius: 999px;
    background: var(--rb-surface);
    color: var(--rb-muted);
    cursor: pointer;
    opacity: 0;
    pointer-events: none;
    transition:
      opacity var(--duration-fast) var(--ease-smooth-out),
      color var(--duration-fast) var(--ease-smooth-out);
  }

  .objeto:hover .quitar-bloque,
  .objeto.seleccionado .quitar-bloque,
  .objeto:focus-within .quitar-bloque {
    opacity: 1;
    pointer-events: auto;
  }

  .quitar-bloque:hover {
    color: var(--rb-text);
  }

  /* El placeholder no puede parecer texto escrito: lo vacío se confunde.
     Usa muted y no faint para no caer bajo el contraste mínimo. */
  .objeto textarea::placeholder,
  .objeto li input[type="text"]::placeholder {
    color: var(--rb-muted);
    font-style: italic;
  }

  .objeto textarea {
    display: block;
    width: 100%;
    height: 100%;
    padding: 18px 12px 10px;
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
    outline: 1px solid rgb(0 0 0 / 10%);
    outline-offset: -1px;
  }

  :global(:root[data-theme-base="dark"]) .objeto img {
    outline: 1px solid rgb(255 255 255 / 10%);
    outline-offset: -1px;
  }

  .objeto.lista {
    display: flex;
    flex-direction: column;
    padding: 18px 10px 6px;
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
    min-height: 28px;
  }

  .objeto li input[type="checkbox"] {
    width: 14px;
    height: 14px;
    flex: none;
    accent-color: var(--rb-ok);
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
    margin-left: -8px;
    padding: 1px 8px;
    min-height: 28px;
    border: 0;
    border-radius: var(--rb-radius-xs);
    background: none;
    color: var(--rb-muted);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background-color var(--duration-quick) var(--ease-smooth-out);
  }

  .mas:hover {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .asa {
    position: absolute;
    right: -7px;
    bottom: -7px;
    z-index: 6;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 2px solid var(--rb-surface);
    border-radius: 3px;
    background: var(--rb-text);
    cursor: nwse-resize;
    opacity: 0;
    transform: scale(0.25);
    filter: blur(4px);
    pointer-events: none;
    transition:
      opacity var(--duration-fast) var(--ease-smooth-out),
      transform var(--duration-fast) var(--ease-smooth-out),
      filter var(--duration-fast) var(--ease-smooth-out);
  }

  .asa.visible {
    opacity: 1;
    transform: none;
    filter: none;
    pointer-events: auto;
  }

  .asa::after {
    content: "";
    position: absolute;
    inset: -6px;
  }

  .cajon {
    min-width: 0;
    overflow: hidden;
    border-left: 1px solid transparent;
    transition: border-color var(--duration-slow) var(--ease-smooth-out);
  }

  .cajon.abierto {
    overflow: hidden auto;
    border-left-color: var(--rb-hairline);
  }

  .cajon-cuerpo {
    display: flex;
    width: var(--cajon-w);
    box-sizing: border-box;
    flex-direction: column;
    gap: 6px;
    padding: 0 var(--pad) 0 10px;
    opacity: 0;
    transform: translateX(var(--distance-base, 8px));
    filter: blur(var(--blur-small, 2px));
    pointer-events: none;
    transition:
      opacity var(--float-close-dur, var(--duration-fast)) var(--ease-smooth-out),
      transform var(--float-close-dur, var(--duration-fast)) var(--ease-smooth-out),
      filter var(--float-close-dur, var(--duration-fast)) var(--ease-smooth-out);
  }

  .cajon.abierto .cajon-cuerpo {
    opacity: 1;
    transform: none;
    filter: none;
    pointer-events: auto;
    transition:
      opacity var(--float-open-dur, var(--duration-medium)) var(--ease-smooth-out),
      transform var(--float-open-dur, var(--duration-medium)) var(--ease-smooth-out),
      filter var(--float-open-dur, var(--duration-medium)) var(--ease-smooth-out);
  }

  /* Pestañas del cajón: pegadas arriba, el listado scrollea debajo. */

  /*
   * Dos columnas y no cuatro: en 168px de cajón cada pestaña quedaba en 42 y
   * los nombres salían cortados («Tex…», «Re…»). En dos filas entran enteros.
   */

  /*
   * Envoltorio de cada rama del cajón.
   *
   * La transición de panel necesita un elemento que entre y salga, y el reparto
   * es el de `.pila` para que cambiar de pestaña no mueva un píxel.
   */
  .hoja {
    display: flex;
    min-height: 0;
    flex-direction: column;
    gap: 4px;
  }

  .cajon .fuentes {
    position: sticky;
    top: 0;
    z-index: 1;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 3px;
    margin: 0 -2px 4px;
    padding: 2px 0 4px;
    background: var(--rb-bg1);
    border-bottom: 1px solid var(--rb-hairline);
  }

  .fuente {
    min-width: 0;
    padding: 4px 2px;
    border: 0;
    border-radius: var(--rb-radius-xs);
    background: none;
    color: var(--rb-muted);
    font: inherit;
    font-size: 10px;
    font-weight: 600;
    line-height: 1.2;
    text-align: center;
    white-space: nowrap;
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background-color var(--duration-quick) var(--ease-smooth-out);
  }

  .fuente:hover:not(.activa) {
    color: var(--rb-text);
  }

  .fuente.activa {
    color: var(--rb-text);
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
  }

  .fuente:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .recorte-nombre {
    display: block;
    margin-bottom: 2px;
    color: var(--rb-text);
    font-size: 10.5px;
    font-weight: 600;
    -webkit-line-clamp: 1;
    line-clamp: 1;
  }

  /* Paleta flotante: fuera de flujo para no mover la barra al abrir. */
  .paleta-colores {
    position: absolute;
    top: calc(100% + 8px);
    z-index: 10;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    max-width: 232px;
    padding: 8px;
    border: 1px solid var(--rb-hairline);
    border-radius: var(--rb-radius-sm);
    background: var(--rb-surface);
    box-shadow: 0 8px 24px rgb(0 0 0 / 22%);
  }

  /*
   * El color va en un pseudo, como en Annotate: el anillo de selección
   * tiene que pintarse por fuera de la muestra.
   */
  .lapiz {
    position: relative;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: var(--rb-radius-xs);
    background: transparent;
    cursor: pointer;
    transition: transform var(--duration-fast) var(--ease-smooth-out);
  }

  .lapiz::after {
    position: absolute;
    inset: 5px;
    border-radius: 999px;
    background: var(--tinta);
    box-shadow: inset 0 0 0 1px rgb(0 0 0 / 25%);
    content: "";
    transition: inset var(--duration-fast) var(--ease-smooth-out);
  }

  .lapiz.elegido::after {
    inset: 3px;
    box-shadow:
      inset 0 0 0 1px rgb(0 0 0 / 25%),
      0 0 0 2px var(--rb-surface),
      0 0 0 3px var(--rb-text);
  }

  .lapiz:hover:not(.elegido)::after {
    inset: 4px;
  }

  .lapiz:active {
    transform: scale(0.96);
  }

  .lapiz-libre {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1.5px dashed color-mix(in sRGB, var(--rb-text) 35%, transparent);
    cursor: pointer;
  }

  .lapiz-libre span {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: 999px;
    background: var(--rb-surface);
    color: var(--rb-muted);
    font-size: 12px;
    line-height: 1;
  }

  .lapiz-libre:focus-within {
    box-shadow: var(--rb-focus);
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
    outline: 1px solid rgb(0 0 0 / 10%);
    outline-offset: -1px;
  }

  :global(:root[data-theme-base="dark"]) .recorte img {
    outline: 1px solid rgb(255 255 255 / 10%);
    outline-offset: -1px;
  }

  .hueco {
    margin: 0;
    color: var(--rb-muted);
    font-size: 11.5px;
  }

  @media (prefers-reduced-motion: reduce) {
    .girando,
    .mini,
    .mini-x,
    .cuerpo,
    .cajon,
    .cajon-cuerpo,
    .cajon.abierto .cajon-cuerpo,
    .paleta-colores,
    .pastilla.lista,
    .lapiz,
    .lapiz::after,
    .recorte,
    .objeto,
    .agarre,
    .asa,
    .mas,
    .tinta-punto,
    .vista.soltando .papel {
      transition: none;
    }

    .cajon-cuerpo,
    .paleta-colores {
      opacity: 1;
      transform: none;
      filter: none;
    }

    .linea-nueva {
      animation: none;
      opacity: 1;
    }
  }
</style>
