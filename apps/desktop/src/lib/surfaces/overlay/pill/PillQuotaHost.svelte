<script lang="ts">
  /**
   * El panel de cupos: una gota más del grupo líquido.
   *
   * # Por qué se funde en vez de flotar aparte
   *
   * Antes era una caja con su borde y su sombra, o sea un elemento externo que
   * casualmente aparecía al lado de la pill. Publicándolo en el grupo líquido
   * comparte campo con la isla y nace un cuello entre las dos: se lee como que
   * la pill se estiró para mostrar algo, que es lo que de verdad pasó.
   *
   * De ahí que acá NO haya `background`, `border` ni `box-shadow`. La silueta la
   * pinta `Skin` desde `OverlaySurface`, y una caja propia encima se vería como
   * un parche rectangular sobre el blob. Este archivo aporta la geometría (por
   * `publishEmergeSkin`) y el contenido; el color es de la piel.
   *
   * # El hueco no es decorativo
   *
   * `GAP` tiene que quedar por debajo de `REACH` o el cuello no cruza y vuelven
   * a ser dos formas sueltas. Con `BLEND = 24`, `REACH` son 12 px.
   *
   * # Montaje
   *
   * Va en `OverlaySurface`, junto a los floats, y no dentro de `PillSurface`:
   * ahí el filtro del goo se volvería el bloque contenedor de su `fixed`, y el
   * panel quedaría posicionado contra la pill en vez de contra el viewport.
   *
   * El gesto y la temporización están en `quotaHover.svelte.ts`.
   */
  import { REACH } from "$liquid/constants";
  import AgentLogo from "$features/agents/AgentLogo.svelte";
  import { t } from "$domain/i18n.svelte";
  import { agentQuotas } from "$domain/agentQuotas.svelte";
  import { liquid } from "$surfaces/overlay/group.svelte";
  import { publishEmergeSkin } from "$surfaces/overlay/floatEmergeSkin";
  import { quotaRows, spanFrom, type QuotaRow, type WindowLabel } from "./pillQuota";
  import { quotaHoverState } from "./quotaHover.svelte";

  /** Radio de la silueta. El mismo de los otros floats. */
  const CORNER = 20;
  /** Hueco al botón. Por debajo de `REACH` para que el cuello cruce. */
  const GAP = Math.round(REACH * 0.7);
  /** Margen mínimo contra el borde de la ventana. */
  const EDGE = 6;
  /** `--morph-close-dur`: cuánto dura el repliegue antes de desmontar. */
  const CLOSE_MS = 100;

  let el = $state<HTMLElement | null>(null);
  let x = $state(0);
  let y = $state(0);
  /** Lado del panel que mira a la pill: de ahí nace el morph. */
  let side = $state<"top" | "bottom" | "left" | "right">("top");
  /** Dónde cae el cuello sobre ese lado, en %. Es el origen del scale. */
  let tail = $state(50);
  /** Medido y colocado. Antes de esto no se puede mostrar sin verlo saltar. */
  let placed = $state(false);
  /** Montado. Sigue en true durante el repliegue, o no habría qué animar. */
  let alive = $state(false);
  /** Abierto del todo. Es la clase que dispara el morph. */
  let shown = $state(false);

  const rows = $derived(quotaRows(agentQuotas.overview));
  const now = $derived(quotaHoverState.open ? Date.now() : 0);

  function spanText(ms: number): string {
    const span = spanFrom(ms);
    return `${span.value} ${t(`pill.quota.unit.${span.unit}`)}`;
  }

  function windowText(win: WindowLabel, minutes: number | null): string {
    if (win !== "custom") return t(`pill.quota.window.${win}`);
    if (minutes == null) return t("pill.quota.window.unknown");
    return spanText(minutes * 60_000);
  }

  /** Plan tal como lo guarda el proveedor (`max 20x`, `pro_plus`, `plus`). */
  function planText(plan: string | null): string {
    return plan ? plan.replace(/_/g, " ") : "";
  }

  /** Centavos → «1.213» con la separación de miles del idioma activo. */
  function moneyText(cents: number): string {
    return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(
      cents / 100,
    );
  }

  function spendText(row: QuotaRow): string {
    if (!row.spend) return "";
    const amount = t("pill.quota.spend", { amount: moneyText(row.spend.cents) });
    if (row.spend.periodEnd == null || row.spend.periodEnd <= now) return amount;
    return `${amount} · ${t("pill.quota.periodEnds", {
      when: spanText(row.spend.periodEnd - now),
    })}`;
  }

  /**
   * Montar / desmontar, con el repliegue en el medio.
   *
   * Solo lee `quotaHoverState.open` y solo escribe `alive` / `shown` /
   * `placed`. Preguntar acá por `alive` —«si ya está cerrado, no hagas nada»—
   * sería leer y escribir el mismo estado en un efecto, que es como se rompió
   * la primera versión. El costo de no preguntar es un `setTimeout` de más al
   * arrancar, que no hace nada.
   */
  $effect(() => {
    if (quotaHoverState.open) {
      alive = true;
      return;
    }
    shown = false;
    const timer = setTimeout(() => {
      alive = false;
      placed = false;
    }, CLOSE_MS);
    return () => clearTimeout(timer);
  });

  /**
   * Coloca el panel hacia adentro de la pantalla.
   *
   * La isla solo existe acoplada a un canto, así que el borde más cercano al
   * botón ES el canto donde vive la pill: el panel va al lado opuesto, o
   * nacería fuera de la pantalla. `side` es el lado del panel que mira a la
   * pill, que es lo que `.float-emerge` toma como origen del morph.
   */
  $effect(() => {
    const anchor = quotaHoverState.anchor;
    // Dependencias explícitas: el contenido cambia el tamaño medido.
    void rows.length;
    void quotaHoverState.fallback;
    void agentQuotas.loading;
    if (!alive || !anchor || !el) {
      placed = false;
      return;
    }
    // `offsetWidth/Height` y no `getBoundingClientRect()`: el morph de
    // `.float-emerge` arranca en `scale(0.55)`, y el rect devuelve la medida
    // YA escalada. Colocando con ese número el panel se recortaba contra un
    // ancho que no era el suyo y terminaba saliéndose de la pantalla al
    // llegar a tamaño completo. La caja de layout ignora el transform.
    const bw = el.offsetWidth;
    const bh = el.offsetHeight;
    if (bw <= 0 || bh <= 0) {
      placed = false;
      return;
    }
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const cx = anchor.x + anchor.w / 2;
    const cy = anchor.y + anchor.h / 2;

    const room = { left: cx, right: vw - cx, top: cy, bottom: vh - cy };
    const dock = (Object.keys(room) as (keyof typeof room)[]).reduce((a, b) =>
      room[a] <= room[b] ? a : b,
    );

    // Todo en locales y una sola escritura al final. Leer `x` para recortarlo
    // (`x = clamp(x)`) es leer y escribir el mismo estado dentro de un efecto:
    // Svelte corta la actualización y el panel se queda a medio abrir. Pasó.
    let nx: number;
    let ny: number;
    if (dock === "left" || dock === "right") {
      nx = dock === "left" ? anchor.x + anchor.w + GAP : anchor.x - bw - GAP;
      ny = cy - bh / 2;
    } else {
      ny = dock === "top" ? anchor.y + anchor.h + GAP : anchor.y - bh - GAP;
      nx = cx - bw / 2;
    }

    nx = Math.min(Math.max(nx, EDGE), Math.max(EDGE, vw - bw - EDGE));
    ny = Math.min(Math.max(ny, EDGE), Math.max(EDGE, vh - bh - EDGE));

    // El cuello se mide contra la posición YA recortada: calculado antes,
    // apuntaría a donde el panel habría estado si hubiera entrado entero.
    const along =
      dock === "left" || dock === "right"
        ? ((cy - ny) / Math.max(bh, 1)) * 100
        : ((cx - nx) / Math.max(bw, 1)) * 100;

    side = dock;
    x = nx;
    y = ny;
    tail = Math.min(Math.max(along, 0), 100);
    placed = true;
  });

  // El morph necesita un cuadro entre «montado en su sitio» y «abierto», o el
  // navegador pinta el estado final sin transición.
  $effect(() => {
    if (!placed) return;
    const raf = requestAnimationFrame(() => (shown = true));
    return () => cancelAnimationFrame(raf);
  });

  // La silueta, para que el campo de distancia la funda con la isla.
  $effect(() => {
    if (!alive || !el) {
      liquid.publish("quota", []);
      return;
    }
    // Despierta el seguimiento en los dos morphs: al abrir y al replegar.
    void shown;
    void placed;
    void x;
    void y;
    // Y también cuando cambia el contenido, que llega después del morph: los
    // cupos se consultan al abrir, y `publishEmergeSkin` deja de seguir el
    // rect en cuanto queda quieto. Con el panel acoplado arriba, crecer de
    // «Leyendo cupos…» a las filas no mueve `x` ni `y` —el ancho se queda en
    // `min-width` y el borde de arriba está clavado al botón—, así que sin
    // estas dependencias la silueta se quedaba con la altura del cargando.
    void rows.length;
    void quotaHoverState.fallback;
    void agentQuotas.loading;
    return publishEmergeSkin("quota", el, CORNER);
  });

  // Desmontar la superficie no puede dejar la gota publicada: el Skin la
  // seguiría dibujando sin nadie que la mueva.
  $effect(() => () => liquid.publish("quota", []));
