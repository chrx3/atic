/**
 * La pill y la lámina que la hospeda.
 *
 * El overlay cubre el escritorio virtual y por defecto deja pasar el mouse.
 * Casi todo lo de acá existe porque el webview no puede contestar solo: no ve
 * los monitores, no recibe eventos de puntero mientras es click-through, y no
 * decide si su ventana acepta el teclado.
 */

import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { on } from "./events";

export type Point = { x: number; y: number };
export type Area = { x: number; y: number; w: number; h: number };
/** Una zona del overlay que sí recibe el mouse. El `id` viaja a Rust. */
export type HitRect = { id: string; x: number; y: number; w: number; h: number };

export const setPillVisible = (visible: boolean) =>
  invoke<void>("set_pill_visible", { visible });

/** Dónde tiene que arrancar la pill, en px CSS del overlay. */
export const pillHome = () => invoke<Point | null>("pill_home");

/** Guarda dónde quedó la pill tras arrastrarla, en px CSS del overlay. */
export const savePillHome = (x: number, y: number) =>
  invoke<void>("save_pill_home", { x, y });

/** Guarda el hogar de la pill sin moverla. Llamar ANTES de redimensionar: si
 *  no, se guarda la posición ya desplazada por el pivote y la pill deriva. */
export const stashPillHome = () => invoke<void>("stash_pill_home");

/** Encoge y vuelve al hogar en un solo movimiento. `false` = no había hogar. */
export const morphPillHome = (width: number, height: number) =>
  invoke<boolean>("morph_pill_home", { width, height });

export const restorePillPosition = () => invoke<boolean>("restore_pill_position");

/** Vuela la pill al cursor y fija ahí su hogar. Se llama al terminar de
 *  colapsar: antes de eso el ancla se calcularía con el tamaño del panel. */
export const summonPillHere = () => invoke<void>("summon_pill_here");

/** Manda una traza al log de Rust, para verla junto a las de geometría. */
export const pillTrace = (msg: string) => invoke<void>("pill_trace", { msg });

/**
 * Dónde está el puntero, en px CSS del overlay.
 *
 * El webview no puede saberlo por su cuenta: mientras el overlay está en
 * click-through no recibe ningún evento de puntero, así que lo último que
 * conoce el DOM es viejo o no existe.
 */
export const overlayCursor = () => invoke<Point | null>("overlay_cursor");

/** Las áreas útiles de cada monitor, en px CSS del overlay. */
export const overlayWorkAreas = () => invoke<Area[]>("overlay_work_areas");

/**
 * Qué partes del overlay reciben el mouse, en px CSS del overlay.
 *
 * Sin esto la lámina sería invisible pero taparía el escritorio entero. Rust
 * arma la ventana solo cuando el cursor entra en una de estas zonas.
 *
 * Van en el espacio del overlay —el escritorio virtual— y no en el del
 * viewport: Rust mueve la ventana al atender esta llamada, así que un número
 * relativo a ella se traduce con un origen que ya cambió.
 */
export const setOverlayHitRects = (rects: HitRect[]) =>
  invoke<void>("set_overlay_hit_rects", { rects });

/** Click-through total mientras dura un OLE drag hacia otra app. */
export const setOverlayItemDrag = (on: boolean) =>
  invoke<void>("set_overlay_item_drag", { on });

/** Redimensiona una ventana flotante. `up` dice si creció hacia arriba. */
export const resizeFloating = (args: {
  label: string;
  width: number;
  height: number;
  anchor?: string;
  animate?: boolean;
}) => invoke<{ up: boolean }>("resize_floating", args);

/**
 * Deja que el overlay reciba el teclado, o se lo vuelve a quitar.
 *
 * El overlay nace inactivable para no robarle el foco a la app en la que estés
 * escribiendo, y el precio es que tampoco puede recibir teclas. Se pide al
 * entrar a un campo de texto y se devuelve al salir; entre medio, la app de
 * abajo pierde el foco, así que el modo dura lo que dura la escritura.
 */
export const setOverlayTextMode = (on_: boolean) =>
  invoke<void>("set_overlay_text_mode", { on: on_ });

/**
 * Mostrar u ocultar la pill.
 *
 * Antes era `show()`/`hide()` sobre su ventana. Ahora es un evento: el overlay
 * monta o desmonta el componente, y al desmontarlo deja de publicar su zona
 * viva — que es lo que de verdad hace que la pill "no esté", porque el overlay
 * vuelve a ser transparente al mouse ahí.
 */
export const onPillVisibility = (cb: (visible: boolean) => void): Promise<UnlistenFn> =>
  on("pill-visibility", cb);

/**
 * El usuario hizo clic fuera de todas las zonas vivas del overlay.
 *
 * Reemplaza al `blur` de la ventana: el overlay abarca la pantalla y casi nunca
 * tiene el foco, así que ese evento dejó de significar «el usuario se fue». Lo
 * detecta Rust, que ve todos los clics por Raw Input.
 */
export const onOverlayDismiss = (cb: () => void): Promise<UnlistenFn> =>
  on("overlay-dismiss", cb);

export const onPillClipboardToggle = (cb: () => void): Promise<UnlistenFn> =>
  on("pill-clipboard-toggle", cb);
export const onPillClipboardClose = (cb: () => void): Promise<UnlistenFn> =>
  on("pill-clipboard-close", cb);
export const onPillSnippetsToggle = (cb: () => void): Promise<UnlistenFn> =>
  on("pill-snippets-toggle", cb);
export const onPillSnippetsClose = (cb: () => void): Promise<UnlistenFn> =>
  on("pill-snippets-close", cb);

/** Traer pill / reset: cierra clipboard y vuelve a estado compacto. */
export const onPillReset = (cb: () => void): Promise<UnlistenFn> =>
  on("pill-reset", cb);

/** Rueda de la pill: la tecla se mantiene presionada (abre). */
export const onPillRadialPress = (cb: () => void): Promise<UnlistenFn> =>
  on("pill-radial-press", cb);

/** Rueda de la pill: la tecla se soltó (activa la selección y cierra). */
export const onPillRadialRelease = (cb: () => void): Promise<UnlistenFn> =>
  on("pill-radial-release", cb);
