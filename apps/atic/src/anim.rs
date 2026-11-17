use std::time::{Duration, Instant};

/// Curva `cubic-bezier(x1, y1, x2, y2)` como en CSS: resuelve x(t) = progreso y
/// devuelve y(t). Admite y fuera de [0, 1] para curvas con rebote.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    let bezier = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    let mut t = progress;
    for _ in 0..24 {
        let x = bezier(x1, x2, t);
        if (x - progress).abs() < 1e-4 {
            break;
        }
        if x < progress {
            low = t;
        } else {
            high = t;
        }
        t = (low + high) / 2.0;
    }
    bezier(y1, y2, t)
}

/// `--ease-island`: abre y cierra la isla con un leve sobrepaso.
pub fn ease_island(t: f32) -> f32 {
    cubic_bezier(0.33, 1.38, 0.46, 1.0, t)
}

/// `--ease-smooth-out`.
pub fn ease_smooth_out(t: f32) -> f32 {
    cubic_bezier(0.22, 1.0, 0.36, 1.0, t)
}

/// Salida con rebote para que las gotas se asienten en su sitio.
pub fn ease_back_out(t: f32) -> f32 {
    cubic_bezier(0.34, 1.56, 0.64, 1.0, t)
}

pub fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

/// Progreso de un tramo `[start, start + duration]` dentro de una línea de tiempo.
pub fn segment(time: f32, start: f32, duration: f32) -> f32 {
    ((time - start) / duration).clamp(0.0, 1.0)
}

/// Cuánto más lento va todo lo que se mueve en la pill (los `Tween` y las
/// líneas de tiempo del notch), frente a los tiempos escritos en el código.
/// Las curvas hacen casi todo el recorrido al principio: con los tiempos de
/// la web la forma llegaba de golpe. El hover no se estira (`Tween::hover`):
/// lento, responde tarde.
pub const SLOW: f32 = 1.5;

/// Valor interpolado que se puede redirigir a mitad de camino sin saltos.
pub struct Tween {
    from: f32,
    to: f32,
    start: Instant,
    duration: Duration,
    ease: fn(f32) -> f32,
}

impl Tween {
    pub fn new(value: f32, duration: Duration, ease: fn(f32) -> f32) -> Self {
        Self::hover(value, duration.mul_f32(SLOW), ease)
    }

    /// Como `new`, con la duración tal cual (sin `SLOW`): para el hover.
    pub fn hover(value: f32, duration: Duration, ease: fn(f32) -> f32) -> Self {
        Self {
            from: value,
            to: value,
            start: Instant::now(),
            duration,
            ease,
        }
    }

    pub fn value(&self, now: Instant) -> f32 {
        let progress =
            now.saturating_duration_since(self.start).as_secs_f32() / self.duration.as_secs_f32();
        lerp(self.from, self.to, (self.ease)(progress.clamp(0.0, 1.0)))
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn set(&mut self, target: f32, now: Instant) {
        if self.to == target {
            return;
        }
        self.from = self.value(now);
        self.to = target;
        self.start = now;
    }

    /// Salta al valor sin animar.
    pub fn snap(&mut self, value: f32) {
        self.from = value;
        self.to = value;
    }

    pub fn is_running(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) < self.duration
    }
}

// --- Resortes (estilo M3 Expressive) ---------------------------------------
//
// El estilo Atic anima con `Tween` (duración y curva fijas). El estilo M3
// Expressive anima lo mismo con resortes: se pueden redirigir a mitad de
// camino conservando la velocidad, sin el quiebre de un `Tween` que vuelve a
// empezar su curva. Las dos cosas comparten API (`value`, `set`, `snap`,
// `target`, `is_running`), así que leer un valor no cambia con el estilo:
// `Anim` lleva los dos y usa el que toca.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui_m3::Spring;

/// El sistema pide reducir el movimiento: los resortes saltan a su destino.
/// La pill lo deja al día en cada cuadro (`set_reduced_motion`); se lee sin
/// `cx` porque `SpringTrack::set` no lo tiene.
static REDUCED: AtomicBool = AtomicBool::new(false);

