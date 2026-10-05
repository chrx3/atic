//! De dónde salen las reuniones: los datos reales de Atic (la tabla
//! `recordings` de `atic.db3` y `recordings/<id>/{transcript,summary}.json`) o,
//! con `MEETINGS_DEMO=1`, unas de prueba en memoria para ver la ventana llena.
//!

use std::path::PathBuf;

use atic_core::{AppDirs, Db, Recording, RecordingStatus, Segment, Speaker, Summary, Transcript};
use chrono::{Duration, Local, TimeZone, Utc};

/// Dónde están los datos: los de Atic o, con `MEETINGS_DATA_DIR=<carpeta>`,
/// una carpeta aparte con la misma estructura (`atic.db3`, `config.json`,
/// `recordings/<id>/`) para probar sin tocar los reales.
#[derive(Clone, Debug)]
pub struct Paths {
    root: PathBuf,
    db: PathBuf,
}

impl Paths {
    fn resolve() -> anyhow::Result<Self> {
        if let Some(root) = std::env::var_os("MEETINGS_DATA_DIR").map(PathBuf::from) {
            std::fs::create_dir_all(root.join("recordings"))?;
            return Ok(Self { db: root.join("atic.db3"), root });
        }
        let dirs = AppDirs::new()?;
        Ok(Self { root: dirs.data_dir(), db: dirs.db_path() })
    }

    /// Se abre en cada uso: Atic escribe la misma base y una conexión vieja
    /// no vería sus cambios hasta reabrir.
    pub fn db(&self) -> anyhow::Result<Db> {
        Ok(Db::open(&self.db)?)
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn recordings_dir(&self) -> PathBuf {
        self.root.join("recordings")
    }

    pub fn recording_dir(&self, id: &str) -> PathBuf {
        self.recordings_dir().join(id)
    }

    pub fn transcript_path(&self, id: &str) -> PathBuf {
        self.recording_dir(id).join("transcript.json")
    }

    pub fn summary_path(&self, id: &str) -> PathBuf {
        self.recording_dir(id).join("summary.json")
    }
}

pub enum Source {
    Atic(Paths),
    Demo,
}

impl Source {
    pub fn open() -> anyhow::Result<Self> {
        if std::env::var_os("MEETINGS_DEMO").is_some() {
            return Ok(Self::Demo);
        }
        Ok(Self::Atic(Paths::resolve()?))
    }

    /// Las rutas reales; `None` con los datos de prueba (nada se escribe).
    pub fn paths(&self) -> Option<&Paths> {
        match self {
            Self::Atic(paths) => Some(paths),
            Self::Demo => None,
        }
    }

    pub fn list(&self) -> anyhow::Result<Vec<Recording>> {
        match self {
            Self::Atic(paths) => Ok(paths.db()?.list_recordings()?),
            Self::Demo => Ok(demo_recordings()),
        }
    }

    pub fn transcript(&self, id: &str) -> anyhow::Result<Option<Transcript>> {
        match self {
            Self::Atic(paths) => Ok(Transcript::load(&paths.transcript_path(id))?),
            Self::Demo => Ok(demo_transcript(id)),
        }
    }

    pub fn summary(&self, id: &str) -> anyhow::Result<Option<Summary>> {
        match self {
            Self::Atic(paths) => Ok(Summary::load(&paths.summary_path(id))?),
            Self::Demo => Ok(demo_summary(id)),
        }
    }

