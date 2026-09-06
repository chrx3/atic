//! Biblioteca del sidecar MCP. El binario `atic-mcp` es un `main` delgado;
//! los tests de integración pueden hablar con `hub_client` sin spawnear stdio.

pub mod budget;
pub mod hub_client;
pub mod payload;
pub mod tools;