pub fn set_reduced_motion(reduced: bool) {
    REDUCED.store(reduced, Ordering::Relaxed);
}

pub fn reduced_motion() -> bool {
    REDUCED.load(Ordering::Relaxed)
}

/// Cuánto más lento corren los resortes: 1 normal. Solo para revisar
/// transiciones a cámara lenta (`numbers."pill.spring.time_scale"` en
/// `tweaks.json`, sin compilar). A los resortes no se les aplica `SLOW`: sus
/// tiempos ya son los de M3 (`SPATIAL_SLOW` se asienta en ~0,6 s, el ritmo
/// aprobado para la pill).
pub fn spring_time_scale() -> f32 {
    crate::tweak::num("pill.spring.time_scale", 1.0).max(0.01)
}

/// Los seis resortes de M3 Expressive (`gpui_m3::Spring`), con nombre para
/// poder afinarlos en vivo: `tweaks.json` →
/// `numbers."pill.spring.<grupo>.stiffness"` y `."<grupo>.damping"`, con
/// `<grupo>` = `spatial_fast`, `spatial`, `spatial_slow`, `effects_fast`,
/// `effects` o `effects_slow`. El ajuste vale desde el próximo `set`.
///
/// - Espaciales (con un leve sobrepaso): posición, tamaño, forma.
/// - De efectos (sin sobrepaso): opacidad y color. Nunca uses uno espacial
///   para la opacidad: el sobrepaso la saca de rango.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpringKind {
    SpatialFast,
    Spatial,
    SpatialSlow,
    EffectsFast,
    Effects,
    EffectsSlow,
}

impl SpringKind {
    /// Los valores de M3, sin ajustes.
    pub fn base(self) -> Spring {
        match self {
            SpringKind::SpatialFast => Spring::SPATIAL_FAST,
            SpringKind::Spatial => Spring::SPATIAL,
            SpringKind::SpatialSlow => Spring::SPATIAL_SLOW,
            SpringKind::EffectsFast => Spring::EFFECTS_FAST,
            SpringKind::Effects => Spring::EFFECTS,
            SpringKind::EffectsSlow => Spring::EFFECTS_SLOW,
        }
    }

    /// El nombre del grupo en `tweaks.json`.
    pub fn key(self) -> &'static str {
        match self {
            SpringKind::SpatialFast => "spatial_fast",
            SpringKind::Spatial => "spatial",
            SpringKind::SpatialSlow => "spatial_slow",
            SpringKind::EffectsFast => "effects_fast",
            SpringKind::Effects => "effects",
            SpringKind::EffectsSlow => "effects_slow",
        }
    }

    /// El resorte con lo que diga `tweaks.json` (en release, el de M3).
    pub fn spring(self) -> Spring {
        let base = self.base();
        let key = self.key();
        Spring {
            stiffness: crate::tweak::num(&format!("pill.spring.{key}.stiffness"), base.stiffness).max(1.0),
            damping_ratio: crate::tweak::num(&format!("pill.spring.{key}.damping"), base.damping_ratio).max(0.05),
        }
    }
}

/// Segundos (de resorte) hasta que lo que falta, partiendo de `x0` con
/// velocidad `v0`, queda bajo una milésima del recorrido: desde ahí el valor
/// salta al destino y deja de pedir cuadros. Usa la envolvente de la
/// oscilación, así que nunca corta antes de tiempo.
fn settle_after(spring: Spring, x0: f32, v0: f32) -> f32 {
    let w0 = spring.stiffness.max(1.0).sqrt();
    let z = spring.damping_ratio;
    let (amplitude, rate) = if z < 1.0 {
        let wd = w0 * (1.0 - z * z).sqrt();
        let b = (v0 + z * w0 * x0) / wd;
        ((x0 * x0 + b * b).sqrt(), z * w0)
    } else {
        // Crítico (`Spring::solve` trata igual a los sobreamortiguados):
        // (x0 + b·t)·e^(-w0·t) ≤ (|x0| + 2|b|/(e·w0))·e^(-w0·t/2).
        let b = v0 + w0 * x0;
        (x0.abs() + 2.0 * b.abs() / (std::f32::consts::E * w0), w0 / 2.0)
    };
    let tolerance = (amplitude * 1e-3).max(1e-4);
    if amplitude <= tolerance {
        return 0.0;
    }
    (amplitude / tolerance).ln() / rate
}

