import http from "node:http"; import fs from "node:fs"; import path from "node:path";
const root = path.dirname(new URL(import.meta.url).pathname.slice(1));
const types = { ".png": "image/png", ".jpg": "image/jpeg", ".svg": "image/svg+xml", ".json": "application/json", ".js": "text/javascript", ".webm": "video/webm", ".mp4": "video/mp4" };
http.createServer((q, r) => { const f = path.join(root, decodeURIComponent(q.url.split("?")[0])); fs.readFile(f, (e, b) => { if (e) { r.writeHead(404); return r.end(); } r.writeHead(200, { "Content-Type": types[path.extname(f)] || "application/octet-stream", "Access-Control-Allow-Origin": "*" }); r.end(b); }); }).listen(1431);
