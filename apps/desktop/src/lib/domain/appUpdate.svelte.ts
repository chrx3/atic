/**
 * Actualizaciones de la app.
 *
 * GitHub Releases no avisa por websocket: se pregunta `latest.json` al
 * arrancar, al enfocar la ventana (con tope) y cada pocas horas.
 *
 * Descarga e instalación van aparte: el evento Finished del plugin es
 * «archivo en disco», no «ya quedó instalado».
 *
 * Consultar no debe bloquear Descargar: un `check()` lento o colgado dejaba
 * `checking` en true y el clic de la gota / el botón no hacía nada.
 */
import { t } from "./i18n.svelte";
import {
  applyAppUpdateAndRelaunch,
  checkAppUpdate,
  downloadAppUpdate,
  friendlyUpdateError,
  type AppUpdate,
} from "$ipc/updates";

const POLL_MS = 4 * 60 * 60 * 1000;
const FOCUS_GAP_MS = 30 * 60 * 1000;

class AppUpdateStore {
  /** Recurso de Tauri: no proxyar, tiene rid y bytes descargados. */
  pending = $state.raw<AppUpdate | null>(null);
  checking = $state(false);
  downloading = $state(false);
  downloaded = $state(false);
  installing = $state(false);
  percent = $state<number | null>(null);
  error = $state<string | null>(null);
  /** Ya se preguntó al menos una vez (para no decir «al día» al abrir). */
  checked = $state(false);
  /**
   * Versión de mentira (solo DEV). En `tauri dev` no se sondea GitHub: el
   * instalador de un release no es el binario que estás corriendo. Esto pinta
   * el chip / la gota para revisar layout, sin tocar el updater.
   */
  previewVersion = $state<string | null>(null);

  #lastCheck = 0;
  #checkGen = 0;

  get version(): string | null {
    return this.pending?.version ?? this.previewVersion;
  }

  /** Hay aviso que pintar: update real o simulación de DEV. */
  get visible(): boolean {
    return this.pending != null || this.previewVersion != null;
  }

  /** Solo mientras se escribe el instalador. Consultar GitHub no cuenta. */
  get busy(): boolean {
    if (this.previewVersion && !this.pending) return false;
    return this.downloading || this.installing;
  }

  async check(opts?: { force?: boolean }): Promise<void> {
    if (this.busy || this.checking) return;
    if (!opts?.force) {
      if (this.pending) return;
      if (this.checked && Date.now() - this.#lastCheck < FOCUS_GAP_MS) return;
    }

    const gen = ++this.#checkGen;
    this.checking = true;
    this.error = null;
    try {
      const update = await checkAppUpdate();
      if (gen !== this.#checkGen) {
        void update?.close();
        return;
      }
      this.#lastCheck = Date.now();
      this.checked = true;
      if (this.busy || this.downloaded) {
        void update?.close();
        return;
      }
      if (update?.version && update.version === this.pending?.version) {
        void update.close();
        return;
      }
      const previous = this.pending;
      this.pending = update;
      this.downloaded = false;
      void previous?.close();
    } catch (err) {
      if (gen !== this.#checkGen) return;
      this.checked = true;
      this.#lastCheck = Date.now();
      this.error = friendlyUpdateError(err, {
        timeout: t("about.checkTimeout"),
        fetch: t("about.checkFetch"),
      });
    } finally {
      if (gen === this.#checkGen) this.checking = false;
    }
  }

  /** Un clic: primero baja, después instala. */
  async advance(): Promise<void> {
    if (this.previewVersion && !this.pending) {
      this.#previewAdvance();
      return;
    }
    if (this.downloaded) {
      await this.apply();
      return;
    }
    await this.download();
  }

  /**
   * Pinta (o apaga) un aviso de update ficticio. En DEV: Ctrl+Alt+U.
   * El clic recorre las fases (bajar → listo → instalar) sin llamar a GitHub.
   */
  simulate(version: string | null): void {
    this.previewVersion = version;
    this.downloading = false;
    this.downloaded = false;
    this.installing = false;
    this.percent = null;
    this.error = null;
  }

  #previewAdvance(): void {
    if (this.installing) {
      this.installing = false;
      this.downloaded = false;
      this.percent = null;
      return;
    }
    if (this.downloaded) {
      this.installing = true;
      return;
    }
    if (this.downloading) {
      this.downloading = false;
      this.downloaded = true;
      this.percent = 100;
      return;
    }
    this.downloading = true;
    this.percent = 42;
  }

  async download(): Promise<void> {
    const update = this.pending;
    if (!update || this.busy || this.downloaded) return;
    this.downloading = true;
    this.percent = null;
    this.error = null;
    let downloaded = 0;
    let contentLength: number | null = null;
    try {
      await downloadAppUpdate(update, (event) => {
        if (event.event === "Started") {
          contentLength = event.data.contentLength ?? null;
          this.percent = 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          this.percent =
            contentLength && contentLength > 0
              ? Math.round((downloaded / contentLength) * 100)
              : null;
        } else if (event.event === "Finished") {
          this.percent = 100;
        }
      });
      this.downloaded = true;
    } catch (err) {
      this.error = friendlyUpdateError(err, {
        timeout: t("about.checkTimeout"),
        fetch: t("about.checkFetch"),
      });
      this.downloaded = false;
    } finally {
      this.downloading = false;
    }
  }

  async apply(): Promise<void> {
    const update = this.pending;
    if (!update || !this.downloaded || this.busy) return;
    this.installing = true;
    this.error = null;
    try {
      await applyAppUpdateAndRelaunch(update);
    } catch (err) {
      this.error = friendlyUpdateError(err, {
        timeout: t("about.checkTimeout"),
        fetch: t("about.checkFetch"),
      });
      this.installing = false;
    }
  }

  /** Arranca el sondeo. El `$effect` que lo llama debe devolver este teardown. */
  startPolling(): () => void {
    void this.check();
    const tick = window.setInterval(() => void this.check(), POLL_MS);
    const onFocus = () => {
      if (Date.now() - this.#lastCheck < FOCUS_GAP_MS) return;
      void this.check();
    };
    window.addEventListener("focus", onFocus);
    return () => {
      this.#checkGen += 1;
      this.checking = false;
      window.clearInterval(tick);
      window.removeEventListener("focus", onFocus);
    };
  }
}

export const appUpdate = new AppUpdateStore();

if (import.meta.env.DEV && typeof localStorage !== "undefined") {
  if (localStorage.getItem("atic-fake-update") !== "0") {
    appUpdate.simulate("0.4.25");
  }
}
