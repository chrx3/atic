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
  import { ms, MOTION } from "$lib/motion";
  import { agentQuotas } from "$domain/agentQuotas.svelte";
  import { config } from "$domain/config.svelte";
  import { sessionEffect } from "$domain/session";
  import { isAgentShown } from "$features/agents/agentCatalog";
  import {
    boxShape,
    gapBetween,
    nearestStemBody,
    stemBetween,
    stemBodyFits,
  } from "$liquid/geometry";
  import { INFLUENCE, REACH } from "$liquid/constants";
  import { liquid, LIQUID_HUB } from "$surfaces/overlay/group.svelte";
  import {
    placeBesideAnchor,
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
    type QuotaTone,
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
  /** Tope de un hilo desde las gotas de la rueda: más lejos ya es un alambre. */
  const STEM_MAX = 96;
  /** Margen mínimo contra el borde de la ventana. */
  const EDGE = 6;
  /** `--morph-close-dur`: cuánto dura el repliegue antes de desmontar. */
  const CLOSE_MS = ms(MOTION.morphClose);
  /** Radio de la gota de detalle del canto (más chica que el panel). */
  const CARD_CORNER = 12;
  /** Radio del cuello panel→detalle. Un poco más fino que el de la pill. */
  const CARD_STEM_R = 5;

  let el = $state<HTMLElement | null>(null);
  /** Card de detalle del canto: gota propia, unida por un cuello. */
  let cardEl = $state<HTMLElement | null>(null);
  /** Anillo de cada agente: de ahí sale la altura de la card. */
  let agentEls = $state<Record<string, HTMLElement | null>>({});
  /** Centro de la card, en coords del panel (layout, no bounding). */
  let cardTop = $state(0);
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
  /**
   * Hay cuello de verdad con la pill. Si no, no se publica hilo ni se entra
   * al hub: un hilo a un botón que ya no está deja el pezón del techo.
   */
  let joined = $state(false);
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

  /**
   * Agente bajo el puntero. La fila de anillos no lleva texto: el detalle
   * completo aparece abajo, y sin hover manda la ventana más apretada.
   */
  let hover = $state<string | null>(null);

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

  /**
   * De dónde sale el cuello.
   *
   * Primero el gajo que abrió el panel: el hilo tiene que nacer de Agentes,
   * no del AABB de la flor. Si ese cuerpo no sirve, las gotas publicadas
   * (`parts`) son la flor de verdad. `pill-skin` en ese tramo sigue midiendo
   * el stack —el disco de reposo arriba-izquierda del root— y un hilo a ese
   * disco era el palo que se veía al lado de la flor.
   */
  function stemForQuota(
    panel: { x: number; y: number; w: number; h: number },
    stemSide: "top" | "bottom" | "left" | "right",
  ): { body: { x: number; y: number; w: number; h: number }; radius: number } | null {
    const petal = quotaHoverState.anchor;
    if (
      petal &&
      stemBodyFits(petal, stemSide, STEM_R) &&
      stemBetween(petal, panel, stemSide, STEM_R) &&
      gapBetween(petal, panel) <= STEM_MAX
    ) {
      const g = gapBetween(petal, panel);
      const radius = Math.min(12, Math.max(STEM_R, Math.round(g / 6)));
      return { body: petal, radius };
    }
    const parts = quotaHoverState.parts;
    if (parts && parts.length > 0) {
      const hub = unionRects(parts);
      if (
        hub &&
        stemBodyFits(hub, stemSide, STEM_R) &&
        stemBetween(hub, panel, stemSide, STEM_R) &&
        gapBetween(hub, panel) <= STEM_MAX
      ) {
        const g = gapBetween(hub, panel);
        const radius = Math.min(12, Math.max(STEM_R, Math.round(g / 6)));
        return { body: hub, radius };
      }
    }
    const body = nearestStemBody(
      [
        surfaces.live["pill-skin"],
        surfaces.live["pill"],
        quotaHoverState.anchor,
      ],
      panel,
      stemSide,
      STEM_R,
      INFLUENCE,
    );
    return body ? { body, radius: STEM_R } : null;
  }

  function spendText(row: QuotaRow): string {
    if (!row.spend) return "";
    const amount = t("pill.quota.spend", { amount: moneyText(row.spend.cents) });
    if (row.spend.periodEnd == null || row.spend.periodEnd <= now) return amount;
    return `${amount} · ${t("pill.quota.periodEnds", {
      when: spanText(row.spend.periodEnd - now),
    })}`;
  }

  /** La ventana más apretada del agente: el anillo resume eso. */
  function headline(row: QuotaRow): { percent: number; tone: QuotaTone } {
    const first = row.bars[0];
    if (!first) return { percent: 0, tone: "ok" };
    return row.bars.reduce(
      (best, bar) =>
        bar.percent > best.percent
          ? { percent: bar.percent, tone: bar.tone }
          : best,
      { percent: first.percent, tone: first.tone },
    );
  }

  /** Sin hover, el agente más apretado; con hover, el apuntado. */
  const detailRow = $derived.by(() => {
    const pointed = hover ? rows.find((r) => r.agent === hover) : null;
    if (pointed) return pointed;
    const withBars = rows.filter((row) => row.bars.length > 0);
    if (withBars.length === 0) return rows[0] ?? null;
    return withBars.reduce((a, b) =>
      headline(b).percent > headline(a).percent ? b : a,
    );
  });

  /**
   * Isla vertical (pill acoplada a un canto) y sin rueda abierta: el panel se
   * dibuja en columna para acompañar el canto. Se pregunta por la silueta y
   * no por `side`: con la rueda abierta el panel también puede caer a un
   * costado y ahí el layout horizontal sigue siendo el correcto.
   */
  const sideLayout = $derived.by(() => {
    if (quotaHoverState.parts?.length) return false;
    const shape = surfaces.live["pill-skin"] ?? surfaces.live["pill"];
    return shape != null && shape.h > shape.w * 1.2;
  });

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
    hover = null;
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
    void sideLayout;
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
    const parts = quotaHoverState.parts;
    void pill?.x;
    void pill?.y;
    void pill?.w;
    void pill?.h;
    void skin?.x;
    void skin?.y;
    void skin?.w;
    void skin?.h;
    void parts;
    const hub = unionRects(parts && parts.length > 0 ? parts : [pill, skin, anchor]);
    if (!hub) {
      placed = false;
      return;
    }
    // Con la rueda abierta el ancla es el gajo, no la flor entera: si no, el
    // panel cuelga del AABB de 252 px y se lee como un cartel bajo la rueda.
    // `placeBesideAnchor` lo pone hacia afuera de ese gajo.
    //
    // En la isla, `placeBesidePill` prueba siempre abajo primero. Acoplada a
    // un canto está parada: el panel se iba al pie de la pantalla. Parada, el
    // hueco está al costado. La orientación se pregunta a la PIEL: `pill` es
    // el respiro de la rueda y es ancha aunque la isla esté parada.
    const shape = skin ?? hub;
    const at =
      parts && parts.length > 0
        ? placeBesideAnchor(hub, anchor, { w: bw, h: bh }, {
            gap: GAP,
            corner: CORNER,
          })
        : shape.h > shape.w * 1.2
          ? placeOnSide(
              shape,
              // `side` es el lado del panel que mira a la isla: «right» lo
              // pone a la izquierda de ella.
              shape.x - GAP - bw - EDGE >= 0 ? "right" : "left",
              { w: bw, h: bh },
              { gap: GAP, corner: CORNER },
            )
          : placeBesidePill(hub, { w: bw, h: bh }, { gap: GAP, corner: CORNER });

    // Sobre el eje paralelo manda el botón: el panel queda frente a la
    // herramienta que lo abrió. Pegado al canto de la isla, que es lo que
    // hace `placeBesidePill` sola, el hilo salía de un botón y el panel
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

  /**
   * La card del canto se centra contra el anillo del proveedor que se está
   * viendo. Se mide con `offsetTop` (layout, no bounding): el morph escala el
   * panel y un bounding a mitad de vuelo dejaría la card corrida.
   */
  $effect(() => {
    if (!sideLayout || !el || !cardEl) return;
    const row = detailRow;
    const ring = row ? agentEls[row.agent] : null;
    if (!ring) return;
    const h = cardEl.offsetHeight;
    const vh = window.innerHeight;
    const top = ring.offsetTop + ring.offsetHeight / 2 - h / 2;
    cardTop = Math.max(EDGE - y, Math.min(top, vh - EDGE - h - y));
  });

  $effect(() => (shown && el ? surfaces.add("quota", el) : undefined));

  /**
   * La card del canto flota fuera de la caja del panel, así que se publica
   * como zona viva propia. Sin esto, el overlay se desarma al mover el puntero
   * hacia ella y el panel se cierra justo cuando ibas a leer el detalle.
   */
  $effect(() => {
    if (!sideLayout || !cardEl) return;
    void side;
    return surfaces.add("quota-card", cardEl);
  });

  $effect(() => {
    if (!alive || !el) {
      joined = false;
      liquid.publish("quota", []);
      return;
    }
    void shown;
    void placed;
    void x;
    void y;
    void rows.length;
    void hover;
    void sideLayout;
    void cardTop;
    void quotaHoverState.fallback;
    void quotaHoverState.parts;
    void quotaHoverState.anchor;
    void agentQuotas.loading;
    const live = surfaces.live["pill-skin"] ?? surfaces.live["pill"];
    void live?.x;
    void live?.y;
    void live?.w;
    void live?.h;
    const stemSide = side;
    const host = el;
    const panelGuess = {
      x,
      y,
      w: host.offsetWidth,
      h: host.offsetHeight,
    };
    const hang = stemForQuota(panelGuess, stemSide);
    const gap = hang
      ? gapBetween(hang.body, panelGuess)
      : live
        ? gapBetween(live, panelGuess)
        : Infinity;
    joined = hang != null || gap <= REACH;
    const group = joined || gap <= INFLUENCE ? LIQUID_HUB : undefined;
    return publishMeasuredSkin(
      "quota",
      () => {
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
        const next = stemForQuota(rect, stemSide);
        if (next) {
          const stem = stemBetween(next.body, rect, stemSide, next.radius);
          if (stem) shapes.push(stem);
        }
        // Gota de detalle del canto: caja + cuello, en la misma isla que el
        // panel para que la piel las una. El contenido va encima.
        let card = "";
        const cr = sideLayout ? cardEl?.getBoundingClientRect() : null;
        if (cr && cr.width > 0 && cr.height > 0) {
          const cardRect = { x: cr.x, y: cr.y, w: cr.width, h: cr.height };
          const neck = stemBetween(
            cardRect,
            rect,
            stemSide === "left" ? "right" : "left",
            CARD_STEM_R,
          );
          if (neck) {
            shapes.push(boxShape(cardRect, CARD_CORNER * k), neck);
            card = rectKey(cardRect);

            // La card viaja con transición: el hit-rect tiene que seguirla
            // cuadro a cuadro, no quedar clavado en la posición vieja.
            if (!surfaces.dragging) surfaces.schedule();
          }
        }
        return {
          key: `${rectKey(rect)}:${stemSide}:${next ? rectKey(next.body) : ""}:${next?.radius ?? 0}:${card}`,
          shapes,
        };
      },
      group,
    );
  });

  $effect(() => () => liquid.publish("quota", []));
</script>

{#if alive}
  <div
    class="q-panel float-emerge"
    class:is-joined={joined}
    class:is-shown={shown}
    class:is-side={sideLayout}
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
      <div class="q-rings">
        {#each rows as row (row.agent)}
          {@const head = headline(row)}
          <div
            class="q-agent"
            class:is-hovered={detailRow?.agent === row.agent}
            aria-hidden="true"
            bind:this={agentEls[row.agent]}
            onpointerenter={() => (hover = row.agent)}
          >
            <span class="q-ring">
              <svg viewBox="0 0 40 40">
                <circle
                  class="q-ring-track"
                  cx="20"
                  cy="20"
                  r="17"
                  pathLength="100"
                ></circle>
                <circle
                  class="q-ring-fill is-{head.tone}"
                  cx="20"
                  cy="20"
                  r="17"
                  pathLength="100"
                  style:stroke-dasharray="{Math.max(head.percent, 1)} 100"
                ></circle>
              </svg>
              <span class="q-ring-logo"
                ><AgentLogo agent={row.agent} size={16} /></span
              >
            </span>
            <span class="q-agent-pct" data-numeric>
              {row.bars.length > 0 ? `${Math.round(head.percent)}%` : "—"}
            </span>
          </div>
        {/each}
      </div>

      {#if detailRow}
        <div class="q-detail" bind:this={cardEl} style:top="{cardTop}px">
          <div class="q-detail-head">
            <span class="q-detail-name">{detailRow.name}</span>
            {#if detailRow.staleAt != null}
              <span class="q-detail-meta"
                >{t("pill.quota.stale", {
                  when: spanText(now - detailRow.staleAt),
                })}</span
              >
            {:else if detailRow.plan}
              <span class="q-detail-meta">{planText(detailRow.plan)}</span>
            {/if}
          </div>

          {#if detailRow.error}
            <p class="q-detail-error">{detailRow.error}</p>
          {:else if detailRow.bars.length > 0}
            <!-- El modelo entra a la clave: dos semanales «model» del mismo
                 largo (Antigravity: Gemini y Claude+GPT) colisionaban y
                 Svelte tiraba each_key_duplicate, dejando el panel en
                 «Leyendo…». `resetsAt` desempata dos custom iguales. -->
            {#if sideLayout}
              {#each detailRow.bars as bar (bar.window + (bar.model ?? "") + bar.minutes + (bar.resetsAt ?? ""))}
                <p class="q-line is-{bar.tone}">
                  <span class="q-line-win">{windowText(bar)}</span>
                  <span class="q-line-val">
                    <strong data-numeric>{Math.round(bar.percent)}%</strong>
                    {#if bar.resetsAt != null && bar.resetsAt > now}
                      <span class="q-line-reset"
                        >· {t("pill.quota.reset", {
                          when: spanText(bar.resetsAt - now),
                        })}</span
                      >
                    {/if}
                  </span>
                </p>
              {/each}
            {:else}
              {#each detailRow.bars as bar (bar.window + (bar.model ?? "") + bar.minutes + (bar.resetsAt ?? ""))}
                <div class="q-bar is-{bar.tone}">
                  <span class="q-win">{windowText(bar)}</span>
                  <span class="q-track">
                    <span class="q-fill" style:width="{Math.max(bar.percent, 2)}%"></span>
                  </span>
                  <span class="q-val">
                    <span class="q-pct" data-numeric>{Math.round(bar.percent)}%</span>
                    {#if bar.resetsAt != null && bar.resetsAt > now}
                      <span class="q-sep" aria-hidden="true">·</span>
                      <span class="q-reset"
                        >{t("pill.quota.reset", {
                          when: spanText(bar.resetsAt - now),
                        })}</span
                      >
                    {/if}
                  </span>
                </div>
              {/each}
            {/if}
          {/if}

          {#if !detailRow.error && detailRow.spend}
            <p class="q-detail-note">{spendText(detailRow)}</p>
          {/if}
        </div>
      {/if}
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

    /* Ancho fijo, no `min/max`: el detalle cambia al hover y un panel
       content-sized movía los anillos bajo el puntero — el hover entraba y
       salía en loop. Con ancho fijo, el alto puede crecer sin tocar el resto. */
    width: 16rem;
    padding: 0.55rem 0.7rem 0.6rem;
    overflow: hidden;
    border-radius: 20px;
    background: transparent;
    color: var(--text);
    font-size: 0.75rem;
    line-height: 1.35;
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

  /* Anillos por agente: el estado de un vistazo, sin una línea de texto. */
  .q-rings {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: 0.45rem 0.6rem;
  }

  .q-agent {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.2rem;
  }

  .q-ring {
    position: relative;
    display: grid;
    width: 2.15rem;
    height: 2.15rem;
    place-items: center;
  }

  .q-ring svg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    transform: rotate(-90deg);
  }

  .q-ring-track,
  .q-ring-fill {
    fill: none;
    stroke-width: 3;

    /* El arco se asienta cuando llega dato fresco y el tono entra suave. */
    transition:
      stroke var(--duration-fast, 125ms) var(--ease-smooth-out, ease-out),
      stroke-dasharray var(--duration-slow, 200ms) var(--ease-smooth-out, ease-out);
  }

  .q-ring-track {
    stroke: color-mix(in sRGB, var(--text) 13%, transparent);
  }

  .q-agent.is-hovered .q-ring-track {
    stroke: color-mix(in sRGB, var(--text) 26%, transparent);
  }

  .q-ring-fill {
    stroke: var(--accent);
    stroke-linecap: round;
  }

  .q-ring-fill.is-warn {
    stroke: var(--warn);
  }

  .q-ring-fill.is-hot {
    stroke: var(--danger);
  }

  .q-ring-logo {
    display: grid;
    place-items: center;
    color: var(--text);
  }

  .q-agent-pct {
    font-size: 0.6875rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    transition: color var(--duration-fast, 125ms) var(--ease-smooth-out, ease-out);
  }

  .q-agent.is-hovered .q-agent-pct {
    color: var(--text);
  }

  /* En el canto el panel queda con los anillos contra el borde y el detalle
     flota aparte, hacia adentro: así el cuerpo no reserva el alto del texto
     ni deja huecos. `data-side` es el lado del panel que da a la isla, así
     que la isla a la izquierda espeja todo. */
  .q-panel.is-side {
    width: auto;
    overflow: visible;
  }

  .q-panel.is-side .q-rings {
    flex-flow: column nowrap;
    align-items: stretch;
    gap: 0.35rem;
  }

  .q-panel.is-side .q-agent {
    flex-direction: row-reverse;
    align-items: center;
    gap: 0.4rem;
  }

  .q-panel.is-side[data-side="left"] .q-agent {
    flex-direction: row;
  }

  /* Card de detalle: la pinta la piel como una gota más (caja + cuello), así
     que acá solo van contenido y posición. El `top` se anima recién cuando el
     panel ya está asentado: si no, la gota se deslizaría desde 0 al abrir. */
  .q-panel.is-side .q-detail {
    position: absolute;
    width: 11rem;
    margin: 0;
    padding: 0.6rem 0.7rem 0.65rem;
    border-top: 0;
  }

  .q-panel.is-side.is-shown .q-detail {
    transition: top var(--duration-slow, 200ms) var(--ease-smooth-out, ease-out);
  }

  .q-panel.is-side[data-side="right"] .q-detail {
    right: calc(100% + 1.15rem);
  }

  .q-panel.is-side[data-side="left"] .q-detail {
    left: calc(100% + 1.15rem);
  }

  .q-line {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
    margin: 0;
    font-size: 0.6875rem;
    line-height: 1.4;
  }

  .q-line-win {
    overflow: hidden;
    color: var(--muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .q-line-val {
    color: var(--muted);
    white-space: nowrap;
  }

  .q-line-val strong {
    color: var(--text);
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }

  .q-line.is-warn .q-line-val strong {
    color: var(--warn);
  }

  .q-line.is-hot .q-line-val strong {
    color: var(--danger);
  }

  .q-line-reset {
    color: var(--faint);
  }

  /* Detalle del agente apuntado: nombre, plan y una línea por ventana. */
  .q-detail {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    margin-top: 0.5rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--line);
  }

  .q-detail-head {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
  }

  .q-detail-name {
    color: var(--text);
    font-weight: 600;
  }

  .q-detail-meta {
    margin-left: auto;
    color: var(--faint);
    font-size: 0.6875rem;
  }

  .q-detail-error {
    margin: 0;
    color: var(--warn);
    font-size: 0.6875rem;
    line-height: 1.4;
    text-wrap: pretty;
  }

  .q-detail-note {
    margin: 0;
    color: var(--muted);
    font-size: 0.6875rem;
    line-height: 1.4;
  }

  /* Etiqueta fija: todas las pistas arrancan y terminan en la misma columna. */
  .q-bar {
    display: grid;
    align-items: center;
    grid-template-columns: 5.25rem minmax(0, 1fr) minmax(4.75rem, max-content);
    gap: 0.4rem;
  }

  .q-win {
    overflow: hidden;
    color: var(--muted);
    font-size: 0.6875rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .q-track {
    height: 0.3rem;
    overflow: hidden;
    border-radius: 999px;
    background: color-mix(in sRGB, var(--text) 13%, transparent);
  }

  .q-fill {
    display: block;
    height: 100%;
    border-radius: 999px;
    background: var(--accent);
  }

  .q-bar.is-warn .q-fill {
    background: var(--warn);
  }

  .q-bar.is-hot .q-fill {
    background: var(--danger);
  }

  /* El % manda; el reinicio acompaña como texto, no como columna. */
  .q-val {
    display: flex;
    align-items: baseline;
    justify-content: flex-end;
    gap: 0.25rem;
    white-space: nowrap;
  }

  .q-pct {
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }

  .q-bar.is-warn .q-pct {
    color: var(--warn);
  }

  .q-bar.is-hot .q-pct {
    color: var(--danger);
  }

  .q-sep,
  .q-reset {
    color: var(--faint);
    font-size: 0.6875rem;
  }

  .q-fallback {
    color: var(--muted);
    font-size: 0.6875rem;
    line-height: 1.4;
  }

  @media (prefers-reduced-motion: reduce) {
    .q-panel.float-emerge.is-shown {
      pointer-events: auto;
      animation: none;
    }

    /* Sin viaje espacial ni arcos animados: el estado cambia igual. */
    .q-panel.is-side.is-shown .q-detail,
    .q-ring-track,
    .q-ring-fill,
    .q-agent-pct {
      transition: none;
    }
  }
</style>
