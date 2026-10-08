import type { ReactNode } from "react";
import { AbsoluteFill } from "remotion";
import { camTransform, useCam3D } from "./cam3d";
import { useFormat } from "./format";
import { C, FONT_SANS } from "./theme";

/** Pantalla lógica (px de la app). Se pinta a 2× dentro del cuadro de video. */
export const SCREEN = { w: 500, h: 660, scale: 2 } as const;

export type Camera = {
  /** Zoom extra sobre la escala base. */
  z: number;
  /** Punto lógico que se quiere enfocar... */
  fx: number;
  fy: number;
  /** ...y dónde debe quedar en la pantalla (también lógico). */
  ax: number;
  ay: number;
};

export const CAMERA_HOME: Camera = { z: 1, fx: 0, fy: 0, ax: 0, ay: 0 };

const Wallpaper = () => (
  <AbsoluteFill
    style={{
      background:
        "radial-gradient(120% 90% at 20% 0%, #e9e3d6 0%, #cfd3d1 45%, #aeb9c2 100%)",
    }}
  >
    {/* ventana de fondo, apagada, para que la pill oscura tenga contra qué leerse */}
    <div
      style={{
        position: "absolute",
        left: 34,
        top: 70,
        width: 432,
        height: 520,
        borderRadius: 12,
        background: "rgb(255 255 255 / 62%)",
        boxShadow: "0 24px 60px rgb(20 24 30 / 22%)",
        overflow: "hidden",
      }}
    >
      <div
        style={{
          height: 30,
          background: "rgb(255 255 255 / 70%)",
          display: "flex",
          alignItems: "center",
          gap: 6,
          paddingLeft: 12,
        }}
      >
        {["#ff5f57", "#febc2e", "#28c840"].map((c) => (
          <span key={c} style={{ width: 9, height: 9, borderRadius: 9, background: c, opacity: 0.8 }} />
        ))}
      </div>
      <div style={{ padding: "18px 22px", display: "grid", gap: 10 }}>
        {[88, 64, 92, 40, 76, 58, 84, 30, 70, 52, 90, 44].map((w, i) => (
          <div
            key={i}
            style={{
              height: 8,
              width: `${w}%`,
              borderRadius: 4,
              background: i % 5 === 3 ? "rgb(60 70 80 / 22%)" : "rgb(60 70 80 / 13%)",
            }}
          />
        ))}
      </div>
    </div>
  </AbsoluteFill>
);

/** Monitor: marco redondeado con el escritorio y la cámara sobre el contenido. */
export const Screen = ({
  camera = CAMERA_HOME,
  children,
  wallpaper = true,
}: {
  camera?: Camera;
  children: ReactNode;
  wallpaper?: boolean;
}) => {
  const format = useFormat();
  const cam3d = useCam3D();
  const scale = format.scale;
  const logicalW = format.logicalW ?? SCREEN.w;
  const inset = (logicalW - SCREEN.w) / 2;
  // A pantalla completa el monitor ya es más ancho: un zoom-out dejaría ver bajo el escritorio.
  const z = format.fullBleed ? Math.max(camera.z, 1) : camera.z;
  const k = scale * z;
  const tx = (camera.ax - camera.fx * z + inset) * scale;
  const ty = (camera.ay - camera.fy * z) * scale;
  const card = (
    <div
      style={{
        position: "absolute",
        left: format.screenLeft,
        top: format.screenTop,
        width: logicalW * scale,
        height: SCREEN.h * scale,
        borderRadius: format.fullBleed ? 0 : 34 * (scale / 2),
        overflow: "hidden",
        background: C.bg,
        boxShadow: format.fullBleed ? "none" : "0 0 0 2px rgb(240 240 234 / 8%), 0 40px 120px rgb(0 0 0 / 55%)",
        fontFamily: FONT_SANS,
        ...(cam3d
          ? { transform: camTransform(cam3d), transformOrigin: `${cam3d.ox}% ${cam3d.oy}%` }
          : null),
      }}
    >
      {wallpaper && <Wallpaper />}
      <div
        style={{
          position: "absolute",
          left: 0,
          top: 0,
          width: logicalW,
          height: SCREEN.h,
          transformOrigin: "0 0",
          transform: `translate(${tx}px, ${ty}px) scale(${k})`,
        }}
      >
        {children}
      </div>
    </div>
  );
  if (!cam3d) return card;
  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        perspective: cam3d.perspective,
        perspectiveOrigin: `${format.screenLeft + (logicalW * scale) / 2}px ${format.screenTop + (SCREEN.h * scale) / 2}px`,
      }}
    >
      {card}
    </div>
  );
};
