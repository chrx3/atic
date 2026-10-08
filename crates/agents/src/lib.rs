//! Lo que Atic necesita de los agentes CLI sin depender de ninguna UI: los
//! hooks que se inyectan en sus consolas para enterarse de lo que esperan
//! (`hooks`), cómo contestarles tecleando (`prompts`) y las sesiones de los
//! que no tienen hooks: OpenCode (`opencode`) y Cursor (`cursor`).
//!
//! Lo usan la app de Tauri (avisos en un archivo compartido) y la pill GPUI
//! (un archivo por consola, ver [`hooks::Sink`]): así nunca leen lo mismo ni
//! contestan dos veces.

pub mod cursor;
pub mod exe;
pub mod hooks;
pub mod mcp_install;
pub mod opencode;
pub mod prompts;
pub mod seen;
