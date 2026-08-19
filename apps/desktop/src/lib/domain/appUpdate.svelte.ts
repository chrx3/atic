/**
 * Actualizaciones de la app.
 *
 * GitHub Releases no avisa por websocket: se pregunta `latest.json` al
 * arrancar, al enfocar la ventana (con tope) y cada pocas horas.
 */
import {
  checkAppUpdate,
  friendlyUpdateError,
  installAppUpdateAndRelaunch,
  type AppUpdate,
} from "$ipc/updates";

const POLL_MS = 4 * 60 * 60 * 1000;
const FOCUS_GAP_MS = 30 * 60 * 1000;

class AppUpdateStore {
  pending = $state<AppUpdate | null>(null);
  checking = $state(false);
  downloading = $state(false);
  percent = $state<number | null>(null);
  error = $state<string | null>(null);
  /** Ya se preguntó al menos una vez (para no decir «al día» al abrir). */
  checked = $state(false);

  #lastCheck = 0;

  get version(): string | null {
    return this.pending?.version ?? null;
  }

  async check(): Promise<void> {
    if (this.checking || this.downloading) return;
    this.checking = true;
    this.error = null;
    try {
      const update = await checkAppUpdate();
      this.#lastCheck = Date.now();
      this.pending = update;
      this.checked = true;
    } catch (err) {
      this.error = friendlyUpdateError(err);
      this.checked = true;
    } finally {
      this.checking = false;
    }
  }

  async install(): Promise<void> {
    const update = this.pending;
    if (!update || this.downloading) return;
    this.downloading = true;
    this.percent = null;
    this.error = null;
    let downloaded = 0;
    let contentLength: number | null = null;
    try {
      await installAppUpdateAndRelaunch(update, (event) => {
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
    } catch (err) {
      this.error = friendlyUpdateError(err);
      this.downloading = false;
      this.percent = null;
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
