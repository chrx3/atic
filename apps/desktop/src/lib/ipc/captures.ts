/** Capturas de pantalla: el disparo, la selección, el archivo y su OCR. */

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { CaptureItem, OverlayInfo } from "$core/types";
import { on } from "./events";

/**
 * URL mostrable para un archivo del disco.
 *
 * El webview no puede abrir un `file://` propio: Tauri lo sirve por su
 * protocolo de assets, y sin esta conversión la imagen no carga.
 */
export const captureSrc = (path: string): string => convertFileSrc(path);

export const capturePrimaryMonitor = () => invoke<string>("capture_primary_monitor");
export const listRecentCaptures = () => invoke<CaptureItem[]>("list_recent_captures");
export const deleteCapture = (path: string) => invoke<void>("delete_capture", { path });
export const copyCapturePath = (path: string) =>
  invoke<void>("copy_capture_path", { path });
export const copyCaptureImage = (path: string) =>
  invoke<void>("copy_capture_image", { path });
export const revealCapture = (path: string) => invoke<void>("reveal_capture", { path });
export const activateCapture = (path: string) =>
  invoke<void>("activate_capture", { path });
export const cleanupCapturesNow = () => invoke<number>("cleanup_captures_now");

// --- OCR ---
export const ocrCaptureText = (path: string) =>
  invoke<string>("ocr_capture_text", { path });
export const ocrCaptureAndCopy = (path: string) =>
  invoke<string>("ocr_capture_and_copy", { path });
export const readCaptureOcrCache = (path: string) =>
  invoke<string | null>("read_capture_ocr_cache", { path });

// --- Overlay de selección ---
export const startCaptureSession = () => invoke<void>("start_capture_session");
export const overlayInfo = () => invoke<OverlayInfo>("overlay_info");
export const completeWindowCapture = (hwnd: number) =>
  invoke<string>("complete_window_capture", { hwnd });
export const completeRegionCapture = (
  left: number,
  top: number,
  width: number,
  height: number,
) => invoke<string>("complete_region_capture", { left, top, width, height });
export const completeMonitorCapture = (x: number, y: number) =>
  invoke<string>("complete_monitor_capture", { x, y });
export const cancelCaptureSession = () => invoke<void>("cancel_capture_session");
/** Muestra el overlay cuando el frame congelado ya cargó (evita telón gris). */
export const showCaptureOverlay = () => invoke<void>("show_capture_overlay");

export const onScreenshotCreated = (
  cb: (item: CaptureItem) => void,
): Promise<UnlistenFn> => on("screenshot-created", cb);

export const onScreenshotShelfUpdated = (cb: () => void): Promise<UnlistenFn> =>
  on("screenshot-shelf-updated", cb);
