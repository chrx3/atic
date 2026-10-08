import { AbsoluteFill } from "remotion";
import { Desktop, ERROR_LINES, type TermLine } from "../desk/desk";
import { AgentesCursor } from "../lib/AgentesCursor";
import { approveCenter, type Cue } from "../lib/AgentesNotchState";
import { WIN, boardToScreen, pathAt, type Waypoint } from "../lib/agentesTimeline";
import { blinkLid } from "../lib/AticMark";
import { Caption } from "../lib/Caption";
import { useStageBg } from "../lib/format";
import { Screen, SCREEN } from "../lib/Screen";
import { seg, useMs } from "../lib/time";
import { AgentesStoryEditor } from "./AgentesStoryEditor";
import { AgentesStoryNotch } from "./AgentesStoryNotch";
import { AgentesStoryWindow } from "./AgentesStoryWindow";
import { T, cameraAt } from "./agentesStoryTimeline";

/*
 * Cronología (ms de la escena; el resto de instantes vive en `T`, agentesStoryTimeline.ts):
 *   450  Ctrl+Shift+A abre la ventana de agentes     1150 clic en la tarjeta de Claude Code
 *   1600 se escribe el prompt, 2500 se envía         3750 el agente edita src/db.ts (el editor del fondo cambia)
 *   4100 pide permiso de Bash (npm test)             4750 la pill abre la cara de permiso, 5500 Aprobar
 *   5650 «12 passed» en la consola, 6150 la pill dice «Listo»
 *   6700 la ventana se retira y el editor muestra ✓ 12 passed
 */
const CX = SCREEN.w / 2;
const CLAUDE_CARD = boardToScreen(352, 388);
const APPROVE = approveCenter(CX);

/** Editor del fondo: ocupa el escritorio entero salvo la pill (arriba) y la barra de tareas. */
const EDITOR_RECT = { x: 12, y: 54, w: 476, h: 546 };
const EDITOR_TERM_H = 190;
/** Línea de db.ts que nombra el error (src/db.ts:88). */
const FAILING_LINE = 10;

/** Recorrido del cursor: aparece sobre el editor, elige Claude Code y se va. */
const PATH_TO_CARD: Waypoint[] = [
  { at: 600, x: CX + 110, y: 330 },
  { at: T.hoverCard, x: CX + 20, y: 250 },
  { at: T.click - 50, x: CLAUDE_CARD.x, y: CLAUDE_CARD.y },
];
/** Segundo recorrido: baja desde la pill hasta APROBAR y hace clic. */
const PATH_TO_APPROVE: Waypoint[] = [
  { at: T.faceOpen - 50, x: CX + 120, y: 175 },
  { at: T.approvePress - 130, x: APPROVE.x, y: APPROVE.y },
  { at: T.approvePress + 200, x: APPROVE.x, y: APPROVE.y },
];

/** Chip de la pill: trabajando → permiso → trabajando → listo. */
const CUES: Cue[] = [
  { at: T.working, state: "working" },
  { at: T.cueWaiting, state: "waiting" },
  { at: T.approve, state: "working" },
  { at: T.ready, state: "ready" },
];

/** Salida de la prueba cuando el editor la vuelve a correr: aparece línea a línea. */
const PASS_LINES: TermLine[] = [
  { t: "$ npm test", c: "cmd" },
  { t: " PASS  src/orders.test.ts", c: "ok" },
  { t: "  ✓ getOrders › devuelve los pedidos", c: "dim" },
  { t: "  ✓ getOrders › filtra por usuario", c: "dim" },
  { t: "  ✓ getOrders › ordena por fecha", c: "dim" },
  { t: "  ✓ countOrders › cuenta los pedidos", c: "dim" },
  { t: "" },
  { t: "✓ 12 passed", c: "ok" },
  { t: "  Tiempo: 3,4 s", c: "dim" },
];
const PASS_LINE_MS = 70;