</script>

{#if alive}
  <!-- `aria-hidden`: es un apoyo visual del hover. El botón que lo abre ya
       lleva su propio nombre accesible. -->
  <div
    class="q-panel float-emerge"
    class:is-shown={shown}
    data-side={side}
    style:left="{x}px"
    style:top="{y}px"
    style:--tail="{tail}%"
    bind:this={el}
    aria-hidden="true"
  >
    {#if rows.length > 0}
      <div class="q-rows">
        {#each rows as row (row.agent)}
          <div class="q-row">
            <div class="q-head">
              <AgentLogo agent={row.agent} size={13} />
              <span class="q-name">{row.name}</span>
              {#if row.staleAt != null}
                <span class="q-meta"
                  >{t("pill.quota.stale", { when: spanText(now - row.staleAt) })}</span
                >
              {:else if row.plan}
                <span class="q-meta">{planText(row.plan)}</span>
              {/if}
            </div>

            {#if row.error}
              <div class="q-note is-error">{row.error}</div>
            {:else if row.spend}
              <div class="q-note">{spendText(row)}</div>
            {/if}

            {#each row.bars as bar (bar.window + bar.minutes)}
              <div class="q-bar">
                <span class="q-win">{windowText(bar.window, bar.minutes)}</span>
                <span class="q-track">
                  <span
                    class="q-fill is-{bar.tone}"
                    style:width="{Math.max(bar.percent, 2)}%"
                  ></span>
                </span>
                <span class="q-pct" data-numeric>{Math.round(bar.percent)}%</span>
                <span class="q-reset">
                  {bar.resetsAt != null && bar.resetsAt > now
                    ? spanText(bar.resetsAt - now)
                    : ""}
                </span>
              </div>
            {/each}
          </div>
        {/each}
      </div>
    {:else if agentQuotas.loading}
      <div class="q-fallback">{t("pill.quota.loading")}</div>
    {:else}
      <div class="q-fallback">{quotaHoverState.fallback}</div>
    {/if}
  </div>
{/if}

<style>
  /*
   * Sin fondo, sin borde y sin sombra: los pinta `Skin` sobre la silueta ya
   * fundida con la isla. Ver la cabecera del componente.
   */
  .q-panel {
    position: fixed;
    z-index: var(--z-overlay-float, 100);
    min-width: 13rem;
    max-width: 22rem;
    padding: 0.46rem 0.6rem;
    background: transparent;
    color: var(--text);
    font-size: 0.72rem;
    line-height: 1.3;

    /* Duro, y también contra `.float-emerge.is-shown`, que lo pone en `auto`:
       el puntero se queda en el botón que abrió el panel. Si el panel tomara
       el mouse, el puntero saldría del botón, el hover se cortaría y el panel
       se cerraría solo. */
    pointer-events: none !important;
  }

  .q-rows {
    display: flex;
    flex-direction: column;
    gap: 0.36rem;
  }

  .q-head {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }

  .q-name {
    font-weight: 600;
  }

  .q-meta {
    /* Empuja el plan contra el canto derecho sin un `justify` que también
       separaría el logo del nombre. */
    margin-left: auto;
    color: var(--faint);
    font-size: 0.66rem;
  }

  .q-note,
  .q-fallback {
    color: var(--muted);
    font-size: 0.68rem;
  }

  .q-note.is-error {
    color: var(--warn);
  }

  .q-bar {
    display: grid;
    align-items: center;

    /* Cuatro columnas fijas y no `auto`: con anchos que dependan del texto,
       las barras de dos agentes distintos no arrancan en la misma x y el
       panel deja de leerse como una tabla. */
    grid-template-columns: 4.2rem 1fr 2.1rem 2.1rem;
    gap: 0.34rem;
  }

  .q-win,
  .q-reset {
    color: var(--muted);
    font-size: 0.66rem;
  }

  .q-reset {
    text-align: right;
  }

  .q-track {
    height: 0.28rem;
    overflow: hidden;
    border-radius: 999px;

    /* El canal va sobre la piel, no sobre un fondo propio: se oscurece la piel
       misma para que el surco parezca hundido en la gota. */
    background: color-mix(in sRGB, var(--text) 14%, transparent);
  }

  .q-fill {
    display: block;
    height: 100%;
    border-radius: 999px;
    background: var(--accent);
  }

  .q-fill.is-warn {
    background: var(--warn);
  }

  .q-fill.is-hot {
    background: var(--danger);
  }

  .q-pct {
    text-align: right;
    font-size: 0.68rem;
    font-variant-numeric: tabular-nums;
  }
</style>
