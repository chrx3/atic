/**
 * Los números que el overlay comparte con Rust.
 *
 * Cada uno existe dos veces —acá y en `src-tauri/`— porque las dos mitades
 * hacen la misma cuenta desde lados distintos: el frontend coloca un `div`
 * dentro del overlay, Rust coloca ventanas del sistema, y tienen que coincidir
 * al píxel o la burbuja se despega de la pill.
 *
 * Duplicarlos no es un descuido: no hay forma de que el webview lea una `const`
 * de Rust en tiempo de ejecución sin inventar un comando para cada número. Lo
 * que sí se puede hacer es que la copia FALLE RUIDOSAMENTE cuando la otra
 * cambia, y de eso se encarga `contract.test.ts`, que lee el `.rs` y compara.
 *
 * Si un número de acá cambia sin su gemelo, el test se pone rojo. Es la única
 * defensa real: un comentario que dice «si cambia esto cambiá aquello» no
 * detiene a nadie.
 */

/**
 * Margen contra el borde del monitor (`bounds`, pantalla completa).
 *
 * `0` = puede solapar taskbar y pegarse al canto (modo Dynamic Island).
 * Gemelo de `MARGIN` en `floating.rs`.
 */
export const MARGIN = 0;

/**
 * Lo más chico que puede quedar el lanzador de agentes.
 *
 * Gemelos de `BUBBLE_MIN_W` / `BUBBLE_MIN_H` en `agents/bridge.rs`. Rust los
 * aplica al persistir; la vista sube el mínimo de altura cuando muestra una
 * consola para conservar un terminal útil.
 */
export const BUBBLE_MIN_W = 336;
export const BUBBLE_MIN_H = 176;

/**
 * Hueco entre la pill y la consola.
 *
 * No tiene gemelo con nombre: en `floating.rs` viaja como el parámetro `gap` de
 * la colocación, y quien lo elige es el frontend. Está acá porque es el número
 * del que depende que el cuello se dibuje: por debajo del alcance de la unión
 * las dos siluetas se funden, por encima se separan.
 */
export const BUBBLE_GAP = 10;
