/**
 * Iconos del sitio.
 *
 * Es el mismo catálogo de la app (`apps/desktop/src/lib/icons.ts`) recortado a
 * lo que el sitio usa. Los datos vienen de `lucide` como IconNode y los pinta
 * `Icon.svelte` — sin Morphicons, que acá no hace falta.
 */
import type { ToolId } from "$atic/lib/core/tools";
import {
  AlignLeft,
  AppWindow,
  Calculator,
  Camera,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CircleDot,
  Clipboard,
  Copy,
  Crop,
  Download,
  ExternalLink,
  FileText,
  Image,
  Keyboard,
  Lock,
  Mic,
  Minus,
  Moon,
  Pencil,
  Pipette,
  Pin,
  Plus,
  Power,
  RotateCcw,
  Search,
  Settings,
  ShieldCheck,
  Square,
  SquareTerminal,
  Star,
  Sun,
  Trash2,
  VolumeX,
  X,
  Check,
  ArrowRight,
  Eraser,
  Ellipsis,
  Circle,
  MoveUpRight,
  Undo2,
  Redo2,
  Highlighter,
  Monitor,
  Play,
  Pause,
  Sparkles,
  Type,
  ZoomIn,
} from "lucide";

/** La forma que devuelve `lucide`: [tag, attrs][] con los trazos del icono. */
export type IconNode = [tag: string, attrs: Record<string, string | number | undefined>][];

export type SettingsIconId =
  | "general"
  | "shortcuts"
  | "audio"
  | "summary"
  | "settings"
  | "about";

export type ExtraIconId = "more" | "pill" | "back" | "window";

export type AppIconId = ToolId | SettingsIconId | ExtraIconId;

export const TOOL_ICONS: Record<AppIconId, IconNode> = {
  meetings: CircleDot,
  dictation: Mic,
  agents: SquareTerminal,
  clipboard: Clipboard,
  snippets: AlignLeft,
  captures: Crop,
  board: Pencil,
  color: Pipette,
  launcher: Search,
  general: Settings,
  settings: Settings,
  shortcuts: Keyboard,
  audio: Mic,
  summary: FileText,
  about: FileText,
  more: Ellipsis,
  window: AppWindow,
  back: ChevronLeft,
  pill: Square,
} as unknown as Record<AppIconId, IconNode>;

export {
  AlignLeft,
  AppWindow,
  Calculator,
  Camera,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Circle,
  CircleDot,
  Clipboard,
  Copy,
  Crop,
  Download,
  Eraser,
  Ellipsis,
  ExternalLink,
  FileText,
  Highlighter,
  Image,
  Keyboard,
  Lock,
  Mic,
  Minus,
  Monitor,
  Moon,
  MoveUpRight,
  Pencil,
  Pipette,
  Pin,
  Play,
  Pause,
  Plus,
  Power,
  Redo2,
  RotateCcw,
  Search,
  Settings,
  ShieldCheck,
  Sparkles,
  Square,
  SquareTerminal,
  Star,
  Sun,
  Trash2,
  Undo2,
  VolumeX,
  X,
  ZoomIn,
  ArrowRight,
  Type,
};
