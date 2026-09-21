/**
 * El overlay abre agentes en la isla de la pill. El float no debe nacer
 * encima: si Rust manda ancla, se ignora mientras esto está on.
 */
export const agentsIslandHost = $state({ on: false });

/**
 * Traspaso del gesto de detach directo al motor de arrastre del float.
 *
 * La pill no espera al header (la barra de la consola monta recién cuando el
 * traspaso adopta las sesiones): en cuanto el float existe, el arrastre pasa
 * por acá y el panel sigue a la mano mientras el contenido llega en segundo
 * plano. Se registra al montar el float y se baja al desmontar.
 */
export const agentsFloatHandoff: {
  current: null | ((init: { pointerId: number; x: number; y: number }) => void);
} = { current: null };
