/** Estado del panel de sistema. Solo vive mientras el panel está abierto. */

import type {
  AlertSettings,
  SystemAudio,
  SystemDisplay,
  SystemSnapshot,
} from "$ipc/system";
import {
  setSystemAlertSettings,
  setSystemAwake,
  systemAction,
  systemAlertSettings,
  systemAudio,
  systemAwake,
  systemCloseApp,
  systemDisplays,
  systemFocusApp,
  systemForceApp,
  systemSetBrightness,
  systemSetMuted,
  systemSetSessionVolume,
  systemSetVolume,
  systemSnapshot,
} from "$ipc/system";
import { hasBackground, visibleApps, type SystemSort } from "$core/systemList";

export type SystemTab = "resources" | "audio" | "display" | "alerts";
export type { SystemSort };

/**
 * Último valor gana.
 *
 * Un slider dispara un `input` por píxel y cada uno era un IPC; el brillo
 * encima entra a un framework del sistema. Acá el arrastre entero se convierte
 * en "el valor más nuevo, en cuanto el anterior conteste".
 */
class UltimoGana {
  private ultimo = new Map<string, number>();
  private enVuelo = new Set<string>();

  async push(key: string, value: number, send: (v: number) => Promise<void>) {
    this.ultimo.set(key, value);
    if (this.enVuelo.has(key)) return;
    this.enVuelo.add(key);
    try {
      let v = this.ultimo.get(key);
      while (v !== undefined) {
        this.ultimo.delete(key);
        await send(v);
        v = this.ultimo.get(key);
      }
    } catch {
      this.ultimo.delete(key);
    } finally {
      this.enVuelo.delete(key);
    }
  }
}

class SystemStore {
  snapshot = $state<SystemSnapshot | null>(null);
  audio = $state<SystemAudio | null>(null);
  displays = $state<SystemDisplay[]>([]);
  tab = $state<SystemTab>("resources");
  sort = $state<SystemSort>("cpu");
  /** Filtro de la lista. Con texto escrito, el segundo plano se muestra igual. */
  query = $state("");
  /** Mostrar procesos sin ventana: apagado, porque son decenas. */
  showBackground = $state(false);
  loading = $state(true);
  error = $state<string | null>(null);
  /** Fila con una acción en curso: la vista la muestra apagándose. */
  pending = $state<Record<string, true>>({});
  /** Café: el equipo no se duerme mientras esté puesto. */
  awake = $state(false);
  /** Umbrales de los avisos, como están guardados. */
  alerts = $state<AlertSettings>({ enabled: true, cpu: 85, ram: 90, seconds: 120 });

  private cola = new UltimoGana();
  /** Espera antes de escribir los umbrales en disco (ver `saveAlerts`). */
  private alertsTimer = 0;

  get apps() {
    return visibleApps(this.snapshot?.apps ?? [], {
      sort: this.sort,
      query: this.query,
      background: this.showBackground,
    });
  }

  /** ¿Hay algo detrás del interruptor? Si no, no se ofrece. */
  get hasBackground() {
    return hasBackground(this.snapshot?.apps ?? []);
  }

  /**
   * Refresca SOLO lo que la pestaña a la vista necesita.
   *
   * Antes esto leía las tres cosas cada segundo: enumerar los procesos de la
   * máquina mientras mirabas el volumen. Recursos es lo único que cambia solo;
   * audio y pantallas se leen al entrar y cuando el usuario toca algo.
   */
  async hydrate(tab: SystemTab = this.tab): Promise<void> {
    try {
      if (tab === "resources") {
        this.snapshot = await systemSnapshot();
      } else if (tab === "audio") {
        this.audio = await systemAudio();
      } else if (tab === "display") {
        this.displays = await systemDisplays();
      } else {
        // Avisos enseña el valor de ahora al lado del umbral.
        this.snapshot = await systemSnapshot();
      }
      this.error = null;
    } catch (err) {
      this.error = err instanceof Error ? err.message : String(err);
    } finally {
      this.loading = false;
    }
  }

