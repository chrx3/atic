/** Editor de anotaciones: abrir, guardar, copiar y cerrar. */

import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { AnnotateOpen } from "$core/types";
import { on } from "./events";

/** Abre el editor sobre una captura del directorio de capturas. */
export const openAnnotator = (path: string) => invoke<void>("open_annotator", { path });

/**
 * Qué captura tiene que dibujar el editor, si hay alguna.
 *
 * Es la vía fiable, y el evento `annotate-open` solo el atajo: un WebView2
 * oculto está backgrounded y el evento emitido antes de mostrar la ventana no
 * llega nunca. Ver el porqué en `annotate.rs`.
 */
export const pendingAnnotation = () =>
  invoke<AnnotateOpen | null>("pending_annotation");

/**
 * La captura como data URL.
 *
 * No se usa `convertFileSrc` como en el estante: un `<img>` de otro origen
 * contamina el canvas y `toDataURL` deja de funcionar, así que copiar o
 * guardar fallaría recién al final. Ver el porqué en `annotate.rs`.
 */
export const annotationImage = (path: string) =>
  invoke<string>("annotation_image", { path });

/** Abre la pizarra: congela la pantalla y dibuja encima, donde está. */
export const startBoard = () => invoke<void>("start_board");

export const closeAnnotator = () => invoke<void>("close_annotator");

/**
 * Guarda lo dibujado como una captura nueva y devuelve su ruta.
 *
 * `data` es lo que da `canvas.toDataURL("image/png")`; Rust acepta el data URL
 * entero para no repartir el mismo recorte entre los dos lados.
 */
export const saveAnnotation = (data: string) =>
  invoke<string>("save_annotation", { data });

export const copyAnnotation = (data: string) =>
  invoke<void>("copy_annotation", { data });

export const onAnnotateOpen = (
  cb: (payload: AnnotateOpen) => void,
): Promise<UnlistenFn> => on("annotate-open", cb);
