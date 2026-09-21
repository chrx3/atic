/**
 * Avisos del equipo: lo que el vigilante de Rust encontró.
 *
 * Vive aparte del store del panel a propósito. El panel solo existe mientras
 * está abierto; esto tiene que estar vivo siempre, porque la gracia del aviso
 * es justamente no depender de que alguien abra nada.
 */
import {
  onSystemAlert,
  systemAlerts as leerAvisos,
  type SystemAlert,
  type SystemAlertKind,
} from "$ipc/system";

class SystemAlertsStore {
  list = $state<SystemAlert[]>([]);
  /**
   * Avisos que el usuario ya vio y bajó.
   *
   * No se apagan solos con el tiempo: se apagan cuando el problema se va. Un
   * aviso que vuelve a aparecer solo a los cinco minutos, con el equipo igual
   * de ahogado, es ruido.
   */
  private silenciados = $state<Partial<Record<SystemAlertKind, true>>>({});
  private arrancado = false;

  async init(): Promise<void> {
    if (this.arrancado) return;
    this.arrancado = true;
    void onSystemAlert((avisos) => this.apply(avisos));
    try {
      this.apply(await leerAvisos());
    } catch {
      /* backend viejo: sin avisos y ya */
    }
  }

  private apply(avisos: SystemAlert[]): void {
    this.list = avisos;
    // Lo que dejó de estar encendido puede volver a avisar más adelante.
    const vivos = new Set(avisos.map((a) => a.kind));
    const quedan: Partial<Record<SystemAlertKind, true>> = {};
    for (const kind of Object.keys(this.silenciados) as SystemAlertKind[]) {
      if (vivos.has(kind)) quedan[kind] = true;
    }
    this.silenciados = quedan;
  }

  /** El peor aviso a la vista: el que más se pasó de su umbral. */
  get top(): SystemAlert | null {
    let peor: SystemAlert | null = null;
    for (const aviso of this.list) {
      if (this.silenciados[aviso.kind]) continue;
      if (!peor || aviso.value - aviso.threshold > peor.value - peor.threshold) {
        peor = aviso;
      }
    }
    return peor;
  }

  /** Lo vi: baja el chip hasta que el problema pase y vuelva. */
  dismiss(kind: SystemAlertKind): void {
    this.silenciados = { ...this.silenciados, [kind]: true };
  }
}

export const systemAlerts = new SystemAlertsStore();
