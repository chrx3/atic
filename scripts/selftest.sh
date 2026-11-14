#!/usr/bin/env bash
# Corre la autoprueba de arranque de la pill (PILL_SELFTEST=arranque, ver
# apps/atic/src/selftest.rs) con un ejecutable ya empaquetado, muestra el JSON
# y falla si la pill no terminó bien. Es el paso «Verify» del release.
#
# Uso:
#   bash scripts/selftest.sh <ruta a atic-pill> [<salida.json>]
#   bash scripts/selftest.sh "target/pill-mac/Atic.app/Contents/MacOS/atic-pill"
#
# Por omisión exige el agente de Atic Code, atic-mcp y la ventana
# (PILL_SELFTEST_EXIGE=sidecar,mcp,ventana): en un paquete tienen que estar.
# Se pisa exportando PILL_SELFTEST_EXIGE o PILL_SELFTEST_OMITE (por ejemplo,
# PILL_SELFTEST_OMITE=ventana en un Mac sin Metal).
#
# Si la pill muere a medias, el JSON queda con lo que alcanzó y «en_curso»
# dice qué estaba comprobando.
#
# Puede correr junto a la pill abierta: la autoprueba no toma el candado de
# instancia única ni toca la que está corriendo.

set -euo pipefail

bin="${1:?Uso: selftest.sh <ruta a atic-pill> [<salida.json>]}"
out="${2:-${TMPDIR:-/tmp}/atic-selftest.json}"
plazo="${PILL_SELFTEST_PLAZO:-120}"

rm -f "$out"
PILL_SELFTEST=arranque \
  PILL_SELFTEST_SALIDA="$out" \
  PILL_SELFTEST_PLAZO="$plazo" \
  PILL_SELFTEST_EXIGE="${PILL_SELFTEST_EXIGE-sidecar,mcp,ventana}" \
  "$bin" > /dev/null &
pid=$!
# La pill se rinde sola al vencer su plazo; esto es por si ni eso alcanza.
for _ in $(seq $((plazo + 60))); do
  kill -0 "$pid" 2>/dev/null || break
  sleep 1
done
if kill -0 "$pid" 2>/dev/null; then
  kill -9 "$pid" 2>/dev/null || true
  echo "La autoprueba no terminó ni con su plazo ($plazo s): se cerró a la fuerza." >&2
fi
status=0
wait "$pid" || status=$?

if [[ ! -s "$out" ]]; then
  echo "La pill terminó con $status y no escribió $out (¿se cayó al arrancar?)." >&2
  exit 1
fi
cat "$out"
echo
# Una tabla corta para el resumen del job.
node -e '
  const r = require(process.argv[1]);
  const rows = r.comprobaciones.map(c => `| ${c.nombre} | ${c.estado} | ${c.ms} | ${String(c.detalle).replace(/\|/g, "/")} |`);
  if (r.en_curso) rows.push(`| ${r.en_curso} | no terminó | | la pill salió (código ${process.argv[2]}) sin terminar esta comprobación |`);
  const text = [`### Autoprueba ${r.os}/${r.arch} ${r.version}: ${r.ok ? "ok" : "FALLA"}`, "", "| Comprobación | Estado | ms | Detalle |", "|---|---|---|---|", ...rows, ""].join("\n");
  if (process.env.GITHUB_STEP_SUMMARY) require("fs").appendFileSync(process.env.GITHUB_STEP_SUMMARY, text + "\n");
' "$out" "$status" 2>/dev/null || true
if grep -q '"en_curso"' "$out"; then
  echo "La pill salió con $status a medias: el JSON es parcial (mira «en_curso»)." >&2
  [[ "$status" -ne 0 ]] || status=1
fi
if [[ "$status" -ne 0 ]]; then
  echo "La autoprueba falló (código $status): mira las comprobaciones con estado «falla»." >&2
  exit "$status"
fi
echo "Autoprueba ok: $out"
