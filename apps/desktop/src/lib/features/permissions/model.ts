/** Reglas de la pantalla de permisos, sin Svelte: se pueden testear. */

import type {
  MacPermissionKind,
  MacPermissions,
  MicrophoneStatus,
} from "$ipc/permissions";

export interface PermissionRow {
  kind: MacPermissionKind;
  granted: boolean;
  /** Acción principal de la fila; `none` cuando ya está concedido. */
  action: "allow" | "settings" | "none";
}

/** ¿Falta alguno de los permisos que Atic necesita? */
export function hasMissingPermissions(status: MacPermissions): boolean {
  return (
    status.microphone !== "authorized" ||
    !status.screen_recording ||
    !status.accessibility
  );
}

/**
 * Qué puede hacer la UI con el micrófono: pedirlo solo si está sin decidir
 * (macOS no vuelve a mostrar el diálogo si ya se denegó).
 */
export function microphoneAction(
  status: MicrophoneStatus,
): "allow" | "settings" | "none" {
  if (status === "authorized") return "none";
  if (status === "not_determined") return "allow";
  return "settings";
}

/** Filas de la lista, en el orden en que se muestran. */
export function permissionRows(status: MacPermissions): PermissionRow[] {
  return [
    {
      kind: "microphone",
      granted: status.microphone === "authorized",
      action: microphoneAction(status.microphone),
    },
    {
      kind: "screen_recording",
      granted: status.screen_recording,
      action: status.screen_recording ? "none" : "allow",
    },
    {
      kind: "accessibility",
      granted: status.accessibility,
      action: status.accessibility ? "none" : "settings",
    },
  ];
}
