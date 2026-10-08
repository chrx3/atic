//! Lo que Atic necesita de los agentes CLI sin depender de ninguna UI: los
//! hooks que se inyectan en sus consolas para enterarse de lo que esperan
//! (`hooks`) y cómo contestarles tecleando (`prompts`).
//!
//! Lo usan la app de Tauri (avisos en un archivo compartido) y la pill GPUI
//! (un archivo por consola, ver [`hooks::Sink`]): así nunca leen lo mismo ni
//! contestan dos veces.

pub mod hooks;
pub mod prompts;
