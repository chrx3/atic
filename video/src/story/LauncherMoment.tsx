import { AbsoluteFill, interpolate } from "remotion";
import { AticMark, blinkLid } from "../lib/AticMark";
import { Caption } from "../lib/Caption";
import { AppWindow, BrowserApp, ChatApp, D, Desktop, type Rect } from "../desk/desk";
import { FlipCursor, type CursorKey } from "../lib/FlipCursor";
import { useStageBg } from "../lib/format";
import { LF } from "../lib/Launcher";
import { Notch } from "../lib/Notch";
import { Screen, SCREEN } from "../lib/Screen";
import { C, EASE, FONT_SANS, SHADOW_GOO, lerp } from "../lib/theme";
import { seg, useMs } from "../lib/time";
import { LauncherStory, launcherGoneMs, type LauncherStoryTimeline } from "./LauncherStory";

/**
 * Paso 9 de la historia — Apps: Ctrl+Espacio abre el launcher, «cor» encuentra «Correo»
 * y Enter lo abre: el correo del incidente aparece en el escritorio.
 */

/* ------------------------------- cronología (ms) ------------------------------- */

const FLY_MS = 650; // la pill se desprende del techo como disco
const FLY_DUR = 420;
const TL: LauncherStoryTimeline = {
  bornMs: 700, // nace la gota del launcher
  typeMs: 2700, // escribe «cor»
  enterMs: 3900, // Enter sobre «Correo»
};
const MAIL_OPEN_MS = 4050; // el correo aparece mientras el launcher se recoge
const MAIL_OPEN_DUR = 380;
const GONE_MS = launcherGoneMs(TL); // el launcher desaparece
const RETURN_MS = GONE_MS; // el disco vuelve al techo
const RETURN_DUR = 420;
const ZOOM_BACK_MS = GONE_MS - 320; // la cámara vuelve a 1×

/* ------------------------------ posiciones lógicas ------------------------------ */

// La barra nace centrada; la pill queda a su izquierda (toolSlots.ts).
const CX = SCREEN.w / 2;
const BAR_LEFT = CX - LF.barW / 2;
const BAR_TOP = 170;
const DISC = 40;
const SLOT_X = BAR_LEFT - 16 - DISC;

const BROWSER_RECT: Rect = { x: 150, y: 80, w: 340, h: 310 };
const CHAT_RECT: Rect = { x: 14, y: 300, w: 320, h: 290 };
const MAIL_RECT: Rect = { x: 44, y: 130, w: 412, h: 350 };

/** Puntero discreto: reposa a un lado y, al abrirse el correo, se acerca a leerlo. */
const POINTER_KEYS: CursorKey[] = [
  { ms: 250, x: 448, y: 585 },
  { ms: MAIL_OPEN_MS, x: 448, y: 585 },
  { ms: 5300, x: 372, y: 330 },
];

/* ---------------------------------- correo ---------------------------------- */

const MAIL_FOLDERS: [string, string | null][] = [
  ["Recibidos", "3"],
  ["Enviados", null],
  ["Borradores", null],
  ["Archivo", null],
];

const MAIL_BODY = [
  "Hola,",
  "A las 14:02 el panel empezó a responder en más de 900 ms y hay pedidos que terminan con error.",
  "Parece un problema con la base de datos. ¿Puedes confirmar si ya está controlado?",
  "Gracias,",
  "Lucía",
];

