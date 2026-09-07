/** Compila el sidecar de comandos Unix antes de `tauri build`/`dev`.
 * En Windows corre el `.ps1` con PowerShell 5.1 (no hay `pwsh` en dev);
 * en el resto, el `.sh`. Mismo estilo que `test-color-picker.mjs`.
 */
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const win = process.platform === "win32";
const script = win
  ? path.join(root, "scripts", "build-unix.ps1")
  : path.join(root, "scripts", "build-unix.sh");
const r = win
  ? spawnSync("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script], {
      stdio: "inherit",
      cwd: root,
    })
  : spawnSync("sh", [script], { stdio: "inherit", cwd: root });
process.exit(r.status ?? 1);
