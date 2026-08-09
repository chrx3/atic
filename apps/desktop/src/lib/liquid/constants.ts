/**
 * Los números del sistema líquido, elegidos mirándolo en la ventana real.
 *
 * Reemplazan a los siete que existían para suplir lo que el filtro SVG no
 * alcanzaba a hacer —`GOO_SIGMA`, `GOO_GROW`, `preFilter()`, y las cinco del
 * cuello dibujado: grosor 26→10, piso 6, corte 140 y penetración 9/7—. Con el
 * campo de distancia no hace falta ninguna: el cuello sale de `BLEND` y el
 * contorno pasa por la geometría pedida sin engordar.
 */

import { sminBulge, sminReach } from "./sdf";

/**
 * Cuánto se mezclan las formas. Es la perilla que manda.
 *
 * Con 20 el alcance son 10 px (`sminReach` = k/2): el cuello corta pasado ese
 * hueco. Alineado al launcher lab (favGap/dotGap 15 → fusión al acercarse).
 */
export const BLEND = 20;

/**
 * Lado de la celda de muestreo, en px.
 *
 * Marching squares no ve nada más fino que su celda, y el costo va con el
 * cuadrado. Ocho equilibra silueta limpia y costo en el overlay.
 */
export const CELL = 8;

/** Pasadas de suavizado sobre el contorno ya trazado. */
export const SMOOTH = 2;

/** El hueco más grande que el cuello todavía cruza, con `BLEND`. */
export const REACH = sminReach(BLEND);

/** Cuánto engorda la silueta cerca de una junta, con `BLEND`. */
export const BULGE = sminBulge(BLEND);
