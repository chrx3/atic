//! Los tres niveles de la pill: a la vista, detrás de «Más» y fuera de ella.
//!
//! En una rueda cada gajo que se suma achica a todos los demás, así que elegir
//! cuáles están no es una preferencia cosmética: es lo que decide si se le
//! puede apuntar a una herramienta sin mirar. De ahí las tres salidas — se
//! esconde lo que no se usa nunca, y lo que se usa poco baja a un segundo
//! nivel en vez de robarle sitio al primero.
//!
//! La regla vive acá y no en el editor (`customize.rs`) porque lo que importa
//! son los casos raros —listas vacías, índices de herramientas que ya no
//! existen, una misma herramienta en las dos listas—, y eso no se comprueba
//! mirando la pill.
//!
//! Una herramienta es su índice en `CATALOG`. Sigue siendo un número y no un
//! enum para no romper los que ya la nombran por posición (`CLIPBOARD_TOOL`,
//! `capture::TOOL`, `peeks::any_peek`…), pero el índice es del **catálogo**, no
//! de la lista visible: la tira muestra un subconjunto y el orden lo elige la
//! persona.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub(crate) struct ToolDef {
    pub(crate) name: &'static str,
    pub(crate) icon: &'static str,
}

/// El catálogo. Su orden es el que usa el resto del código como índice, así
/// que no se reordena: se agrega al final y ya.
pub(crate) const CATALOG: [ToolDef; 10] = [
    ToolDef {
        name: "Reuniones",
        icon: "icons/circle-dot.svg",
    },
    ToolDef {
        name: "Clipboard",
        icon: "icons/clipboard.svg",
    },
    ToolDef {
        name: "Textos",
        icon: "icons/text-align-start.svg",
    },
    ToolDef {
        name: "Agentes",
        icon: "icons/square-terminal.svg",
    },
    ToolDef {
        name: "Sistema",
        icon: "icons/cpu.svg",
    },
    ToolDef {
        name: "Capturas",
        icon: "icons/crop.svg",
    },
    ToolDef {
        name: "Pizarra",
        icon: "icons/pencil.svg",
    },
    ToolDef {
        name: "Color",
        icon: "icons/pipette.svg",
    },
    ToolDef {
        name: "Flip",
        icon: "icons/flip.svg",
    },
    ToolDef {
        name: "Ahora suena",
        icon: "icons/audio-lines.svg",
    },
];

/// Cuántas herramientas hay. Es el tope de una fila del editor.
pub(crate) const COUNT: usize = CATALOG.len();

/// Cuántas fichas caben en la tira como mucho: el nivel a la vista entero más
/// la puerta a «Más».
pub(crate) const STRIP_MAX: usize = COUNT + 1;

pub(crate) fn name(tool: usize) -> &'static str {
    CATALOG.get(tool).map_or("?", |def| def.name)
}

pub(crate) fn icon(tool: usize) -> &'static str {
    CATALOG.get(tool).map_or("icons/circle.svg", |def| def.icon)
}

/// Dónde puede quedar una herramienta.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Bucket {
    /// A la vista: el primer nivel.
    Ring,
    /// Detrás de «Más»: el segundo nivel, el de los tres puntos.
    More,
    /// Fuera de la pill. Sigue existiendo, pero no tiene sitio en ella.
    Hidden,
}

/// En el orden en que se muestran y en el que las recorre el teclado.
pub(crate) const BUCKETS: [Bucket; 3] = [Bucket::Ring, Bucket::More, Bucket::Hidden];

pub(crate) fn bucket_title(bucket: Bucket) -> &'static str {
    match bucket {
        Bucket::Ring => "A la vista",
        Bucket::More => "Detrás de «Más»",
        Bucket::Hidden => "Fuera de la pill",
    }
}

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub(crate) struct Layout {
    /// Primer nivel, sin contar el gajo «Más».
    pub(crate) ring: Vec<usize>,
    /// Segundo nivel.
    pub(crate) more: Vec<usize>,
    /// Fuera de la pill. No se guarda: se deduce de las otras dos.
    pub(crate) hidden: Vec<usize>,
}

impl Layout {
    pub(crate) fn list(&self, bucket: Bucket) -> &[usize] {
        match bucket {
            Bucket::Ring => &self.ring,
            Bucket::More => &self.more,
            Bucket::Hidden => &self.hidden,
        }
    }
}

/// Los ids conocidos, en el orden pedido, sin repetir.
fn pick(ids: &[usize], taken: &[usize]) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::with_capacity(ids.len());
    for &id in ids {
        if id >= COUNT || taken.contains(&id) || out.contains(&id) {
            continue;
        }
        out.push(id);
    }
    out
}

