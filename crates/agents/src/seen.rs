/// Una sesión de agente vista por fuera (base de datos, proceso), sin el
/// modelo de presencia de ninguna app: cada una la convierte al suyo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub id: String,
    pub cwd: String,
    pub status: SeenStatus,
    /// La primera línea de la última respuesta, si terminó.
    pub preview: Option<String>,
    /// Última actividad, en segundos desde 1970.
    pub updated: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeenStatus {
    Working,
    Ready,
    Idle,
}
