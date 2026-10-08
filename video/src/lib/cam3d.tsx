import { createContext, useContext, type ReactNode } from "react";

/** Pose 3D del monitor dentro del cuadro (grados y px de salida). */
export type Cam3D = {
  rx: number;
  ry: number;
  rz: number;
  tx: number;
  ty: number;
  tz: number;
  s: number;
  /** Origen de la transformación, en % del monitor. */
  ox: number;
  oy: number;
  perspective: number;
};

export const CAM_FLAT: Cam3D = {
  rx: 0,
  ry: 0,
  rz: 0,
  tx: 0,
  ty: 0,
  tz: 0,
  s: 1,
  ox: 50,
  oy: 50,
  perspective: 1600,
};

const Cam3DContext = createContext<Cam3D | null>(null);

export const Cam3DProvider = ({
  cam,
  children,
}: {
  cam: Cam3D | null;
  children: ReactNode;
}) => <Cam3DContext.Provider value={cam}>{children}</Cam3DContext.Provider>;

export const useCam3D = () => useContext(Cam3DContext);

export const camTransform = (c: Cam3D) =>
  `translate3d(${c.tx}px, ${c.ty}px, ${c.tz}px) rotateX(${c.rx}deg) rotateY(${c.ry}deg) rotateZ(${c.rz}deg) scale(${c.s})`;
