//! Cliente HTTP del hub: lee `hub.json` en cada llamada y pega con token.
//!
//! Se lee en cada `tools/call` y no al arrancar porque Atic puede cerrarse y
//! abrirse entre turno y turno; el `health` de 1 s cubre el pid muerto.

use std::time::Duration;

use directories::ProjectDirs;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::payload::HubError;

/// Copy exacto del §3.7: sin Atic no hay quién apruebe permisos.
pub const HUB_MISSING: &str = "Atic no está abierto. Ábrelo desde la bandeja para delegar: sin Atic no hay quién apruebe los permisos del otro agente.";

#[derive(Debug, Clone)]
pub struct Hub {
    base: String,
    token: String,
    cliente: reqwest::Client,
}

#[derive(Debug)]
pub enum HubFallo {
    /// Atic cerrado o inalcanzable: se contesta con el copy, no con un stack.
    Ausente,
    /// El hub contestó `{ "error": … }`: se propaga tal cual al modelo.
    Error(HubError),
    /// Red rota a mitad de llamada.
    Red(String),
}

impl HubFallo {
    pub fn mensaje(&self) -> String {
        match self {
            HubFallo::Ausente => HUB_MISSING.to_string(),
            HubFallo::Error(e) => {
                // El agente necesita el código y `data` para resolver casos
                // como `ambiguous_session` sin tener que adivinar qué pasó.
                // Se mantiene como texto porque MCP lo entrega en un bloque
                // de error, pero conserva la forma estructurada del contrato.
                let mut detalle = serde_json::json!({
                    "code": e.code,
                    "message": e.message,
                });
                if let Some(data) = &e.data {
                    detalle["data"] = data.clone();
                }
                format!("Atic respondió con un error: {detalle}")
            }
            HubFallo::Red(m) => format!("No se pudo hablar con Atic: {m}."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_del_hub_conserva_codigo_y_datos() {
        let fallo = HubFallo::Error(HubError {
            code: "ambiguous_session".into(),
            message: "Hay dos sesiones con ese nombre.".into(),
            data: Some(serde_json::json!({"sessions": ["s1", "s2"]})),
        });
        let texto = fallo.mensaje();
        assert!(texto.contains("ambiguous_session"));
        assert!(texto.contains("Hay dos sesiones con ese nombre."));
        assert!(texto.contains("s1"));
        assert!(texto.contains("s2"));
    }
}

/// Ruta de `hub.json`. En tests, `ATIC_HUB_JSON` apunta a uno de mentira.
fn hub_json_path() -> Option<std::path::PathBuf> {
    if let Some(var) = std::env::var_os("ATIC_HUB_JSON") {
        let p = std::path::PathBuf::from(var);
        if !p.as_os_str().is_empty() {
            return Some(p);
        }
    }
    ProjectDirs::from("com", "ciat", "atic").map(|d| d.data_dir().join("hub.json"))
}

/// Cliente HTTP compartido por todas las llamadas.
///
/// Uno por proceso y no uno por `tools/call`: `reqwest::Client` es un pool de
/// conexiones, y armarlo de nuevo en cada request tiraba el pool a la basura
/// (además de que cada `locate()` hacía su propio cliente para el health).
fn cliente() -> &'static reqwest::Client {
    static CLIENTE: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENTE.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// Localiza al hub y comprueba que respira (`GET /v1/health`, 1 s).
pub async fn locate() -> Result<Hub, HubFallo> {
    let ruta = hub_json_path().ok_or(HubFallo::Ausente)?;
    let texto = std::fs::read_to_string(&ruta).map_err(|_| HubFallo::Ausente)?;
    let json: serde_json::Value = serde_json::from_str(&texto).map_err(|_| HubFallo::Ausente)?;
    let port = json
        .get("port")
        .and_then(|p| p.as_u64())
        .ok_or(HubFallo::Ausente)?;
    let token = json
        .get("token")
        .and_then(|t| t.as_str())
        .ok_or(HubFallo::Ausente)?
        .to_string();
    let resp = cliente()
        .get(format!("http://127.0.0.1:{port}/v1/health"))
        .header("Authorization", format!("Bearer {token}"))
        .timeout(Duration::from_secs(1))
        .send()
        .await
        .map_err(|_| HubFallo::Ausente)?;
    if !resp.status().is_success() {
        return Err(HubFallo::Ausente);
    }
    Ok(Hub {
        base: format!("http://127.0.0.1:{port}"),
        token,
        cliente: cliente().clone(),
    })
}

impl Hub {
    async fn get<R: DeserializeOwned>(&self, ruta: &str) -> Result<R, HubFallo> {
        let resp = self
            .cliente
            .get(format!("{}{ruta}", self.base))
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| HubFallo::Red(e.to_string()))?;
        self.leer(resp).await
    }

    async fn post<B: Serialize, R: DeserializeOwned>(
        &self,
        ruta: &str,
        cuerpo: &B,
        espera_s: u64,
    ) -> Result<R, HubFallo> {
        let resp = self
            .cliente
            .post(format!("{}{ruta}", self.base))
            .header("Authorization", format!("Bearer {}", self.token))
            .timeout(Duration::from_secs(espera_s.saturating_add(15)))
            .json(cuerpo)
            .send()
            .await
            .map_err(|e| HubFallo::Red(e.to_string()))?;
        self.leer(resp).await
    }

    async fn leer<R: DeserializeOwned>(&self, resp: reqwest::Response) -> Result<R, HubFallo> {
        let codigo = resp.status();
        if codigo.is_success() {
            resp.json::<R>()
                .await
                .map_err(|e| HubFallo::Red(e.to_string()))
        } else {
            let envuelto: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
            match envuelto
                .get("error")
                .and_then(|e| serde_json::from_value::<HubError>(e.clone()).ok())
            {
                Some(e) => Err(HubFallo::Error(e)),
                None => Err(HubFallo::Red(format!("HTTP {codigo}"))),
            }
        }
    }

    pub async fn agents(&self) -> Result<crate::payload::AgentList, HubFallo> {
        self.get("/v1/agents").await
    }

    pub async fn sessions(
        &self,
        backend: Option<&str>,
    ) -> Result<crate::payload::SessionList, HubFallo> {
        let ruta = match backend {
            Some(b) => format!("/v1/sessions?backend={b}"),
            None => "/v1/sessions".to_string(),
        };
        self.get(&ruta).await
    }

    pub async fn spawn(
        &self,
        cuerpo: &crate::payload::SpawnRequest,
    ) -> Result<crate::payload::SessionOnly, HubFallo> {
        self.post("/v1/spawn", cuerpo, 30).await
    }

    pub async fn prompt(
        &self,
        cuerpo: &crate::payload::PromptRequest,
        espera_s: u64,
    ) -> Result<crate::payload::Outcome, HubFallo> {
        self.post("/v1/prompt", cuerpo, espera_s).await
    }

    pub async fn rename(
        &self,
        cuerpo: &crate::payload::RenameRequest,
    ) -> Result<serde_json::Value, HubFallo> {
        self.post("/v1/rename", cuerpo, 30).await
    }

    pub async fn delegate(
        &self,
        cuerpo: &crate::payload::DelegateRequest,
        espera_s: u64,
    ) -> Result<crate::payload::Outcome, HubFallo> {
        self.post("/v1/delegate", cuerpo, espera_s).await
    }

    pub async fn wait(
        &self,
        cuerpo: &crate::payload::WaitRequest,
        espera_s: u64,
    ) -> Result<crate::payload::Outcome, HubFallo> {
        self.post("/v1/wait", cuerpo, espera_s).await
    }

    pub async fn cancel(
        &self,
        cuerpo: &crate::payload::CancelRequest,
    ) -> Result<crate::payload::Cancelling, HubFallo> {
        self.post("/v1/cancel", cuerpo, 30).await
    }

    pub async fn permission(
        &self,
        cuerpo: &crate::payload::PermissionRequest,
    ) -> Result<serde_json::Value, HubFallo> {
        self.post("/v1/permission", cuerpo, 30).await
    }

    pub async fn close(
        &self,
        cuerpo: &crate::payload::CloseRequest,
    ) -> Result<crate::payload::Closed, HubFallo> {
        self.post("/v1/close", cuerpo, 30).await
    }
}