/// Un progreso de 0 a 1 con forma de resorte, `elapsed` después de empezar:
/// para líneas de tiempo con tramos (`segment`) como el cambio de nivel o el
/// vuelo al notch. Se queda en 1 desde que llega la primera vez (el sobrepaso
/// no se ve: los tramos ya terminaron), y con movimiento reducido es 1 al tiro.
pub fn spring_progress(kind: SpringKind, elapsed: Duration) -> f32 {
    if reduced_motion() {
        return 1.0;
    }
    let spring = kind.spring();
    let t = elapsed.as_secs_f32() / spring_time_scale();
    if t >= settle_after(spring, 1.0, 0.0) {
        return 1.0;
    }
    spring.step(t).clamp(0.0, 1.0)
}

/// Un valor que sigue a su destino con un resorte, como `Tween` pero físico.
///
/// Es analítico: guarda desde dónde partió (respecto del destino), con qué
/// velocidad y cuándo; `value(now)` resuelve el resorte en ese instante, así
/// que leer no cambia nada (`&self`) y da lo mismo cada cuántos cuadros se lea.
/// `set` a mitad de camino parte del valor y la velocidad de ese momento: no
/// hay saltos ni quiebres al cambiar de idea.
///
/// - `new(valor, SpringKind)`: quieto en `valor`.
/// - `.leaving(SpringKind)`: otro resorte para cuando baja (el destino nuevo es
///   menor que el valor de ahora). Para lo que sale más rápido que lo que
///   entra, como el contenido del notch (`Effects` al entrar, `EffectsFast` al
///   salir).
/// - `set(destino, now)`: hacia allá. Si ya iba hacia ese destino, no hace
///   nada (como `Tween::set`). Con movimiento reducido, salta.
/// - `snap(valor)`: salta sin animar.
/// - `is_running(now)`: si todavía se mueve (para pedir cuadros).
///
/// Los espaciales pasan un poco del destino (1,5 % con `Spatial`): quien lea
/// un 0..1 para algo que no puede pasarse (opacidad, un ancho mínimo) lo
/// limita con `clamp`, como ya se hace con `ease_island`.
#[derive(Clone, Debug)]
pub struct SpringTrack {
    kind: SpringKind,
    leave: Option<SpringKind>,
    /// El resorte del recorrido actual (tomado en `set`, con los ajustes).
    spring: Spring,
    target: f32,
    /// Al empezar el recorrido: cuánto faltaba (valor − destino) y la velocidad.
    offset: f32,
    velocity: f32,
    start: Instant,
    /// Segundos (de resorte) hasta asentarse.
    settle: f32,
}

impl SpringTrack {
    pub fn new(value: f32, kind: SpringKind) -> Self {
        Self {
            kind,
            leave: None,
            spring: kind.base(),
            target: value,
            offset: 0.0,
            velocity: 0.0,
            start: Instant::now(),
            settle: 0.0,
        }
    }

    /// Usa `kind` cuando el valor baja.
    pub fn leaving(mut self, kind: SpringKind) -> Self {
        self.leave = Some(kind);
        self
    }