/** Ventana de correo genérica con el mensaje del incidente abierto. */
const MailWindow = ({ rect, open }: { rect: Rect; open: number }) => (
  <div
    style={{
      position: "absolute",
      inset: 0,
      opacity: open,
      transform: `translateY(${(1 - open) * 10}px) scale(${lerp(0.95, 1, open)})`,
      transformOrigin: `${rect.x + rect.w / 2}px ${rect.y + rect.h * 0.45}px`,
      pointerEvents: "none",
    }}
  >
    <AppWindow rect={rect} title="Incidente 14:02 — pool de conexiones — Correo">
      <div style={{ position: "absolute", inset: 0, display: "flex", fontFamily: FONT_SANS, color: D.ink }}>
        <div style={{ width: 92, background: D.paper2, borderRight: `1px solid ${D.line}`, padding: "10px 8px", fontSize: 8.8 }}>
          <div style={{ background: D.blue, color: "#fff", borderRadius: 6, padding: "5px 0", textAlign: "center", fontWeight: 650, marginBottom: 10 }}>
            Redactar
          </div>
          {MAIL_FOLDERS.map(([name, badge], i) => (
            <div
              key={name}
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                padding: "5px 6px",
                borderRadius: 5,
                marginBottom: 2,
                background: i === 0 ? "rgb(47 111 235 / 12%)" : "transparent",
                color: i === 0 ? D.blue : D.inkSoft,
                fontWeight: i === 0 ? 650 : 500,
              }}
            >
              {name}
              {badge && <span style={{ fontSize: 7.5 }}>{badge}</span>}
            </div>
          ))}
        </div>
        <div style={{ flex: 1, minWidth: 0, padding: "14px 16px" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <span style={{ fontSize: 7.5, fontWeight: 650, color: D.red, background: "rgb(229 72 77 / 12%)", padding: "2px 6px", borderRadius: 999 }}>
              Urgente
            </span>
          </div>
          <div style={{ fontSize: 13.5, fontWeight: 700, lineHeight: 1.25, marginTop: 7 }}>Incidente 14:02 — pool de conexiones</div>
          <div style={{ display: "flex", alignItems: "center", gap: 7, marginTop: 10, paddingBottom: 10, borderBottom: `1px solid ${D.line}` }}>
            <span style={{ width: 22, height: 22, borderRadius: 11, background: D.amber, flex: "none" }} />
            <div style={{ fontSize: 8.6, lineHeight: 1.35 }}>
              <div style={{ fontWeight: 650 }}>Lucía Fuentes</div>
              <div style={{ color: D.inkSoft }}>para mí · 14:30</div>
            </div>
          </div>
          <div style={{ marginTop: 10, display: "grid", gap: 7, fontSize: 9.4, lineHeight: 1.45, color: D.ink }}>
            {MAIL_BODY.map((line) => (
              <div key={line}>{line}</div>
            ))}
          </div>
          <div style={{ display: "flex", gap: 6, marginTop: 14 }}>
            {["Responder", "Reenviar"].map((label, i) => (
              <span
                key={label}
                style={{
                  fontSize: 8.6,
                  fontWeight: 650,
                  padding: "5px 12px",
                  borderRadius: 6,
                  background: i === 0 ? D.blue : "transparent",
                  color: i === 0 ? "#fff" : D.inkSoft,
                  boxShadow: i === 0 ? "none" : `inset 0 0 0 1px ${D.line}`,
                }}
              >
                {label}
              </span>
            ))}
          </div>
        </div>
      </div>
    </AppWindow>
  </div>
);

/* ---------------------------------- escena ---------------------------------- */

export const LauncherMoment = () => {
  const ms = useMs();

  // Zoom atrás para que quepan pill, barra y favoritos; vuelve a 1× al cerrarse el launcher.
  const zoomOut = EASE.smoothOut(seg(ms, FLY_MS, 700)) * (1 - EASE.smoothOut(seg(ms, ZOOM_BACK_MS, 700)));
  const camera = {
    z: interpolate(zoomOut, [0, 1], [1, 0.8]),
    fx: interpolate(zoomOut, [0, 1], [CX, 305]),
    fy: 0,
    ax: CX,
    ay: 0,
  };

  // El escritorio cubre siempre todo el visor, aunque la cámara se aleje de él.
  const deskStyle = {
    position: "absolute" as const,
    left: camera.fx - camera.ax / camera.z,
    top: camera.fy - camera.ay / camera.z,
    width: SCREEN.w / camera.z,
    height: SCREEN.h / camera.z,
  };

  // La pill se desprende del techo, vuela como disco al hueco izquierdo y vuelve al cerrarse.
  const fly = EASE.liquid(seg(ms, FLY_MS, FLY_DUR));
  const back = EASE.liquid(seg(ms, RETURN_MS, RETURN_DUR));
  const discX = lerp(lerp(CX - DISC / 2, SLOT_X, fly), CX - DISC / 2, back);
  const discY = lerp(lerp(0, BAR_TOP, fly), 0, back);
  const notchBack = seg(ms, RETURN_MS + RETURN_DUR - 140, 140);
  const notchOpacity = Math.max(0, 1 - seg(ms, FLY_MS, 140)) + notchBack;
  const discOpacity = seg(ms, FLY_MS, 100) * (1 - notchBack);

  const mailOpen = EASE.smoothOut(seg(ms, MAIL_OPEN_MS, MAIL_OPEN_DUR));

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Apps" sub="Abre lo que sea, sin soltar el teclado" keys={["Ctrl", "Espacio"]} />
      <Screen wallpaper={false} camera={camera}>
        <div style={deskStyle}>
          <Desktop />
        </div>
        <BrowserApp rect={BROWSER_RECT} />
        <ChatApp rect={CHAT_RECT} />
        {mailOpen > 0 && <MailWindow rect={MAIL_RECT} open={mailOpen} />}

        {notchOpacity > 0 && (
          <div style={{ opacity: Math.min(1, notchOpacity) }}>
            <Notch
              cx={CX}
              lookX={interpolate(ms, [200, 500, RETURN_MS, RETURN_MS + RETURN_DUR], [0, 0.9, 0.9, 0], {
                extrapolateLeft: "clamp",
                extrapolateRight: "clamp",
              })}
              lid={blinkLid(ms, [520])}
            />
          </div>
        )}

        {ms >= FLY_MS && discOpacity > 0 && (
          <div
            style={{
              position: "absolute",
              left: discX,
              top: discY,
              width: DISC,
              height: DISC,
              borderRadius: "50%",
              background: C.skin,
              filter: SHADOW_GOO,
              display: "grid",
              placeItems: "center",
              opacity: discOpacity,
            }}
          >
            <AticMark size={28} strokeWidth={1.5} ms={ms} lookX={0.6} />
          </div>
        )}

        {ms >= TL.bornMs && (
          <div style={{ position: "absolute", left: BAR_LEFT, top: BAR_TOP }}>
            <LauncherStory ms={ms} tl={TL} />
          </div>
        )}

        <FlipCursor ms={ms} keys={POINTER_KEYS} size={15} />
      </Screen>
    </AbsoluteFill>
  );
};

/**
 * Tramos [desde, hasta] en ms que, seguidos, la cuentan en 1.6 s (≈ 1.5×):
 * la pill sale del techo y nace el launcher, «cor» → Correo, Enter y aparece el correo.
 */
export const LAUNCHER_SEGMENTS: [number, number][] = [
  [620, 1520],
  [2650, 3350],
  [3850, 4650],
];
export const LAUNCHER_HERO_MS = 3400;