  /** Café y umbrales: se leen al abrir y no cambian solos. */
  async hydratePrefs(): Promise<void> {
    const [awake, alerts] = await Promise.all([
      systemAwake().catch(() => false),
      systemAlertSettings().catch(() => null),
    ]);
    this.awake = awake;
    if (alerts) this.alerts = alerts;
  }

  async setAwake(on: boolean): Promise<void> {
    this.awake = on;
    try {
      await setSystemAwake(on);
    } catch {
      this.awake = !on;
    }
  }

  /**
   * Guarda los umbrales. El vigilante los relee en el siguiente latido.
   *
   * La vista cambia al instante; el disco espera. Arrastrando un slider esto
   * se llama en cada cuadro, y cada llamada reescribe el archivo de config:
   * sin la espera, mover el umbral de 85 a 60 son sesenta escrituras.
   */
  saveAlerts(next: Partial<AlertSettings>): void {
    const merged = { ...this.alerts, ...next };
    this.alerts = merged;
    window.clearTimeout(this.alertsTimer);
    this.alertsTimer = window.setTimeout(() => {
      void setSystemAlertSettings(this.alerts).catch(() => {});
    }, 200);
  }

  /** Primera carga: las tres, para que el cambio de pestaña no parpadee. */
  async hydrateAll(): Promise<void> {
    const [snapshot, audio, displays] = await Promise.all([
      systemSnapshot().catch((err: unknown) => {
        this.error = err instanceof Error ? err.message : String(err);
        return null;
      }),
      systemAudio().catch(() => null),
      systemDisplays().catch(() => null),
    ]);
    if (snapshot) {
      this.snapshot = snapshot;
      this.error = null;
    }
    if (audio) this.audio = audio;
    if (displays) this.displays = displays;
    this.loading = false;
  }

  async setVolume(volume: number): Promise<void> {
    if (this.audio) this.audio = { ...this.audio, volume };
    await this.cola.push("master", volume, systemSetVolume);
  }

  async setMuted(muted: boolean): Promise<void> {
    await systemSetMuted(muted);
    if (this.audio) this.audio = { ...this.audio, muted };
  }

  async setSessionVolume(id: string, volume: number): Promise<void> {
    if (this.audio) {
      this.audio = {
        ...this.audio,
        sessions: this.audio.sessions.map((session) =>
          session.id === id ? { ...session, volume } : session,
        ),
      };
    }
    await this.cola.push(`session:${id}`, volume, (v) => systemSetSessionVolume(id, v));
  }

  async setBrightness(id: string, brightness: number): Promise<void> {
    this.displays = this.displays.map((display) =>
      display.id === id ? { ...display, brightness } : display,
    );
    await this.cola.push(`brightness:${id}`, brightness, (v) =>
      systemSetBrightness(id, v),
    );
  }

  /** Trae la app al frente. Es la acción por defecto de una fila. */
  async focusApp(id: string): Promise<void> {
    await systemFocusApp(id);
  }

  /** Cierra por las buenas. Devuelve a cuántos procesos se les pidió. */
  async closeApp(id: string): Promise<number> {
    this.pending = { ...this.pending, [id]: true };
    try {
      const n = await systemCloseApp(id);
      await this.hydrate("resources");
      return n;
    } finally {
      const { [id]: _, ...resto } = this.pending;
      this.pending = resto;
    }
  }

  async forceApp(id: string): Promise<void> {
    this.pending = { ...this.pending, [id]: true };
    try {
      await systemForceApp(id);
      await this.hydrate("resources");
    } finally {
      const { [id]: _, ...resto } = this.pending;
      this.pending = resto;
    }
  }

  async runAction(id: "lock" | "sleep" | "mute" | "trash"): Promise<void> {
    await systemAction(id);
    if (id === "mute") await this.hydrate("audio");
  }
}

export const system = new SystemStore();