    fn elapsed(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.start).as_secs_f32() / spring_time_scale()
    }

    pub fn value(&self, now: Instant) -> f32 {
        let t = self.elapsed(now);
        if t >= self.settle {
            return self.target;
        }
        self.target + self.spring.solve(self.offset, self.velocity, t).0
    }

    /// Unidades por segundo de resorte.
    pub fn velocity(&self, now: Instant) -> f32 {
        let t = self.elapsed(now);
        if t >= self.settle {
            return 0.0;
        }
        self.spring.solve(self.offset, self.velocity, t).1
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn set(&mut self, target: f32, now: Instant) {
        self.set_with(target, now, reduced_motion());
    }

    fn set_with(&mut self, target: f32, now: Instant, reduced: bool) {
        if self.target == target {
            return;
        }
        if reduced {
            self.snap(target);
            return;
        }
        let value = self.value(now);
        let velocity = self.velocity(now);
        let kind = match self.leave {
            Some(leave) if target < value => leave,
            _ => self.kind,
        };
        self.spring = kind.spring();
        self.target = target;
        self.offset = value - target;
        self.velocity = velocity;
        self.start = now;
        self.settle = settle_after(self.spring, self.offset, self.velocity);
    }

    /// Salta al valor sin animar.
    pub fn snap(&mut self, value: f32) {
        self.target = value;
        self.offset = 0.0;
        self.velocity = 0.0;
        self.settle = 0.0;
    }

    pub fn is_running(&self, now: Instant) -> bool {
        self.elapsed(now) < self.settle
    }
}

/// Un valor animado que cambia de motor con el estilo de la pill: `Tween` en
/// Atic (idéntico a antes) y `SpringTrack` en M3 Expressive. Misma API que los
/// dos, así que quien lo lee no sabe cuál corre.
///
/// Arranca en Atic. La pill llama a `restyle` al cambiar de estilo: el motor
/// nuevo parte del valor que se veía y sigue hacia el mismo destino.
pub struct Anim {
    tween: Tween,
    spring: SpringTrack,
    springy: bool,
}

impl Anim {
    /// El `Tween` del estilo Atic y el resorte del Expressive, los dos en el
    /// mismo valor.
    pub fn new(tween: Tween, spring: SpringTrack) -> Self {
        Self { tween, spring, springy: false }
    }

    /// Lo mismo, con el resorte `kind` quieto donde empieza el `Tween`.
    pub fn of(tween: Tween, kind: SpringKind) -> Self {
        let spring = SpringTrack::new(tween.target(), kind);
        Self::new(tween, spring)
    }

    /// Pasa al resorte (`true`) o al `Tween`, desde el valor de ahora.
    pub fn restyle(&mut self, springy: bool, now: Instant) {
        if self.springy == springy {
            return;
        }
        let (value, target) = (self.value(now), self.target());
        if springy {
            self.spring.snap(value);
            self.spring.set(target, now);
        } else {
            self.tween.snap(value);
            self.tween.set(target, now);
        }
        self.springy = springy;
    }

    pub fn value(&self, now: Instant) -> f32 {
        if self.springy {
            self.spring.value(now)
        } else {
            self.tween.value(now)
        }
    }

    pub fn target(&self) -> f32 {
        if self.springy {
            self.spring.target()
        } else {
            self.tween.target()
        }
    }

    pub fn set(&mut self, target: f32, now: Instant) {
        if self.springy {
            self.spring.set(target, now);
        } else {
            self.tween.set(target, now);
        }
    }

    pub fn snap(&mut self, value: f32) {
        // Los dos: el que no corre queda listo por si cambia el estilo.
        self.tween.snap(value);
        self.spring.snap(value);
    }

