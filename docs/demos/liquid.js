/**
 * Fusión líquida para las demos de `docs/demos`.
 *
 * La idea es una sola: dos formas que se acercan no se solapan, se **funden**
 * con un cuello cóncavo, y al separarse el cuello se afina hasta cortarse.
 * Con una perilla —la viscosidad— se pasa de juntas nítidas a gotas gomosas.
 *
 * No es simulación de fluidos: es el truco de metaballs con filtros SVG, que
 * la app YA usa en el cuello de la burbuja de agentes (`bub-liquid-goo` en
 * `routes/agents/+page.svelte`). Acá está generalizado y con la perilla afuera.
 *
 * Script clásico a propósito: los módulos ES no cargan por `file://`, y estas
 * demos se abren haciendo doble clic.
 */
(function (global) {
  "use strict";

  var SVG_NS = "http://www.w3.org/2000/svg";
  var seq = 0;
  var defsSvg = null;

  var reducedMotion =
    typeof matchMedia === "function" &&
    matchMedia("(prefers-reduced-motion: reduce)").matches;

  /** Un solo `<svg>` oculto guarda todos los filtros de la página. */
  function defs() {
    if (defsSvg) return defsSvg;
    var svg = document.createElementNS(SVG_NS, "svg");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("focusable", "false");
    svg.setAttribute("width", "0");
    svg.setAttribute("height", "0");
    svg.style.cssText =
      "position:absolute;width:0;height:0;overflow:hidden;pointer-events:none";
    defsSvg = document.createElementNS(SVG_NS, "defs");
    svg.appendChild(defsSvg);
    document.body.appendChild(svg);
    return defsSvg;
  }

  function el(name, attrs) {
    var node = document.createElementNS(SVG_NS, name);
    for (var key in attrs) {
      if (Object.prototype.hasOwnProperty.call(attrs, key)) {
        node.setAttribute(key, String(attrs[key]));
      }
    }
    return node;
  }

  // ───────────────────────────────────────────────────────────────────────
  // El filtro
  // ───────────────────────────────────────────────────────────────────────

  /**
   * Cómo funciona, porque es lo único que hay que entender:
   *
   *   1. `feGaussianBlur` difumina el grupo entero. Cada forma queda con un
   *      halo de alfa que se desvanece hacia afuera.
   *   2. `feColorMatrix` endurece ese alfa: `a' = gain·a + bias`, recortado a
   *      [0,1]. Todo lo que quede por encima del umbral `-bias/gain` vuelve a
   *      ser opaco; el resto desaparece.
   *
   *   Entre dos formas cercanas los dos halos SE SUMAN. Donde la suma pasa el
   *   umbral, el hueco se rellena: eso es el cuello. Al alejarse, la suma cae,
   *   el cuello se afina y se corta. La viscosidad no es más que el radio del
   *   difuminado — cuanto más ancho el halo, más lejos llega el cuello.
   *
   *   3. `feComposite atop` devuelve el gráfico nítido encima, para que el
   *      endurecido no coma los bordes de las formas originales.
   *
   * DOS CONSECUENCIAS que mandan sobre el diseño y no son opcionales:
   *
   *   - Solo se funde lo que está DENTRO del elemento filtrado, y todo lo que
   *     esté ahí adentro pasa por el difuminado. El texto y los iconos tienen
   *     que vivir en una capa aparte, encima. Las demos lo hacen así: una capa
   *     de siluetas (fundida) y una capa de contenido (intacta).
   *   - Las formas que se funden tienen que ser del MISMO color. El cuello lo
   *     pinta el difuminado, que es el promedio de lo que tenga cerca; con dos
   *     colores distintos la unión sale con una franja sucia en el medio.
   */
  var GAIN = 18;
  var BIAS = -7;

  /**
   * Aplica el filtro de fusión a un contenedor.
   *
   * `sigma` es la viscosidad, en píxeles: 0 = juntas nítidas (sin filtro),
   * alto = gotas gomosas. El valor de la app es 5.
   */
  function goo(container, options) {
    var opts = options || {};
    var gain = opts.gain == null ? GAIN : opts.gain;
    var bias = opts.bias == null ? BIAS : opts.bias;
    var id = opts.id || "goo-" + ++seq;
    var sigma = opts.sigma == null ? 5 : opts.sigma;

    var filter = el("filter", {
      id: id,
      // Región generosa: el cuello vive FUERA de la caja de las formas, y con
      // la región por defecto (-10%/120%) se recorta en cuanto la viscosidad
      // sube. Se ve como un cuello con la punta cortada en recto.
      x: "-50%",
      y: "-50%",
      width: "200%",
      height: "200%",
      "color-interpolation-filters": "sRGB",
    });

    var blur = el("feGaussianBlur", {
      in: "SourceGraphic",
      stdDeviation: sigma,
      result: "blur",
    });
    filter.appendChild(blur);

    // `type` y no `mode`: el atributo de `feColorMatrix` es `type`. La app
    // escribe `mode="matrix"` en `bub-liquid-goo` y anda igual, pero por
    // accidente — `matrix` es el valor por defecto, así que el atributo mal
    // escrito se ignora y cae en el mismo comportamiento.
    filter.appendChild(
      el("feColorMatrix", {
        in: "blur",
        type: "matrix",
        values: "1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 " + gain + " " + bias,
        result: "goo",
      }),
    );
    filter.appendChild(
      el("feComposite", { in: "SourceGraphic", in2: "goo", operator: "atop" }),
    );

    defs().appendChild(filter);
    apply();

    function apply() {
      // Sigma 0 no es "difuminar cero": con el endurecido puesto igual come
      // medio píxel de cada borde. Sin filtro es sin filtro.
      var chain = sigma > 0 ? "url(#" + id + ")" : "";
      // La sombra va DESPUÉS del goo en la misma cadena, y ese orden es todo:
      // así cae sobre la silueta ya fundida —cuello incluido— en vez de que
      // cada forma traiga la suya y se crucen en la unión.
      //
      // Y sobre todo: una sombra puesta con `box-shadow` en cada forma entra
      // al filtro como parte del gráfico, el endurecido la lee como alfa
      // parcial y la usa para alargar el cuello. El resultado es que la
      // fusión ocurre más lejos de lo que se ve, sin motivo aparente.
      if (opts.shadow) chain = (chain ? chain + " " : "") + opts.shadow;
      container.style.filter = chain || "none";
    }

    return {
      /** Cambia la viscosidad en caliente, sin rearmar el filtro. */
      set: function (next) {
        sigma = next;
        blur.setAttribute("stdDeviation", sigma);
        apply();
      },
      get sigma() {
        return sigma;
      },
      /** Distancia a la que este filtro corta el cuello, en px. */
      get reach() {
        return reach(sigma, gain, bias);
      },
      id: id,
      destroy: function () {
        if (filter.parentNode) filter.parentNode.removeChild(filter);
        container.style.filter = "";
      },
    };
  }

  // ───────────────────────────────────────────────────────────────────────
  // Hasta dónde llega el cuello
  // ───────────────────────────────────────────────────────────────────────

  /** erf, Abramowitz & Stegun 7.1.26. Sobra para lo que se usa acá. */
  function erf(z) {
    var sign = z < 0 ? -1 : 1;
    z = Math.abs(z);
    var t = 1 / (1 + 0.3275911 * z);
    var y =
      1 -
      ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) *
        t +
        0.254829592) *
        t *
        Math.exp(-z * z);
    return sign * y;
  }

  /** Φ: la normal acumulada. */
  function phi(x) {
    return 0.5 * (1 + erf(x / Math.SQRT2));
  }

  /**
   * Alcance de la fusión: el hueco máximo, en px, que el cuello todavía cruza.
   *
   * Para un borde recto, el alfa difuminado a distancia `d` por fuera del
   * borde vale `Φ(-d/σ)`. En el medio de un hueco `g` cada uno de los dos
   * bordes aporta `Φ(-g/2σ)`, así que el alfa del medio es `2·Φ(-g/2σ)` y el
   * cuello existe mientras eso supere el umbral `-bias/gain`.
   *
   * Con los valores de la app (18 y −7) da **1.72 σ**: con σ = 5, el cuello se
   * corta a unos 8.6 px de hueco. Por eso las dos gotas del cuello de la
   * burbuja se solapan en vez de tocarse apenas.
   *
   * Es el caso de bordes rectos. Entre dos formas redondas el alcance real es
   * algo menor, porque la curvatura le quita área al halo.
   */
  function reach(sigma, gain, bias) {
    if (!(sigma > 0)) return 0;
    var threshold = -(bias == null ? BIAS : bias) / (gain == null ? GAIN : gain);
    if (!(threshold > 0 && threshold < 2)) return 0;
    // 2·Φ(-g/2σ) decrece con g: bisección directa.
    var lo = 0;
    var hi = 12 * sigma;
    for (var i = 0; i < 60; i++) {
      var mid = (lo + hi) / 2;
      if (2 * phi(-mid / (2 * sigma)) > threshold) lo = mid;
      else hi = mid;
    }
    return (lo + hi) / 2;
  }

  // ───────────────────────────────────────────────────────────────────────
  // Movimiento
  // ───────────────────────────────────────────────────────────────────────

  /**
   * `cubic-bezier(x1,y1,x2,y2)` como función de JS.
   *
   * Las demos calculan la geometría cuadro a cuadro —el cuello depende de la
   * distancia exacta, no hay propiedad CSS que lo interpole—, así que las
   * curvas de `app.css` hay que evaluarlas a mano o el movimiento no sería el
   * de la app. Newton sobre x, que converge en tres o cuatro pasos.
   */
  function ease(x1, y1, x2, y2) {
    var cx = 3 * x1;
    var bx = 3 * (x2 - x1) - cx;
    var ax = 1 - cx - bx;
    var cy = 3 * y1;
    var by = 3 * (y2 - y1) - cy;
    var ay = 1 - cy - by;

    function sampleX(t) {
      return ((ax * t + bx) * t + cx) * t;
    }
    function slopeX(t) {
      return (3 * ax * t + 2 * bx) * t + cx;
    }

    return function (x) {
      if (x <= 0) return 0;
      if (x >= 1) return 1;
      var t = x;
      for (var i = 0; i < 8; i++) {
        var err = sampleX(t) - x;
        if (Math.abs(err) < 1e-6) break;
        var d = slopeX(t);
        if (Math.abs(d) < 1e-6) break;
        t -= err / d;
      }
      return ((ay * t + by) * t + cy) * t;
    };
  }

  /** Las curvas de `app.css`, con su nombre. */
  var EASE = {
    /* --morph-ease: abrir es el momento expresivo, con rebote. */
    morph: ease(0.34, 1.25, 0.64, 1),
    /* --morph-close-ease: cerrar es utilitario y se quita del camino. */
    close: ease(0.22, 1, 0.36, 1),
    linear: function (x) {
      return x;
    },
  };

  /**
   * Anima `t` de 0 a 1 (o al revés) llamando a `onFrame` en cada cuadro.
   * Devuelve una función para cancelar: si llega otra animación a mitad de
   * camino, la anterior tiene que soltar el control y no seguir escribiendo.
   */
  function animate(opts) {
    var from = opts.from == null ? 0 : opts.from;
    var to = opts.to == null ? 1 : opts.to;
    var curve = opts.ease || EASE.linear;
    var duration = reducedMotion ? 0 : opts.duration;
    var start = 0;
    var raf = 0;
    var live = true;

    function step(now) {
      if (!live) return;
      if (!start) start = now;
      var x = duration > 0 ? Math.min(1, (now - start) / duration) : 1;
      opts.onFrame(from + (to - from) * curve(x));
      if (x < 1) raf = requestAnimationFrame(step);
      else if (opts.onDone) opts.onDone();
    }

    raf = requestAnimationFrame(step);
    return function cancel() {
      live = false;
      cancelAnimationFrame(raf);
    };
  }

  // ───────────────────────────────────────────────────────────────────────
  // Escenografía
  // ───────────────────────────────────────────────────────────────────────

  /**
   * El fondo.
   *
   * Calmo a propósito, al revés de lo que pediría una demo de vidrio: acá lo
   * que hay que mirar es la silueta —dónde nace el cuello, cuándo se corta— y
   * un fondo con detalle solo compite con ella.
   */
  function desk(host) {
    var target = host || document.body;
    var wrap = document.createElement("div");
    wrap.className = "desk-set";
    wrap.innerHTML = '<div class="desk"></div><div class="desk-grid"></div>';
    target.insertBefore(wrap, target.firstChild);
    return wrap;
  }

  // ───────────────────────────────────────────────────────────────────────
  // Controles
  // ───────────────────────────────────────────────────────────────────────

  /**
   * Barra de controles declarativa: cada demo describe sus perillas y no
   * vuelve a escribir el mismo `addEventListener`.
   */
  /** Cada rango recuerda cómo repintar su etiqueta (ver `setRange`). */
  var syncers = new WeakMap();

  /**
   * Mueve una perilla desde el código, sin disparar su handler.
   *
   * La reproducción arrastra el control de progreso para que no mienta sobre
   * dónde está la transición. Si lo hiciera despachando `input`, el handler
   * cancelaría la animación que lo está moviendo; y si solo escribiera
   * `.value`, la etiqueta numérica se quedaría en el valor viejo.
   */
  function setRange(input, value) {
    if (!input) return;
    input.value = value;
    var show = syncers.get(input);
    if (show) show();
  }

  function controls(spec) {
    var bar = document.createElement("div");
    bar.className = "bar";

    spec.forEach(function (item) {
      if (item.type === "sep") {
        var sep = document.createElement("span");
        sep.className = "sep";
        bar.appendChild(sep);
        return;
      }

      if (item.type === "range") {
        var field = document.createElement("label");
        field.className = "field";
        var name = document.createElement("span");
        name.className = "field-l";
        var out = document.createElement("b");
        var input = document.createElement("input");
        input.type = "range";
        input.min = item.min;
        input.max = item.max;
        input.step = item.step || 1;
        input.value = item.value;
        name.textContent = item.label + " ";
        name.appendChild(out);
        var show = function () {
          out.textContent = input.value + (item.unit || "");
        };
        show();
        syncers.set(input, show);
        input.addEventListener("input", function () {
          show();
          item.on(parseFloat(input.value));
        });
        field.appendChild(name);
        field.appendChild(input);
        bar.appendChild(field);
        if (item.fire) item.on(parseFloat(input.value));
        return;
      }

      var button = document.createElement("button");
      button.type = "button";
      button.textContent = item.label;
      if (item.type === "toggle") {
        var on = !!item.value;
        var sync = function () {
          button.textContent = on ? item.labelOn || item.label : item.label;
          button.classList.toggle("on", on);
        };
        sync();
        button.addEventListener("click", function () {
          on = !on;
          sync();
          item.on(on);
        });
        if (item.value) item.on(on);
      } else {
        button.addEventListener("click", item.on);
      }
      bar.appendChild(button);
    });

    document.body.appendChild(bar);
    return bar;
  }

  /** Tema claro/oscuro sincronizado con `<html data-theme>`, como la app. */
  function theme(initial) {
    var root = document.documentElement;
    var dark =
      initial != null
        ? initial
        : global.location && global.location.hash.indexOf("oscuro") >= 0;
    root.dataset.theme = dark ? "dark" : "light";
    return {
      get dark() {
        return root.dataset.theme === "dark";
      },
      set: function (value) {
        root.dataset.theme = value ? "dark" : "light";
      },
    };
  }

  /**
   * Botón para plegar la nota.
   *
   * Es texto largo clavado en una esquina, y en pantallas bajas se cruza con
   * lo único que la demo tiene que mostrar.
   */
  function note() {
    var box = document.querySelector(".note");
    if (!box || box.querySelector(".note-x")) return;
    var button = document.createElement("button");
    button.type = "button";
    button.className = "note-x";
    button.textContent = "−";
    button.setAttribute("aria-label", "Plegar la nota");
    button.addEventListener("click", function () {
      var folded = box.classList.toggle("is-folded");
      button.textContent = folded ? "?" : "−";
      button.setAttribute("aria-label", folded ? "Ver la nota" : "Plegar la nota");
    });
    box.appendChild(button);
  }

  /**
   * Arrastre libre.
   *
   * El elemento puede llegar en flujo normal: la primera vez que se toma se
   * congela donde está y recién ahí pasa a absoluto. Así la distribución
   * inicial la decide el CSS —que sabe cuánto mide cada pieza— y no una tabla
   * de porcentajes escrita a mano.
   */
  function drag(node, options) {
    var opts = options || {};
    var handle = opts.handle || node;
    var origin = null;
    handle.style.touchAction = "none";
    if (!opts.keepCursor) handle.style.cursor = "grab";

    handle.addEventListener("pointerdown", function (event) {
      if (event.button !== 0) return;
      if (event.target.closest("button, input, textarea, a, [data-no-drag]")) return;
      if (getComputedStyle(node).position === "static") {
        var left = node.offsetLeft;
        var top = node.offsetTop;
        node.style.position = "absolute";
        node.style.left = left + "px";
        node.style.top = top + "px";
      }
      origin = {
        x: event.clientX,
        y: event.clientY,
        left: node.offsetLeft,
        top: node.offsetTop,
      };
      handle.setPointerCapture(event.pointerId);
      if (!opts.keepCursor) handle.style.cursor = "grabbing";
      node.classList.add("is-dragging");
      event.preventDefault();
    });

    handle.addEventListener("pointermove", function (event) {
      if (!origin) return;
      var x = origin.left + (event.clientX - origin.x);
      var y = origin.top + (event.clientY - origin.y);
      if (opts.clamp) {
        var next = opts.clamp(x, y);
        x = next.x;
        y = next.y;
      }
      node.style.left = x + "px";
      node.style.top = y + "px";
      if (opts.onMove) opts.onMove(x, y);
    });

    var release = function () {
      if (!origin) return;
      origin = null;
      if (!opts.keepCursor) handle.style.cursor = "grab";
      node.classList.remove("is-dragging");
      if (opts.onEnd) opts.onEnd();
    };
    handle.addEventListener("pointerup", release);
    handle.addEventListener("pointercancel", release);
  }

  if (global.location && global.location.hash.indexOf("quieto") >= 0) {
    document.addEventListener("DOMContentLoaded", function () {
      document.body.classList.add("static");
    });
  }

  global.Liquid = {
    goo: goo,
    reach: reach,
    desk: desk,
    controls: controls,
    setRange: setRange,
    theme: theme,
    note: note,
    drag: drag,
    ease: ease,
    EASE: EASE,
    animate: animate,
    reducedMotion: reducedMotion,
    GAIN: GAIN,
    BIAS: BIAS,
  };
})(window);
