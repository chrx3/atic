/**
 * Baja un registro solo si sigue siendo de quien lo publicó.
 *
 * Dos `$effect` pueden compartir id (`agents`: pestaña de la pill y panel).
 * El cleanup del que se va no puede borrar al que acaba de entrar: el overlay
 * quedaría click-through sobre un globo visible.
 */
export function releaseOwned<K, V>(map: Map<K, V>, key: K, owner: V): void {
  if (map.get(key) === owner) map.delete(key);
}