    pub fn is_running(&self, now: Instant) -> bool {
        if self.springy {
            self.spring.is_running(now)
        } else {
            self.tween.is_running(now)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn el_resorte_llega_al_destino_y_se_detiene() {
        let t0 = Instant::now();
        let mut track = SpringTrack::new(0.0, SpringKind::SpatialSlow);
        assert!(!track.is_running(t0));
        track.set_with(1.0, t0, false);
        assert_eq!(track.value(t0), 0.0);
        assert!(track.is_running(t0 + ms(100)));
        let mid = track.value(t0 + ms(150));
        assert!(mid > 0.0 && mid < 1.0, "a medio camino: {mid}");
        // `SPATIAL_SLOW` se asienta en ~0,6 s: a los 2 s está quieto en 1.
        assert_eq!(track.value(t0 + ms(2000)), 1.0);
        assert!(!track.is_running(t0 + ms(2000)));
        assert_eq!(track.target(), 1.0);
    }

    #[test]
    fn el_espacial_pasa_un_poco_y_el_de_efectos_no() {
        let t0 = Instant::now();
        let mut spatial = SpringTrack::new(0.0, SpringKind::Spatial);
        let mut effects = SpringTrack::new(0.0, SpringKind::Effects);
        spatial.set_with(1.0, t0, false);
        effects.set_with(1.0, t0, false);
        let peak = |track: &SpringTrack| (0..200).map(|i| track.value(t0 + ms(i * 5))).fold(0.0, f32::max);
        assert!(peak(&spatial) > 1.001 && peak(&spatial) < 1.05);
        assert!(peak(&effects) <= 1.0);
    }

    #[test]
    fn con_movimiento_reducido_salta() {
        let t0 = Instant::now();
        let mut track = SpringTrack::new(0.0, SpringKind::Spatial);
        track.set_with(1.0, t0, true);
        assert_eq!(track.value(t0), 1.0);
        assert!(!track.is_running(t0));
    }

    #[test]
    fn interrumpir_sigue_desde_donde_iba_y_con_su_velocidad() {
        let t0 = Instant::now();
        let mut track = SpringTrack::new(0.0, SpringKind::Spatial);
        track.set_with(1.0, t0, false);
        let t1 = t0 + ms(80);
        let (before, speed) = (track.value(t1), track.velocity(t1));
        assert!(speed > 0.0);
        track.set_with(0.0, t1, false);
        assert!((track.value(t1) - before).abs() < 1e-4, "sin salto al cambiar de idea");
        assert!((track.velocity(t1) - speed).abs() < 1e-3, "conserva la velocidad");
        // Por la inercia sigue un poco hacia arriba antes de volver.
        assert!(track.value(t1 + ms(10)) > before);
        assert_eq!(track.value(t1 + ms(3000)), 0.0);
    }

    #[test]
    fn el_mismo_destino_no_reinicia() {
        let t0 = Instant::now();
        let mut track = SpringTrack::new(0.0, SpringKind::Spatial);
        track.set_with(1.0, t0, false);
        let later = t0 + ms(120);
        let before = track.value(later);
        track.set_with(1.0, later, false);
        assert_eq!(track.value(later), before);
        assert_eq!(track.value(later + ms(50)), track.value(later + ms(50)));
    }

    #[test]
    fn al_bajar_usa_el_resorte_de_salida() {
        let t0 = Instant::now();
        let mut slow = SpringTrack::new(1.0, SpringKind::Effects);
        let mut quick = SpringTrack::new(1.0, SpringKind::Effects).leaving(SpringKind::EffectsFast);
        slow.set_with(0.0, t0, false);
        quick.set_with(0.0, t0, false);
        let at = t0 + ms(40);
        assert!(quick.value(at) < slow.value(at));
    }

    #[test]
    fn el_progreso_de_resorte_llega_a_uno() {
        assert_eq!(spring_progress(SpringKind::SpatialSlow, Duration::ZERO), 0.0);
        let mid = spring_progress(SpringKind::SpatialSlow, ms(120));
        assert!(mid > 0.0 && mid < 1.0);
        assert_eq!(spring_progress(SpringKind::SpatialSlow, ms(2000)), 1.0);
    }

    #[test]
    fn anim_cambia_de_motor_sin_salto() {
        let t0 = Instant::now();
        let mut anim = Anim::new(
            Tween::new(0.0, ms(240), ease_island),
            SpringTrack::new(0.0, SpringKind::Spatial),
        );
        anim.set(1.0, t0);
        let at = t0 + ms(100);
        let before = anim.value(at);
        anim.restyle(true, at);
        assert!((anim.value(at) - before).abs() < 1e-4);
        assert_eq!(anim.target(), 1.0);
        assert!(anim.is_running(at));
        anim.restyle(false, at + ms(50));
        assert_eq!(anim.target(), 1.0);
    }
}
