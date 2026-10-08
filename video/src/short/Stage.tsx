import { AbsoluteFill } from "remotion";
import { useFormat } from "../lib/format";
import { C } from "../lib/theme";

/** Escenario: degradado cálido, suelo con rejilla en perspectiva y viñeta. */
export const Stage = ({ gMs }: { gMs: number }) => {
  const format = useFormat();
  const h = format.id === "h";
  const scroll = (gMs * 0.09) % 120;
  return (
    <AbsoluteFill
      style={{
        background: h
          ? "radial-gradient(70% 90% at 72% 45%, #2a2924 0%, #171715 55%, #0a0a09 100%)"
          : "radial-gradient(90% 60% at 50% 48%, #2a2924 0%, #171715 55%, #0a0a09 100%)",
        overflow: "hidden",
      }}
    >
      <div
        style={{
          position: "absolute",
          left: "-40%",
          right: "-40%",
          bottom: 0,
          height: "58%",
          transform: "perspective(700px) rotateX(64deg)",
          transformOrigin: "50% 0",
          backgroundImage:
            "linear-gradient(rgba(240,240,234,0.10) 1.5px, transparent 1.5px), linear-gradient(90deg, rgba(240,240,234,0.10) 1.5px, transparent 1.5px)",
          backgroundSize: "120px 120px",
          backgroundPosition: `0 ${scroll}px`,
          maskImage: "linear-gradient(to bottom, transparent 0%, black 45%)",
          WebkitMaskImage: "linear-gradient(to bottom, transparent 0%, black 45%)",
          opacity: 0.55,
        }}
      />
      <AbsoluteFill
        style={{
          background: `radial-gradient(120% 90% at 50% 50%, transparent 55%, ${C.bg} 100%)`,
        }}
      />
    </AbsoluteFill>
  );
};
