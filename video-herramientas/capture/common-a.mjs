import fs from "node:fs";
export const WALL = (f = "wall-work.png") => `html{background:url(http://127.0.0.1:1431/${f}) 0 0/100% 100% !important} body{background:transparent !important}`;
export const extra = (...names) => names.map((n) => fs.readFileSync(new URL(`./assets-a/${n}.js`, import.meta.url), "utf8")).join("\n");
export const ICON = { logo: 404, agents: 448, clipboard: 494, system: 540, color: 586, more: 632, window: 678 };
