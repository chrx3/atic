//! `atic-mcp`: el servidor MCP de Atic (stdio).
//!
//! Binario chico, sin ventana: lo carga el host (Cursor, `codex`, `claude`,
//! OpenCode) y traduce `tools/call` a pedidos del hub por HTTP en localhost.
//! Nada, nunca, a stdout fuera de `rmcp`: los logs van a stderr o se rompe
//! el transporte.

use atic_mcp::tools;
use rmcp::{transport::stdio, ServiceExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // A mano, sin `clap`: son dos flags.
    let mut host: Option<String> = None;
    let mut wait: Option<u64> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--host" => host = args.next().map(|v| v.trim().to_string()),
            "--wait" => {
                wait = args.next().and_then(|v| v.parse().ok());
            }
            _ => {}
        }
    }
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();
    let (servidor, _) = tools::router(host, wait);
    let servicio = servidor.serve(stdio()).await?;
    servicio.waiting().await?;
    Ok(())
}