/// Lee las dos listas guardadas y las vuelve una disposición.
///
/// Sin nada configurado —o con una config vieja de la que no sobrevivió ningún
/// índice— la pill trae el catálogo entero. Una rueda vacía no es una
/// preferencia, es una pill que no sirve para nada.
pub(crate) fn layout(ring: &[usize], more: &[usize]) -> Layout {
    let ring = pick(ring, &[]);
    let more = pick(more, &ring);

    if ring.is_empty() && more.is_empty() {
        return Layout {
            ring: (0..COUNT).collect(),
            more: Vec::new(),
            hidden: Vec::new(),
        };
    }

    // Todo en «Más» y nada en el anillo dejaría un único gajo que solo abre
    // otro nivel: dos pasos para llegar a cualquier cosa. Sube el submenú.
    let (ring, more) = if ring.is_empty() {
        (more, Vec::new())
    } else {
        (ring, more)
    };

    let hidden: Vec<usize> = (0..COUNT)
        .filter(|id| !ring.contains(id) && !more.contains(id))
        .collect();
    Layout { ring, more, hidden }
}

/// Catálogo que se ofreció después de que ya había ruedas personalizadas.
const OFFER_IN_RING: [usize; 2] = [crate::SISTEMA_TOOL, crate::MEDIA_TOOL];

/// Las dos listas que se guardan (`tools.json`).
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Prefs {
    pub(crate) ring: Vec<usize>,
    pub(crate) more: Vec<usize>,
    /// Las herramientas nuevas que ya se ofrecieron en el anillo. Se ofrecen
    /// una sola vez: si después las sacas, no vuelven.
    #[serde(default)]
    pub(crate) offered: Vec<usize>,
}

/// Una pill ya armada no esconde las herramientas nuevas: entran al anillo
/// para que se vean, una sola vez; el editor las mueve o las saca.
fn offer_newcomers(prefs: &Prefs) -> Layout {
    let mut layout = layout(&prefs.ring, &prefs.more);
    let configured = !prefs.ring.is_empty() || !prefs.more.is_empty();
    for id in OFFER_IN_RING {
        if configured && !prefs.offered.contains(&id) && layout.hidden.contains(&id) {
            layout.hidden.retain(|other| *other != id);
            layout.ring.push(id);
        }
    }
    layout
}

/// Coloca `id` en `to`, en la posición `index` de esa lista ya sin él (al final
/// si falta). Es la única regla de edición: la usan el editor de la pill y
/// cualquier otro, para que no puedan decidir distinto.
///
/// El anillo no puede quedar vacío: sacar la última devuelve `None`.
/// `layout` ya se defiende de eso, pero lo haría deshaciendo en silencio lo que
/// la persona acaba de hacer.
pub(crate) fn place(
    layout: &Layout,
    id: usize,
    to: Bucket,
    index: Option<usize>,
) -> Option<(Vec<usize>, Vec<usize>)> {
    if id >= COUNT {
        return None;
    }
    let mut ring: Vec<usize> = layout.ring.iter().copied().filter(|other| *other != id).collect();
    let mut more: Vec<usize> = layout.more.iter().copied().filter(|other| *other != id).collect();
    if to != Bucket::Ring && ring.is_empty() {
        return None;
    }
    let list = match to {
        Bucket::Ring => &mut ring,
        Bucket::More => &mut more,
        // Fuera de la pill no tiene orden propio: solo se saca de las dos.
        Bucket::Hidden => return Some((ring, more)),
    };
    let at = index.unwrap_or(list.len()).min(list.len());
    list.insert(at, id);
    Some((ring, more))
}

/// Fuera de la pill no tiene orden propio: cae donde lo pone el catálogo.
/// Solo se usa para dónde se inserta la ficha que se está arrastrando, así que
/// alcanza con contar las de la fila que van antes en el catálogo.
pub(crate) fn hidden_index(layout: &Layout, id: usize) -> usize {
    layout
        .hidden
        .iter()
        .filter(|other| **other != id && **other < id)
        .count()
}

/// Lo que puede ocupar una ficha de la tira: una herramienta, la puerta al
/// segundo nivel, la vuelta, o el editor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StripId {
    Tool(usize),
    More,
    Back,
    Customize,
}

impl StripId {
    /// La herramienta de atrás, si esta ficha es una.
    pub(crate) fn tool(self) -> Option<usize> {
        match self {
            StripId::Tool(tool) => Some(tool),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            StripId::Tool(tool) => name(tool),
            StripId::More => "Más",
            StripId::Back => "Atrás",
            StripId::Customize => "Personalizar",
        }
    }

    pub(crate) fn icon(self) -> &'static str {
        match self {
            StripId::Tool(tool) => icon(tool),
            StripId::More => "icons/ellipsis.svg",
            StripId::Back => "icons/chevron-up.svg",
            StripId::Customize => "icons/settings-2.svg",
        }
    }
}

