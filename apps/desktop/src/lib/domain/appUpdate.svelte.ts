/**
 * Actualizaciones de la app.
 *
 * GitHub Releases no avisa por websocket: se pregunta `latest.json` al
 * arrancar, al enfocar la ventana (con tope) y cada pocas horas.
 *
 * Descarga e instalación van aparte: el evento Finished del plugin es
 * «archivo en disco», no «ya quedó instalado».
 */
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

  #lastCheck = 0;

  get version(): string | null {
    return this.pending?.version ?? null;
  }

  get busy(): boolean {
    return this.checking || this.downloading || this.installing;
  }

  async check(): Promise<void> {
    if (this.busy || this.downloaded) return;
    this.checking = true;
    this.error = null;
    try {
      const update = await checkAppUpdate();
      this.#lastCheck = Date.now();
      const previous = this.pending;
      this.pending = update;
      this.downloaded = false;
      this.checked = true;
      void previous?.close();
    } catch (err) {
      this.error = friendlyUpdateError(err);
      this.checked = true;
    } finally {
      this.checking = false;
    }
  }

  /** Un clic: primero baja, después instala. */
  async advance(): Promise<void> {
    if (this.downloaded) {
      await this.apply();
      return;
    }
    await this.download();
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
      this.error = friendlyUpdateError(err);
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
      this.error = friendlyUpdateError(err);
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
      window.clearInterval(tick);
      window.removeEventListener("focus", onFocus);
    };
  }
}

export const appUpdate = new AppUpdateStore();
