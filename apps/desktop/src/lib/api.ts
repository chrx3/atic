/**
 * Puente hacia `$ipc`.
 *
 * Este archivo era 659 líneas con los ~110 comandos y los ~35 eventos de toda
 * la app en una sola lista. Ahora vive repartido por dominio en `lib/ipc/`, y
 * lo que queda acá es la superficie vieja para que el árbol previo a la
 * reescritura siga compilando sin tocar cuarenta archivos de imports.
 *
 * Se borra en la fase 9, cuando ya nadie importe `$lib/api`.
 */

export * from "./ipc/agents";
export * from "./ipc/captures";
export * from "./ipc/clipboard";
export * from "./ipc/config";
export * from "./ipc/dictation";
export * from "./ipc/models";
export * from "./ipc/overlay";
export * from "./ipc/paste";
export * from "./ipc/recordings";
export * from "./ipc/search";
export * from "./ipc/snippets";
export * from "./ipc/summaries";
export * from "./ipc/transcripts";
export * from "./ipc/updates";

import { openDataDir } from "./ipc/config";

/** @deprecated Prefer `openDataDir("captures")`. */
export const openCapturesDir = () => openDataDir("captures");