/// Los dos pasos de la tira.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Page {
    Ring,
    More,
}

/// Qué muestra la tira, paso por paso.
///
/// Las herramientas siguen el escalón elegido en el editor. El primer paso
/// lleva la puerta a «Más» siempre: el editor vive en el segundo, así que la
/// puerta tiene que existir aunque no haya nada detrás.
pub(crate) fn strip_page(layout: &Layout, page: Page) -> Vec<StripId> {
    match page {
        Page::Ring => {
            let mut ids: Vec<StripId> = layout.ring.iter().map(|&tool| StripId::Tool(tool)).collect();
            ids.push(StripId::More);
            ids
        }
        Page::More => {
            // La vuelta va primera: la tira no tiene núcleo donde volver.
            let mut ids: Vec<StripId> = vec![StripId::Back];
            ids.extend(layout.more.iter().map(|&tool| StripId::Tool(tool)));
            ids.push(StripId::Customize);
            ids
        }
    }
}

/// La puerta al segundo nivel solo tiene sentido si hay algo detrás. Sin nada,
/// el editor sigue delante de «Más»: es donde se decide qué hay.
pub(crate) fn more_open(layout: &Layout) -> bool {
    !layout.more.is_empty()
}

fn prefs_file() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("atic-gpui").join("tools.json"))
}

/// Lo elegido la última vez, en `%LOCALAPPDATA%\atic-gpui\tools.json`.
pub(crate) fn load() -> Layout {
    let prefs = prefs_file()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Prefs>(&bytes).ok())
        .unwrap_or_default();
    offer_newcomers(&prefs)
}

