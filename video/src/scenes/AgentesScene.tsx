import { AbsoluteFill } from "remotion";
import { AgentesCursor } from "../lib/AgentesCursor";
import { AgentesNotchState, approveCenter, type Cue } from "../lib/AgentesNotchState";
import { AgentsWindow } from "../lib/AgentesWindow";
import { T, WIN, boardToScreen, cameraAt, pathAt, type Waypoint } from "../lib/agentesTimeline";
import { blinkLid } from "../lib/AticMark";
import { useStageBg } from "../lib/format";
import { Caption } from "../lib/Caption";
import { Notch } from "../lib/Notch";
import { Screen, SCREEN } from "../lib/Screen";
import { C } from "../lib/theme";
import { seg, useMs } from "../lib/time";

const CX = SCREEN.w / 2;
/** Celda «Agentes» (índice 4) de la tira abierta: 458 de ancho, celdas de 44 con 2 de hueco. */
const AGENTS_CELL = { x: CX - 229 + 4 * 46 + 22, y: 26 };
const CLAUDE_CARD = boardToScreen(352, 388);
const APPROVE = approveCenter(CX);
/** Cambio del Notch de la tira al de estados: la tira ya terminó de cerrarse. */
const SWAP_MS = 1300;

const PATH_TO_CARD: Waypoint[] = [
  { at: 250, x: CX + 90, y: 110 },
  { at: 560, x: AGENTS_CELL.x, y: AGENTS_CELL.y },
  { at: 1050, x: AGENTS_CELL.x, y: AGENTS_CELL.y },
  { at: T.click - 50, x: CLAUDE_CARD.x, y: CLAUDE_CARD.y },
];
const PATH_TO_APPROVE: Waypoint[] = [
  { at: 4700, x: CX + 120, y: 175 },
  { at: 5350, x: APPROVE.x, y: APPROVE.y },
  { at: 5700, x: APPROVE.x, y: APPROVE.y },
];

const CUES: Cue[] = [
  { at: T.working, state: "working" },
  { at: T.cueWaiting, state: "waiting" },
  { at: T.approve, state: "working" },
  { at: T.ready, state: "ready" },
];

export const AgentesScene = () => {
  const ms = useMs();
  const camera = cameraAt(ms);

  const toCard = pathAt(ms, PATH_TO_CARD);
  const toApprove = pathAt(ms, PATH_TO_APPROVE);
  const cardOpacity = seg(ms, 250, 150) * (1 - seg(ms, T.click + 150, 150));
  const approveOpacity = seg(ms, 4700, 150) * (1 - seg(ms, T.approvePress + 170, 150));
  const cursor = approveOpacity > 0 ? toApprove : toCard;

  // La marca mira hacia el cursor mientras se ve.
  const dx = cursor.x - CX;
  const dy = cursor.y - 20;
  const dist = Math.max(1, Math.hypot(dx, dy));
  const reach = Math.min(1, Math.max(0, (dist - 3) / 22));
  const lookX = (dx / dist) * 0.9 * reach;
  const lookY = (dy / dist) * 0.5 * reach;
  const lid = blinkLid(ms, [700, 4150, 6600]);

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Agentes" sub="Tus agentes CLI, a un atajo" keys={["Ctrl", "Shift", "A"]} />
      <Screen camera={camera}>
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
          {ms >= T.winOpen && <AgentsWindow ms={ms} />}
        </div>

        {ms < SWAP_MS ? (
          <Notch
            cx={CX}
            toggles={[T.stripOpen, T.winOpen]}
            hovers={[{ index: 4, fromMs: T.hoverFrom, toMs: T.winOpen + 80 }]}
            lookX={lookX}
            lookY={lookY}
            lid={lid}
          />
        ) : (
          <AgentesNotchState
            cx={CX}
            ms={ms}
            cues={CUES}
            faceOpenAt={T.faceOpen}
            faceCloseAt={T.approve}
            approveHoverAt={5350}
            approvePressAt={T.approvePress}
            lookX={lookX}
            lookY={lookY}
            lid={lid}
          />
        )}

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
