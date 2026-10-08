import { open } from "./session.mjs";
import fs from "node:fs";
import path from "node:path";
const R = { x1: 196, y1: 372, x2: 884, y2: 812 };
const extraA = `Object.assign(window.__mock, {
  overlay_info: { framePath: "http://127.0.0.1:1431/wall-work.png", width: 2160, height: 2160, kind: "capture",
    candidates: [{ hwnd: 11, title: "Informe Q3 — Operaciones", left: 300, top: 600, width: 1560, height: 1280 }],
    monitors: [{ left: 0, top: 0, width: 2160, height: 2160 }] },
  capture_shelf_landing: { left: 1728, top: 1872, width: 384, height: 240 },
  complete_region_capture: "C:/cap/1.png",
});`;
const dirA = "frames/captures", dirB = "frames/captures-b";
// Part A: capture overlay, drag a region, fly to the shelf slot.
const a = await open({ route: "/capture-overlay", label: "capture-overlay", extra: extraA });
await a.hideCursor();
await a.wait(200);
await a.move(560, 330); await a.wait(100);
await a.move(430, 340); await a.wait(500);
a.startRec(dirA);
await a.hold(150);
await a.glide(R.x1, R.y1, 300);
await a.down();
await a.hold(60);
await a.glide(R.x2, R.y2, 750);
await a.hold(200);
await a.up();
await a.hold(700);
const nA = a.stopRec();
await a.close();
// Part B: the shelf with the new capture, over the live desktop.
const WALL = "html{background:url(http://127.0.0.1:1431/wall-work.png) 0 0/100% 100% !important} body{background:transparent !important} .shelf{position:fixed !important;left:856px !important;top:928px !important;width:208px !important;height:136px !important}";
const item = { id: "cap1", label: "15:04", path: "http://127.0.0.1:1431/assets-c/cap1.png", createdAtMs: Date.now(), width: 1376, height: 880 };
const b = await open({ route: "/capture-shelf", label: "capture-shelf", css: WALL, extra: `Object.assign(window.__mock, { list_recent_captures: [${JSON.stringify(item)}] });` });
await b.move(R.x2, R.y2);
await b.wait(200);
b.startRec(dirB);
await b.eval(`__emit("screenshot-created", ${JSON.stringify(item)})`);
await b.hold(350);
await b.glide(958, 992, 550);
await b.hold(1400);
const nB = b.stopRec();
// Merge B after A and encode.
for (let i = 0; i < nB; i++) fs.renameSync(path.join(dirB, String(i).padStart(5, "0") + ".jpg"), path.join(dirA, String(nA + i).padStart(5, "0") + ".jpg"));
fs.rmSync(dirB, { recursive: true, force: true });
console.log("frames", nA, nB);
await b.encode(dirA, "clips/captures.mp4");
await b.close();
