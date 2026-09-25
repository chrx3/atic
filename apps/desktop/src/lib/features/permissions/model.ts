/** Reglas de la pantalla de permisos, sin Svelte: se pueden testear. */

import type {
  MacPermissionKind,
  MacPermissions,
  MicrophoneStatus,
} from "$ipc/permissions";

/** Los TCC que Atic necesita más las notificaciones, que son opcionales. */
export type PermissionRowKind = MacPermissionKind | "notifications";

export interface PermissionRow {
  kind: PermissionRowKind;
  granted: boolean;
  /** Acción principal de la fila; `none` cuando ya está concedido. */
  action: "allow" | "settings" | "none";
  /** Sin él Atic funciona; solo se pierde un aviso. No cuenta como faltante. */
  optional: boolean;
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

/**
 * Filas de la lista, en el orden en que se muestran.
 *
 * `notifications` es `null` cuando no se pudo consultar (fuera de Tauri): la
 * fila opcional no aparece.
 */
export function permissionRows(
  status: MacPermissions,
  notifications: boolean | null = null,
): PermissionRow[] {
  const rows: PermissionRow[] = [
    {
      kind: "microphone",
      granted: status.microphone === "authorized",
      action: microphoneAction(status.microphone),
      optional: false,
    },
    {
      kind: "accessibility",
      granted: status.accessibility,
      action: status.accessibility ? "none" : "settings",
      optional: false,
    },
    {
      kind: "screen_recording",
      granted: status.screen_recording,
      action: status.screen_recording ? "none" : "allow",
      optional: false,
    },
  ];
  if (notifications !== null) {
    rows.push({
      kind: "notifications",
      granted: notifications,
      action: notifications ? "none" : "allow",
      optional: true,
    });
  }
  return rows;
}
