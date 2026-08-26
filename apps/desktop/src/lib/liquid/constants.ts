/**
 * Los números del sistema líquido, elegidos mirándolo en la ventana real.
 *
 * Reemplazan a los siete que existían para suplir lo que el filtro SVG no
 * alcanzaba a hacer —`GOO_SIGMA`, `GOO_GROW`, `preFilter()`, y las cinco del
 * cuello dibujado: grosor 26→10, piso 6, corte 140 y penetración 9/7—. Con el
 * campo de distancia no hace falta ninguna: el cuello sale de `BLEND` y el
 * contorno pasa por la geometría pedida sin engordar.
 */

import { sminBulge, sminInfluence, sminReach } from "./sdf";

/**
 * Cuánto se mezclan las formas. Es la perilla que manda.
 *
 * Con 24 el alcance son 12 px (`sminReach` = k/2): el cuello corta pasado ese
 * hueco. Idle favs/launcher (15–16) siguen sueltos; grow/approach se lee más
 * viscoso.
 */
export const BLEND = 24;

/**
 * Lado de la celda de muestreo, en px.
 *
 * Marching squares no ve nada más fino que su celda, y el costo va con el
 * cuadrado. Seis equilibra silueta limpia y costo en el overlay.
 */
export const CELL = 6;

/** Pasadas de suavizado sobre el contorno ya trazado. */
export const SMOOTH = 4;

/** El hueco más grande que el cuello todavía cruza, con `BLEND`. */
export const REACH = sminReach(BLEND);

/**
 * Desde qué hueco las siluetas ya se deforman una hacia la otra, con `BLEND`.
 *
 * El doble de `REACH`. Es el que decide qué formas comparten campo: agrupar por
 * `REACH` dejaba fuera toda la aproximación y la junta aparecía de golpe (ver
 * `sminInfluence`). `REACH` sigue siendo el que responde «¿hay cuello?».
 */
export const INFLUENCE = sminInfluence(BLEND);

/** Cuánto engorda la silueta cerca de una junta, con `BLEND`. */
export const BULGE = sminBulge(BLEND);
