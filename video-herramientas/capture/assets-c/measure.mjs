import { connect, sleep } from "../cdp.mjs";
import fs from "node:fs";
const p = await connect();
await p.send("Emulation.setDeviceMetricsOverride", { width: 1080, height: 1080, deviceScaleFactor: 1, mobile: false });
const html = fs.readFileSync("html/wall.html", "utf8").replace("__MODE__", "work");
const f = await p.send("Page.getFrameTree");
await p.send("Page.setDocumentContent", { frameId: f.frameTree.frame.id, html });
await sleep(500);
console.log(await p.eval(`JSON.stringify({card:document.querySelector('.card').getBoundingClientRect(), bars:[...document.querySelectorAll('.bar')].map(b=>b.getBoundingClientRect()).map(r=>[r.x,r.y,r.width,r.height].map(Math.round)), p1:document.querySelector('.doc p').getBoundingClientRect(), h1:document.querySelector('h1').getBoundingClientRect(), win:document.querySelector('.win').getBoundingClientRect(),
 pct:(()=>{const n=document.querySelector('.doc p').firstChild; const i=n.textContent.indexOf('18 %'); const r=document.createRange(); r.setStart(n,i); r.setEnd(n,i+4); return r.getBoundingClientRect();})()})`));
await p.close();
