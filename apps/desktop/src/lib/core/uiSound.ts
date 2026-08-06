/** Toques de UI desde el front (rueda de la pill, etc.). */

import { invoke } from "@tauri-apps/api/core";

/** Mínimo entre ticks: evita un chorro si el scroll manda varios pasos seguidos. */
const TICK_GAP_MS = 55;

let lastWheelTick = 0;

/** Click seco al cambiar de herramienta en la rueda. Respeta `ui_sounds`. */
export function playWheelTick(): void {
  const now = performance.now();
  if (now - lastWheelTick < TICK_GAP_MS) return;
  lastWheelTick = now;
  void invoke("play_ui_sound", { action: "wheel_tick" }).catch(() => {
    /* sin backend (tests / preview web) */
  });
}
