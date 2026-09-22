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
  /**
   * `false` = el float todavía no tiene marco: la pill reintenta. Sin eso el
   * primer despegue (el globo aún no nació) llama al arrastre en vacío y el
   * panel se queda en el rect de la cara.
   */
  current: null | ((init: { pointerId: number; x: number; y: number }) => boolean);
} = { current: null };

/**
 * El float tiene consolas montadas (PTYs vivas).
 *
 * La isla y el float son dos instancias del lanzador con `hasConsole` propio:
 * sin esta señal, la cara muestra el setup sin atajo aunque la consola esté
 * abierta en el float. La escribe el float vía `onLiveChange`; la lee la isla
 * para el botón "Consolas activas".
 */
export const agentsFloatLive = $state({ on: false });
