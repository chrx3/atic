/** Cursor del mouse en coordenadas lógicas; se contrapone al zoom de cámara para verse siempre igual. */
export const AgentesCursor = ({
  x,
  y,
  zoom,
  opacity,
  pressed = false,
}: {
  x: number;
  y: number;
  zoom: number;
  opacity: number;
  pressed?: boolean;
}) => (
  <div
    style={{
      position: "absolute",
      left: x,
      top: y,
      width: 0,
      height: 0,
      opacity,
      transform: `scale(${(pressed ? 0.9 : 1) / zoom})`,
      transformOrigin: "0 0",
      pointerEvents: "none",
      filter: "drop-shadow(0 1px 2px rgb(0 0 0 / 45%))",
    }}
  >
    <svg width={13} height={19} viewBox="0 0 13 19" style={{ display: "block" }}>
      <path
        d="M1 1 V15.2 L4.6 11.9 L7.3 18 L9.7 16.9 L7.1 11 L12 11 Z"
        fill="#fbfbf8"
        stroke="#0d0d0c"
        strokeWidth={1.1}
        strokeLinejoin="round"
      />
    </svg>
  </div>
);
