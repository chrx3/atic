/** Permisos TCC de macOS: estado y solicitud desde la UI. */

import { invoke } from "@tauri-apps/api/core";

export type MacPermissionKind = "microphone" | "screen_recording" | "accessibility";

/** `AVAuthorizationStatus` del micrófono. */
export type MicrophoneStatus =
  "not_determined" | "restricted" | "denied" | "authorized";

export interface MacPermissions {
  microphone: MicrophoneStatus;
  /** Capturas, pizarra y audio del sistema. */
  screen_recording: boolean;
  /** Pegado sintético y cuentagotas. */
  accessibility: boolean;
}

export const macPermissionsStatus = () =>
  invoke<MacPermissions>("mac_permissions_status");

/**
 * Pide un permiso y devuelve si quedó concedido en el momento.
 * Accesibilidad no se concede desde código: el comando muestra el diálogo y
 * abre Ajustes; el switch lo activa el usuario.
 */
export const macRequestPermission = (kind: MacPermissionKind) =>
  invoke<boolean>("mac_request_permission", { kind });

/** Abre el panel de Privacidad y seguridad del permiso. */
export const macOpenPrivacyPane = (kind: MacPermissionKind) =>
  invoke<void>("mac_open_privacy_pane", { kind });