    /// La carpeta de una grabación (para abrirla en el Explorador).
    pub fn folder(&self, id: &str) -> Option<PathBuf> {
        self.paths().map(|paths| paths.recording_dir(id))
    }
}

// --- Datos de prueba ---------------------------------------------------------------

fn at(days_ago: i64, hour: u32, minute: u32) -> chrono::DateTime<Utc> {
    let day = Local::now().date_naive() - Duration::days(days_ago);
    let local = day.and_hms_opt(hour, minute, 0).unwrap();
    Local.from_local_datetime(&local).single().unwrap_or_else(Local::now).with_timezone(&Utc)
}

fn rec(id: &str, title: &str, when: chrono::DateTime<Utc>, mins: i64, status: RecordingStatus) -> Recording {
    Recording {
        id: id.into(),
        title: title.into(),
        started_at: when,
        duration_secs: mins * 60 + 17,
        mic_path: Some("mic.wav".into()),
        system_path: Some("system.wav".into()),
        status,
    }
}

fn demo_recordings() -> Vec<Recording> {
    vec![
        rec("demo-1", "Planificación Q4 con el equipo de producto", at(0, 10, 30), 42, RecordingStatus::Summarized),
        rec("demo-2", "Llamada con proveedor de licencias", at(0, 9, 5), 18, RecordingStatus::Transcribed),
        rec("demo-3", "Grabación 2026-10-03 16:40", at(1, 16, 40), 7, RecordingStatus::Recorded),
        rec("demo-4", "Revisión de diseño del notch", at(3, 11, 0), 63, RecordingStatus::Summarized),
        rec("demo-5", "Entrevista técnica", at(24, 15, 15), 51, RecordingStatus::Error),
    ]
}

fn demo_transcript(id: &str) -> Option<Transcript> {
    let lines: &[(i64, Speaker, &str)] = match id {
        "demo-1" | "demo-4" => &[
            (0, Speaker::Me, "Partamos. La idea hoy es cerrar qué entra en el cuarto trimestre."),
            (6_000, Speaker::Me, "Tenemos tres frentes abiertos: la app de escritorio, el móvil y el sitio."),
            (14_500, Speaker::Others, "Desde producto, lo que más piden los clientes son los resúmenes automáticos de reuniones."),
            (22_000, Speaker::Others, "Y que se puedan mandar por correo sin copiar y pegar."),
            (29_000, Speaker::Others, "..."),
            (31_000, Speaker::Me, "Eso ya existe en la versión actual, pero la ventana es lenta. Estamos rehaciéndola en GPUI."),
            (44_000, Speaker::Others, "¿Cuánto tomaría tener la ventana nueva con lo mismo que hoy?"),
            (51_000, Speaker::Me, "La lectura, un par de días. Grabar y resumir dependen de mover el backend, eso es más largo."),
            (63_000, Speaker::Others, "Ok. Entonces propongo congelar features nuevas en la app vieja y solo corregir bugs."),
            (72_000, Speaker::Me, "De acuerdo. Lo dejo como decisión."),
            (78_000, Speaker::Others, "Otra cosa: el móvil necesita la isla para Android antes de diciembre."),
            (86_000, Speaker::Me, "Eso lo veo con el equipo móvil esta semana y te confirmo el viernes."),
            (95_000, Speaker::Others, "gracias gracias gracias gracias gracias gracias"),
            (99_000, Speaker::Others, "Perfecto, cerramos. Mando el acta en la tarde."),
        ],
        "demo-2" => &[
            (0, Speaker::Others, "Hola, ¿me escuchas bien?"),
            (3_000, Speaker::Me, "Sí, perfecto. Te llamaba por la renovación de las licencias."),
            (9_000, Speaker::Others, "Claro. El precio sube un ocho por ciento a partir de enero."),
            (16_000, Speaker::Me, "¿Hay descuento si pagamos el año completo por adelantado?"),
            (21_000, Speaker::Others, "Sí, un doce por ciento sobre el total anual."),
        ],
        "demo-5" => &[(0, Speaker::Me, "Cuéntame de un proyecto del que estés orgulloso.")],
        _ => return None,
    };
    Some(Transcript {
        language: Some("es".into()),
        segments: lines
            .iter()
            .map(|&(start_ms, speaker, text)| Segment {
                start_ms,
                end_ms: start_ms + 5_000,
                speaker,
                speaker_name: (id == "demo-4" && speaker == Speaker::Others).then(|| "Camila".into()),
                text: text.into(),
            })
            .collect(),
    })
}

fn demo_summary(id: &str) -> Option<Summary> {
    let body = match id {
        "demo-1" => "## Resumen\nSe revisaron los tres frentes del trimestre (escritorio, móvil y sitio). Producto pidió priorizar los **resúmenes automáticos** y el envío por correo; ambos existen pero la ventana actual es lenta, por lo que se está rehaciendo en GPUI.\n\n## Temas tratados\n- Alcance del cuarto trimestre\n- Rendimiento de la ventana de reuniones\n- Isla de Android para el móvil\n\n## Decisiones\n- Congelar funciones nuevas en la app WebView; solo corrección de errores.\n- La ventana de reuniones nueva parte por la lectura (lista, transcripción, resumen).\n\n## Próximos pasos\n- [ ] Confirmar fecha de la isla de Android con el equipo móvil (viernes)\n- [ ] Enviar el acta de la reunión\n- [x] Compartir la lista de lo que usa hoy la app principal",
        "demo-4" => "**Resumen**\nRevisión del notch con Camila. Se validó el modelo de una sola forma que crece y se descartaron las tarjetas flotantes.\n\n**Acuerdos**\n1. Colores planos, sin bordes.\n2. Esquinas bien redondeadas y anidadas.\n\n**Tareas**\n- Ajustar la transparencia del panel con contenido",
        _ => return None,
    };
    Some(Summary {
        template: "executive_minutes".into(),
        title: "Acta".into(),
        body: body.into(),
        subject: None,
        backend: "claude".into(),
        created_at: Utc::now(),
    })
}
