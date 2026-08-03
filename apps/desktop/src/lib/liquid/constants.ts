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
 * Con 59 el alcance son 29.5 px: la burbuja sigue colgando de la pill hasta
 * casi treinta píxeles de distancia y el cuello se forma solo. El hueco de 10
 * que usa la app queda holgado.
 */
export const BLEND = 59;

/**
 * Lado de la celda de muestreo, en px.
 *
 * Marching squares no ve nada más fino que su celda, y el costo va con el
 * cuadrado. Seis es donde la silueta se lee limpia sin que el cálculo pase de
 * unos pocos milisegundos por cuadro en la ventana real.
 */
export const CELL = 6;

/** Pasadas de suavizado sobre el contorno ya trazado. */
export const SMOOTH = 2;

/** El hueco más grande que el cuello todavía cruza, con `BLEND`. */
export const REACH = sminReach(BLEND);

/** Cuánto engorda la silueta cerca de una junta, con `BLEND`. */
export const BULGE = sminBulge(BLEND);