const BLINKS = [700, 3300, 6400];

const editorTermLines = (ms: number): TermLine[] => {
  if (ms < T.editorPass) return ERROR_LINES;
  const shown = 1 + Math.floor((ms - T.editorPass) / PASS_LINE_MS);
  return PASS_LINES.slice(0, Math.min(PASS_LINES.length, shown));
};

/** Paso 8 de la historia: Ctrl+Shift+A, Claude Code arregla el pool y la prueba pasa. */
export const AgentesMoment = () => {
  const ms = useMs();
  const camera = cameraAt(ms);

  const toCard = pathAt(ms, PATH_TO_CARD);
  const toApprove = pathAt(ms, PATH_TO_APPROVE);
  const cardOpacity = seg(ms, 600, 150) * (1 - seg(ms, T.click + 150, 150));
  const approveOpacity = seg(ms, T.faceOpen - 50, 150) * (1 - seg(ms, T.approvePress + 170, 150));
  const cursor = approveOpacity > 0 ? toApprove : toCard;

  // La marca mira hacia el cursor mientras se ve.
  const dx = cursor.x - CX;
  const dy = cursor.y - 20;
  const dist = Math.max(1, Math.hypot(dx, dy));
  const reach = Math.min(1, Math.max(0, (dist - 3) / 22));
  const lookX = cardOpacity + approveOpacity > 0 ? (dx / dist) * 0.9 * reach : 0;
  const lookY = cardOpacity + approveOpacity > 0 ? (dy / dist) * 0.5 * reach : 0;

  const fixed = ms >= T.edit;

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Agentes" sub="Tus agentes CLI, a un atajo" keys={["Ctrl", "Shift", "A"]} />
      <Screen wallpaper={false} camera={camera}>
        <Desktop>
          <AgentesStoryEditor
            rect={EDITOR_RECT}
            fixed={fixed}
            termLines={editorTermLines(ms)}
            termH={EDITOR_TERM_H}
            highlightLine={fixed ? undefined : FAILING_LINE}
          />
          <div
            style={{
              position: "absolute",
              left: WIN.x,
              top: WIN.y,
              width: WIN.w,
              height: WIN.h + WIN.bar,
              transform: `scale(${WIN.s})`,
              transformOrigin: "0 0",
            }}
          >
            {ms >= T.winOpen && ms < T.winClose + 320 && <AgentesStoryWindow ms={ms} />}
          </div>
        </Desktop>

        <AgentesStoryNotch
          cx={CX}
          ms={ms}
          cues={CUES}
          faceOpenAt={T.faceOpen}
          faceCloseAt={T.approve}
          approveHoverAt={T.approvePress - 130}
          approvePressAt={T.approvePress}
          lookX={lookX}
          lookY={lookY}
          lid={blinkLid(ms, BLINKS)}
        />

        <AgentesCursor
          x={toCard.x}
          y={toCard.y}
          zoom={camera.z}
          opacity={cardOpacity}
          pressed={ms >= T.click && ms < T.click + 90}
        />
        <AgentesCursor
          x={toApprove.x}
          y={toApprove.y}
          zoom={camera.z}
          opacity={approveOpacity}
          pressed={ms >= T.approvePress && ms < T.approvePress + 110}
        />
      </Screen>
    </AbsoluteFill>
  );
};

/**
 * Tramos [desde, hasta] (ms) que, seguidos, cuentan la escena en 3,2 s (≈1,2×):
 * abre y elige Claude Code · envía el prompt · permiso y Aprobar · «12 passed» y «Listo» · ventana fuera y ✓ 12 passed.
 */
export const AGENTES_SEGMENTS: [number, number][] = [
  [450, 1050],
  [2300, 2750],
  [4650, 7350],
];

/** Mejor cuadro: cara de permiso de la pill con la consola detrás. */
export const AGENTES_HERO_MS = 5150;
