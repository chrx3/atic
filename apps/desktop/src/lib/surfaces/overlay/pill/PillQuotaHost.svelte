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
   * De ahí que acá NO haya `background` propio: lo pinta `Skin`. El
   * `data-float` sí recibe `--skin` opaco en reposo (regla de OverlaySurface),
   * y sin `border-radius` ese fill era un rectángulo sobre el blob. Por eso
   * el radio coincide con `CORNER`, y va `is-joined` para no pintar el
   * rectángulo encima del hilo.
   *
   * # Es de hover y nada más
   *
   * No se fija, no se arrastra y no se estira: aparece mientras el puntero
   * está en la herramienta, y se va. Un panel que se quedaba pedía todo lo
   * demás —marco propio, tamaño a mano, un modo compacto para cuando el marco
   * no daba— y cada una de esas piezas era una forma de que el panel se viera
   * distinto de la isla de la que nace.
   *
   * # Montaje
   *
   * Va en `OverlaySurface`, junto a los floats, y no dentro de `PillSurface`:
   * ahí el filtro del goo se volvería el bloque contenedor de su `fixed`.
   *
   * El gesto y la temporización están en `quotaHover.svelte.ts`.
   */
  import AgentLogo from "$features/agents/AgentLogo.svelte";
  import { t } from "$domain/i18n.svelte";
  import { agentQuotas } from "$domain/agentQuotas.svelte";
  import { config } from "$domain/config.svelte";
  import { sessionEffect } from "$domain/session";
  import { isAgentShown } from "$features/agents/agentCatalog";
  import { boxShape, stemBetween } from "$liquid/geometry";
  import { liquid } from "$surfaces/overlay/group.svelte";
  import {
    placeBesidePill,
    placeOnSide,
    unionRects,
  } from "$surfaces/overlay/floatPlace";
  import {
    publishMeasuredSkin,
    rectKey,
  } from "$surfaces/overlay/floatEmergeSkin";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import {
    quotaRows,
    spanFrom,
    type QuotaBar,
    type QuotaRow,
  } from "./pillQuota";
  import {
    enterQuotaPanel,
    leaveQuotaPanel,
    quotaHoverState,
  } from "./quotaHover.svelte";

  /** Radio de la silueta. El mismo de los otros floats. */
  const CORNER = 20;
  /**
   * Aire isla→panel: el largo del cuello.
   *
   * Es el número que decide si el efecto se ve. Pegados no hay cuello que
   * mirar —dos gotas a 8 px se funden en un bulto sin cintura—, y lejos el
   * hilo se lee como un alambre entre dos cajas. Con 14 el cuello es corto y
   * gordo, y el `smin` filetea las dos juntas: eso es lo que se lee líquido.
   */
  const GAP = 14;
  /** Radio del hilo. Un cuello flaco es un alambre por más filete que tenga. */
  const STEM_R = 6;
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
  /** Reloj para «corta en X» y lo stale, vivo mientras el panel está montado. */
  let now = $state(0);

  // Los agentes elegidos en Ajustes mandan también acá: es una sola lista de
  // «con qué agentes trabajo», no una preferencia por pantalla.
  $effect(() => sessionEffect(["config"]));

  const agentsShown = $derived(config.current?.agents_shown ?? []);
  const rows = $derived(
    quotaRows(agentQuotas.overview, now).filter((row) =>
      isAgentShown(row.agent, agentsShown),
    ),
  );

  function spanText(ms: number): string {
    const span = spanFrom(ms);
    return `${span.value} ${t(`pill.quota.unit.${span.unit}`)}`;
  }

  function windowText(bar: QuotaBar): string {
    if (bar.window === "model") {
      return t("pill.quota.window.modelWeek", { model: bar.model ?? "" });
    }
    if (bar.window !== "custom") return t(`pill.quota.window.${bar.window}`);
    if (bar.minutes == null) return t("pill.quota.window.unknown");
    return spanText(bar.minutes * 60_000);
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
   * la primera versión.
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

  $effect(() => {
    if (!alive) return;
    now = Date.now();
    const timer = setInterval(() => {
      now = Date.now();
    }, 1_000);
    return () => clearInterval(timer);
  });

  /** Coloca el panel hacia adentro de la pantalla. */
  $effect(() => {
    const anchor = quotaHoverState.anchor;
    void rows.length;
    void quotaHoverState.fallback;
    void agentQuotas.loading;
    if (!alive || !el) {
      placed = false;
      return;
    }
    if (!anchor) {
      placed = false;
      return;
    }
    const bw = el.offsetWidth;
    const bh = el.offsetHeight;
    if (bw <= 0 || bh <= 0) {
      placed = false;
      return;
    }
    const pill = surfaces.live["pill"];
    const skin = surfaces.live["pill-skin"];
    void pill?.x;
    void pill?.y;
    void pill?.w;
    void pill?.h;
    void skin?.x;
    void skin?.y;
    void skin?.w;
    void skin?.h;
    // La rueda es más grande que el disco: anclar al disco dejaba el panel
    // debajo de los gajos y el hover ciclaba (el panel robaba el mouse).
    const face = unionRects([pill, skin, anchor]);
    if (!face) {
      placed = false;
      return;
    }
    // El panel sale por el lado largo de la isla.
    //
    // `placeBesidePill` prueba siempre abajo primero, y acoplada a un canto la
    // isla está parada: el panel se iba al pie de la pantalla, a 300 px de la
    // herramienta que lo abrió y con el hilo cruzando media pantalla. Parada,
    // el hueco está al costado.
    //
    // La orientación se pregunta a la PIEL y no a la unión: `pill` es la caja
    // exterior —el respiro de la rueda— y es ancha aunque la isla esté parada.
    // Acoplada, además, es contra la piel que hay que medir el hueco, o el
    // cuello nace con el ancho de un respiro que no se ve.
    const shape = skin ?? face;
    const at =
      shape.h > shape.w * 1.2
        ? placeOnSide(
            shape,
            // `side` es el lado del panel que mira a la isla: «right» lo pone
            // a la izquierda de ella.
            shape.x - GAP - bw - EDGE >= 0 ? "right" : "left",
            { w: bw, h: bh },
            { gap: GAP, corner: CORNER },
          )
        : placeBesidePill(face, { w: bw, h: bh }, { gap: GAP, corner: CORNER });

    // El lado y el «hacia afuera» salen de la isla entera —así el panel
    // despeja la rueda—, pero sobre el eje paralelo manda el botón: el panel
    // cae debajo de la herramienta que lo abrió, que es de donde el usuario lo
    // llamó y a donde vuelve el cuello. Pegado al canto de la isla, que es lo
    // que hace `placeBesidePill` sola, el hilo salía de un botón y el panel
    // aparecía a media pantalla de distancia.
    const acx = anchor.x + anchor.w / 2;
    const acy = anchor.y + anchor.h / 2;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const horizontal = at.side === "top" || at.side === "bottom";
    const nx = horizontal
      ? Math.min(Math.max(acx - bw / 2, EDGE), Math.max(EDGE, vw - bw - EDGE))
      : at.x;
    const ny = horizontal
      ? at.y
      : Math.min(Math.max(acy - bh / 2, EDGE), Math.max(EDGE, vh - bh - EDGE));
    const alongPct = horizontal
      ? ((acx - nx) / Math.max(bw, 1)) * 100
      : ((acy - ny) / Math.max(bh, 1)) * 100;

    side = at.side;
    x = nx;
    y = ny;
    tail = Math.min(Math.max(alongPct, 0), 100);
    placed = true;
  });

  $effect(() => {
    if (!placed) return;
    const raf = requestAnimationFrame(() => (shown = true));
    return () => cancelAnimationFrame(raf);
  });

  $effect(() => (shown && el ? surfaces.add("quota", el) : undefined));

  $effect(() => {
    if (!alive || !el) {
      liquid.publish("quota", []);
      return;
    }
    void shown;
    void placed;
    void x;
    void y;
    void rows.length;
    void quotaHoverState.fallback;
    void agentQuotas.loading;
    const anchor = quotaHoverState.anchor;
    const stemSide = side;
    const host = el;
    return publishMeasuredSkin("quota", () => {
      const r = host.getBoundingClientRect();
      if (r.width <= 0 || r.height <= 0) {
        return { key: "empty", shapes: [] };
      }
      const layoutW = host.offsetWidth || r.width;
      const layoutH = host.offsetHeight || r.height;
      const k = Math.min(
        r.width / Math.max(layoutW, 1),
        r.height / Math.max(layoutH, 1),
        1,
      );
      const rect = { x: r.x, y: r.y, w: r.width, h: r.height };
      const shapes = [boxShape(rect, CORNER * k)];
      // Del mismo ancla del que se colocó el panel: el hilo sale del botón y
      // cae sobre el panel que está justo debajo. Contra el canto de la
      // pantalla el panel se corre y el centro del botón puede quedarse
      // afuera; de eso se encarga `stemBetween`, que lo trae al solape.
      if (anchor) {
        const stem = stemBetween(anchor, rect, stemSide, STEM_R);
        if (stem) shapes.push(stem);
      }
      return { key: `${rectKey(rect)}:${stemSide}`, shapes };
    });
  });

  $effect(() => () => liquid.publish("quota", []));
</script>

{#if alive}
  <div
    class="q-panel float-emerge is-joined"
    class:is-shown={shown}
    data-side={side}
    data-quota-panel
    data-float="quota"
    style:left="{x}px"
    style:top="{y}px"
    style:--tail="{tail}%"
    style:--float-stack={surfaces.stack("quota")}
    bind:this={el}
    aria-hidden="true"
    onpointerenter={enterQuotaPanel}
    onpointerleave={leaveQuotaPanel}
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

            <!-- El modelo entra a la clave: dos semanales «model» del mismo
                 largo (Antigravity: Gemini y Claude+GPT) colisionaban y Svelte
                 tiraba each_key_duplicate, dejando el panel en «Leyendo…».
                 `resetsAt` desempata dos custom del mismo largo sin modelo. -->
            {#each row.bars as bar (bar.window + (bar.model ?? "") + bar.minutes + (bar.resetsAt ?? ""))}
              <div class="q-bar">
                <span class="q-win">{windowText(bar)}</span>
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
   * Sin fondo propio: lo pinta `Skin`. El radio tiene que coincidir con
   * `CORNER` porque OverlaySurface pinta `--skin` opaco en `[data-float]`
   * cuando no está `is-joined`; sin radio ese fill era el rectángulo cuadrado
   * que se veía detrás del blob.
   */
  .q-panel {
    position: fixed;
    z-index: calc(var(--z-overlay-float, 100) + var(--float-stack, 0));
    box-sizing: border-box;
    min-width: 13rem;
    max-width: 22rem;
    padding: 0.4rem 0.45rem 0.46rem 0.6rem;
    overflow: hidden;
    border-radius: 20px;
    background: transparent;
    color: var(--text);
    font-size: 0.72rem;
    line-height: 1.3;
  }

  /*
   * `.float-emerge.is-shown` arma pointer-events al toque, a mitad del
   * scale. El recuadro de layout ya es el final: robaba el mouse al botón
   * (isla) o al gajo (rueda) y el panel se quedaba abierto o ciclaba.
   */
  .q-panel.float-emerge.is-shown {
    pointer-events: none;
    animation: q-enable-hit 0s linear var(--float-open-dur) forwards;
  }

  @keyframes q-enable-hit {
    to {
      pointer-events: auto;
    }
  }

  /* Sin `overflow`: el panel crece con las filas. Una lista de cupos con
     scroll es pedir que arrastren para ver el dato por el que la abrieron. */
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

  @media (prefers-reduced-motion: reduce) {
    .q-panel.float-emerge.is-shown {
      pointer-events: auto;
      animation: none;
    }
  }
</style>
