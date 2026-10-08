import type { ComponentType } from "react";
import { FORMAT_SQUARE, FormatProvider } from "../lib/format";
import { AgentesMoment } from "../story/AgentesMoment";
import { CapturasMoment } from "../story/CapturasMoment";
import { ClipboardMoment } from "../story/ClipboardMoment";
import { ColorMoment } from "../story/ColorMoment";
import { FlipboardMoment } from "../story/FlipboardMoment";
import { HookMoment } from "../story/HookMoment";
import { LauncherMoment } from "../story/LauncherMoment";
import { PayoffDeskMoment } from "../story/PayoffDeskMoment";
import { PizarraMoment } from "../story/PizarraMoment";
import { StripMoment } from "../story/StripMoment";
import { TextosMoment } from "../story/TextosMoment";

const inSquare = (Moment: ComponentType) => () => (
  <FormatProvider format={FORMAT_SQUARE}>
    <Moment />
  </FormatProvider>
);

/** Cada momento a 1080×1080 a pantalla completa, para revisarlo suelto en el Studio. */
export const SQUARE_MOMENTS: { id: string; component: ComponentType; seconds: number }[] = [
  { id: "Square-Hook", component: inSquare(HookMoment), seconds: 3 },
  { id: "Square-Strip", component: inSquare(StripMoment), seconds: 3 },
  { id: "Square-Clipboard", component: inSquare(ClipboardMoment), seconds: 6 },
  { id: "Square-Pizarra", component: inSquare(PizarraMoment), seconds: 6 },
  { id: "Square-Capturas", component: inSquare(CapturasMoment), seconds: 6 },
  { id: "Square-Color", component: inSquare(ColorMoment), seconds: 5 },
  { id: "Square-Textos", component: inSquare(TextosMoment), seconds: 6 },
  { id: "Square-Flipboard", component: inSquare(FlipboardMoment), seconds: 7 },
  { id: "Square-Agentes", component: inSquare(AgentesMoment), seconds: 8 },
  { id: "Square-Apps", component: inSquare(LauncherMoment), seconds: 6 },
  { id: "Square-Payoff", component: inSquare(PayoffDeskMoment), seconds: 4 },
];
