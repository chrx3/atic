/** El canal con la app del celular (`phone_sync.rs`). */

import { invoke } from "@tauri-apps/api/core";

export type PhoneDevice = {
  id: string;
  name: string;
  /** Epoch en milisegundos. */
  pairedAt: number;
  connected: boolean;
};

export type PhoneStatus = {
  /** El canal está abierto (hay celulares pareados o se pidió un QR). */
  running: boolean;
  devices: PhoneDevice[];
  /** Compartir el portapapeles de texto con los celulares. */
  clipboard: boolean;
  /** Un celular escaneó el QR y espera que lo acepten acá. */
  pendingPair: { deviceId: string; deviceName: string } | null;
};

export type PhonePairing = {
  /** Lo que codifica el QR (`atic1:...`); también sirve para pegarlo. */
  ticket: string;
  qrSvg: string;
  expiresInSecs: number;
};

export const phoneStatus = () => invoke<PhoneStatus>("phone_status");
export const phonePairStart = () => invoke<PhonePairing>("phone_pair_start");
export const phonePairCancel = () => invoke<void>("phone_pair_cancel");
export const phoneUnpair = (deviceId: string) => invoke<void>("phone_unpair", { deviceId });
export const phonePairAnswer = (deviceId: string, accept: boolean) =>
  invoke<void>("phone_pair_answer", { deviceId, accept });
export const phoneSetClipboard = (enabled: boolean) =>
  invoke<void>("phone_set_clipboard", { enabled });
