/** Compila la pill GPUI para el instalador con la pill nativa
 * (`tauri.pill.conf.json`). Solo Windows: en macOS sigue la pill de Tauri.
 */
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

if (process.platform !== "win32") {
  console.log("pill:build: la pill GPUI solo se empaqueta en Windows");
  process.exit(0);
}
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const script = path.join(root, "scripts", "build-pill.ps1");
const r = spawnSync("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script], {
  stdio: "inherit",
  cwd: root,
});
process.exit(r.status ?? 1);
