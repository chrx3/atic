import type { ComponentType } from "react";
import { AgentesMoment } from "./AgentesMoment";
import { ClipboardMoment } from "./ClipboardMoment";
import { ColorMoment } from "./ColorMoment";
import { FlipboardMoment } from "./FlipboardMoment";
import { LauncherMoment } from "./LauncherMoment";
import { PizarraMoment } from "./PizarraMoment";
import { TextosMoment } from "./TextosMoment";

/** Momentos de la historia como composiciones sueltas (duración natural de cada uno, en s). */
export const STORY_MOMENTS: { id: string; component: ComponentType; seconds: number }[] = [
  { id: "Story-Clipboard", component: ClipboardMoment, seconds: 6 },
  { id: "Story-Pizarra", component: PizarraMoment, seconds: 6 },
  { id: "Story-Color", component: ColorMoment, seconds: 5 },
  { id: "Story-Textos", component: TextosMoment, seconds: 6 },
  { id: "Story-Flipboard", component: FlipboardMoment, seconds: 7 },
  { id: "Story-Agentes", component: AgentesMoment, seconds: 8 },
  { id: "Story-Apps", component: LauncherMoment, seconds: 6 },
];