/// Se guarda en el acto, cada soltar del editor: la tira queda como se la dejó.
pub(crate) fn save(layout: &Layout) {
    let Some(path) = prefs_file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let prefs = Prefs {
        ring: layout.ring.clone(),
        more: layout.more.clone(),
        // Lo que se guarda ya pasó por `load`: las nuevas ya se ofrecieron.
        offered: OFFER_IN_RING.to_vec(),
    };
    if let Ok(bytes) = serde_json::to_vec_pretty(&prefs) {
        let _ = std::fs::write(path, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Atajo: la disposición de unas listas, con lo que sobra ya escondido.
    fn made(ring: &[usize], more: &[usize]) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
        let layout = layout(ring, more);
        (layout.ring, layout.more, layout.hidden)
    }

    #[test]
    fn sin_configurar_muestra_el_catalogo_entero() {
        let (ring, more, hidden) = made(&[], &[]);
        assert_eq!(ring, (0..COUNT).collect::<Vec<_>>());
        assert!(more.is_empty());
        assert!(hidden.is_empty());
    }

    #[test]
    fn respeta_el_elegido_no_el_del_catalogo() {
        let (ring, _, _) = made(&[5, 0], &[]);
        // Sistema ya estaba en el anillo de la lista, así que no se duplica.
        assert_eq!(ring, vec![5, 0]);
    }

    #[test]
    fn lo_que_no_esta_en_ninguna_lista_queda_oculto() {
        let (ring, more, hidden) = made(&[0], &[7]);
        assert!(ring.contains(&0));
        assert_eq!(more, vec![7]);
        assert!(hidden.contains(&1));
        assert!(!hidden.contains(&0));
        assert!(!hidden.contains(&7));
    }

    #[test]
    fn una_herramienta_nueva_entra_al_anillo() {
        // Al leer una pill armada antes de que existiera.
        let prefs = Prefs { ring: vec![1, 3, 5, 6, 8], more: vec![0, 2], offered: Vec::new() };
        assert!(offer_newcomers(&prefs).ring.contains(&crate::SISTEMA_TOOL));
    }

    #[test]
    fn descarta_indices_que_no_son_herramientas() {
        let (ring, more, _) = made(&[0, 900], &[7]);
        assert_eq!(ring, vec![0]);
        assert_eq!(more, vec![7]);
    }

    #[test]
    fn no_repite_una_herramienta_ni_dentro_ni_entre_las_dos() {
        let (ring, more, _) = made(&[0, 0, 7], &[7, 1]);
        assert_eq!(ring, vec![0, 7]);
        assert_eq!(more, vec![1]);
    }

    #[test]
    fn con_el_anillo_vacio_el_submenu_sube() {
        let (ring, more, _) = made(&[], &[7, 1]);
        assert!(ring.contains(&7) && ring.contains(&1));
        assert!(more.is_empty());
    }

    #[test]
    fn una_config_sin_ningun_indice_valido_vuelve_al_catalogo() {
        let (ring, _, _) = made(&[900], &[901]);
        assert_eq!(ring, (0..COUNT).collect::<Vec<_>>());
    }

    #[test]
    fn el_primer_paso_repite_el_nivel_y_abre_el_segundo() {
        let layout = layout(&[0, 5], &[7]);
        assert_eq!(
            strip_page(&layout, Page::Ring),
            vec![
                StripId::Tool(0),
                StripId::Tool(5),
                StripId::More
            ]
        );
    }

    #[test]
    fn el_segundo_paso_arranca_con_la_vuelta() {
        let layout = layout(&[0], &[7, 1]);
        assert_eq!(
            strip_page(&layout, Page::More),
            vec![
                StripId::Back,
                StripId::Tool(7),
                StripId::Tool(1),
                StripId::Customize
            ]
        );
    }

    #[test]
    fn el_primer_paso_nunca_pasa_de_la_cantidad_maxima() {
        // El nivel a la vista entero más la puerta, aunque no haya nada atrás.
        let layout = layout(&[], &[]);
        assert_eq!(strip_page(&layout, Page::Ring).len(), STRIP_MAX);
        assert!(!more_open(&layout));
    }

    #[test]
    fn reordena_dentro_del_nivel() {
        let layout = layout(&[0, 1, crate::SISTEMA_TOOL], &[7]);
        let (ring, more) = place(&layout, crate::SISTEMA_TOOL, Bucket::Ring, Some(0)).unwrap();
        assert_eq!(ring, vec![crate::SISTEMA_TOOL, 0, 1]);
        assert_eq!(more, vec![7]);
    }

    #[test]
    fn pasa_del_nivel_al_segundo_en_la_posicion_pedida() {
        let layout = layout(&[0, 1, crate::SISTEMA_TOOL], &[7]);
        let (ring, more) = place(&layout, 1, Bucket::More, Some(0)).unwrap();
        assert_eq!(ring, vec![0, crate::SISTEMA_TOOL]);
        assert_eq!(more, vec![1, 7]);
    }

    #[test]
    fn sin_indice_va_al_final() {
        let layout = layout(&[0, 1], &[7]);
        let (ring, more) = place(&layout, 7, Bucket::Ring, None).unwrap();
        assert_eq!(ring, vec![0, 1, 7]);
        assert!(more.is_empty());
    }

    #[test]
    fn ocultar_la_saca_de_las_dos_listas() {
        let layout = layout(&[0, 1, crate::SISTEMA_TOOL], &[7]);
        let (ring, more) = place(&layout, 7, Bucket::Hidden, None).unwrap();
        assert_eq!(ring, vec![0, 1, crate::SISTEMA_TOOL]);
        assert!(more.is_empty());
    }

    #[test]
    fn el_indice_fuera_de_rango_se_acota() {
        let layout = layout(&[0, 1], &[]);
        let (ring, _) = place(&layout, 0, Bucket::Ring, Some(99)).unwrap();
        assert_eq!(ring, vec![1, 0]);
    }

    #[test]
    fn el_nivel_a_la_vista_no_puede_quedar_vacio() {
        let layout = layout(&[crate::SISTEMA_TOOL], &[7]);
        assert_eq!(place(&layout, crate::SISTEMA_TOOL, Bucket::More, None), None);
        assert_eq!(place(&layout, crate::SISTEMA_TOOL, Bucket::Hidden, None), None);
        assert!(place(&layout, crate::SISTEMA_TOOL, Bucket::Ring, Some(0)).is_some());
    }

    #[test]
    fn fuera_de_la_pill_cae_donde_lo_pone_el_catalogo() {
        let layout = layout(&[0], &[7]);
        // Ocultas: 1, 2, 3, 4, 5, 6 y 8 (el 0 es del anillo y el 7 de «Más»):
        // la 8 cae séptima, detrás de las seis que van antes en el catálogo.
        let (ring, more) = place(&layout, 8, Bucket::Hidden, None).unwrap();
        assert_eq!(ring, vec![0]);
        assert_eq!(more, vec![7]);
        assert_eq!(hidden_index(&layout, 8), 6);
    }

    #[test]
    fn una_herramienta_nueva_se_ofrece_una_sola_vez() {
        let sistema = crate::SISTEMA_TOOL;
        let ring: Vec<usize> = (0..COUNT).filter(|id| *id != sistema).take(3).collect();
        // Una pill ya armada, de antes de Sistema: entra al anillo.
        let before = Prefs { ring: ring.clone(), more: Vec::new(), offered: Vec::new() };
        assert!(offer_newcomers(&before).ring.contains(&sistema));
        // Ya ofrecida y sacada por el editor: no vuelve.
        let after = Prefs { ring: ring.clone(), more: Vec::new(), offered: vec![sistema] };
        assert!(offer_newcomers(&after).hidden.contains(&sistema));
        // Sacarla en el editor arma la disposición sin volver a ofrecerla.
        assert!(layout(&ring, &[]).hidden.contains(&sistema));
    }
}
