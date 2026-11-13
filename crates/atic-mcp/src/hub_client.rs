//! Cliente HTTP del hub: lee `hub.json` en cada llamada y pega con token.
//!
//! Se lee en cada `tools/call` y no al arrancar porque Atic puede cerrarse y
//! abrirse entre turno y turno; el `health` de 1 s cubre el pid muerto.
//!
//! Hay dos tokens. El de `hub.json` lo lee cualquiera: con él se pide como
//! alguien de fuera de Atic Code. Una conversación de Atic Code trae además el
//! suyo en `ATIC_HUB_TOKEN`: con ese, el hub sabe qué conversación pide (quién
//! es el padre, a qué profundidad está) sin fiarse de lo que diga el pedido.

use std::time::Duration;

use directories::ProjectDirs;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::payload::HubError;

/// Sin Atic Code no hay quién apruebe permisos.
pub const HUB_MISSING: &str = "Atic Code no está abierto: ábrelo para delegar (sin Atic no hay quién apruebe los permisos del otro agente).";

#[derive(Debug, Clone)]
pub struct Hub {
    base: String,
    token: String,
    /// El token de `hub.json`, cuando `token` es el de la conversación: el
    /// respaldo si el hub ya no conoce ese (Atic se reinició).
    compartido: Option<String>,
    cliente: reqwest::Client,
}

/// Con qué token se pide y cuál queda de respaldo: el de la conversación
/// (`ATIC_HUB_TOKEN`) primero, si viene; si no, el de `hub.json` a secas.
fn elegir_tokens(de_sesion: Option<String>, de_hub_json: String) -> (String, Option<String>) {
    match de_sesion.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        Some(sesion) if sesion != de_hub_json => (sesion, Some(de_hub_json)),
        _ => (de_hub_json, None),
    }
}

/// Lo que el hub retiene un turno pedido con el token de `hub.json` (de fuera de una
/// conversación): espera al hijo hasta su `waitS` por defecto y le suma su margen
/// (`WAIT_DEFAULT` + `WAIT_MARGIN` en `apps/atic/src/code/hub.rs`). Sin una
/// conversación a la que despertar, el resultado tiene que volver en la respuesta.
const RETENCION_EXTERNA_S: u64 = 600 + 30;
/// Lo que se espera de más a que llegue la respuesta, sobre lo que el hub retiene.
const MARGEN_S: u64 = 15;
/// Un turno que no espera, pedido por una conversación: el hub contesta al lanzarlo.
const LANZAR_S: u64 = 30;

/// Cuánto esperar la respuesta de `delegate` o `prompt`. Con `espera_s` (modo
/// `wait`), lo pedido; sin él, un pedido de la conversación (`externo` falso) solo
/// lanza el turno, pero uno de fuera queda esperando al hijo en el hub: cortarlo a
/// los 30 s dejaba al subagente creado y al modelo con «error sending request».
fn limite_turno(espera_s: Option<u64>, externo: bool) -> u64 {
    match espera_s {
        Some(espera) => espera.saturating_add(MARGEN_S),
        None if externo => RETENCION_EXTERNA_S + MARGEN_S,
        None => LANZAR_S,
    }
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
    let (token, compartido) = elegir_tokens(std::env::var("ATIC_HUB_TOKEN").ok(), token);
    Ok(Hub {
        base: format!("http://127.0.0.1:{port}"),
        token,
        compartido,
        cliente: cliente().clone(),
    })
}

impl Hub {
    /// Manda el pedido que arma `armar` con el token de la conversación. Si el
    /// hub ya no lo conoce (Atic se reinició y la conversación siguió viva),
    /// lo repite con el de `hub.json`: se pide como alguien de fuera, que es
    /// menos (sin despertar al padre) pero no deja a la conversación sin delegar.
    ///
    /// `armar` recibe el token y si con él se pide como alguien de fuera (el de
    /// `hub.json`): el hub trata distinto esos pedidos y los plazos tienen que calzar.
    async fn enviar(
        &self,
        armar: impl Fn(&str, bool) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, HubFallo> {
        let resp = armar(&self.token, self.compartido.is_none())
            .send()
            .await
            .map_err(|e| HubFallo::Red(e.to_string()))?;
        match &self.compartido {
            Some(compartido) if resp.status() == reqwest::StatusCode::UNAUTHORIZED => armar(compartido, true)
                .send()
                .await
                .map_err(|e| HubFallo::Red(e.to_string())),
            _ => Ok(resp),
        }
    }

    async fn get<R: DeserializeOwned>(&self, ruta: &str) -> Result<R, HubFallo> {
        let resp = self
            .enviar(|token, _| {
                self.cliente
                    .get(format!("{}{ruta}", self.base))
                    .header("Authorization", format!("Bearer {token}"))
            })
            .await?;
        self.leer(resp).await
    }

    async fn post<B: Serialize, R: DeserializeOwned>(
        &self,
        ruta: &str,
        cuerpo: &B,
        espera_s: u64,
    ) -> Result<R, HubFallo> {
        self.post_con_timeout(ruta, cuerpo, espera_s.saturating_add(15)).await
    }

    async fn post_con_timeout<B: Serialize, R: DeserializeOwned>(
        &self,
        ruta: &str,
        cuerpo: &B,
        timeout_s: u64,
    ) -> Result<R, HubFallo> {
        self.post_segun(ruta, cuerpo, |_| timeout_s).await
    }

    /// Un turno (`delegate`, `prompt`): el plazo depende de con qué token salga.
    async fn post_turno<B: Serialize, R: DeserializeOwned>(
        &self,
        ruta: &str,
        cuerpo: &B,
        espera_s: Option<u64>,
    ) -> Result<R, HubFallo> {
        self.post_segun(ruta, cuerpo, |externo| limite_turno(espera_s, externo)).await
    }

    async fn post_segun<B: Serialize, R: DeserializeOwned>(
        &self,
        ruta: &str,
        cuerpo: &B,
        timeout_s: impl Fn(bool) -> u64,
    ) -> Result<R, HubFallo> {
        let resp = self
            .enviar(|token, externo| {
                self.cliente
                    .post(format!("{}{ruta}", self.base))
                    .header("Authorization", format!("Bearer {token}"))
                    .timeout(Duration::from_secs(timeout_s(externo)))
                    .json(cuerpo)
            })
            .await?;
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

    pub async fn models(&self, backend: &str) -> Result<serde_json::Value, HubFallo> {
        self.get(&format!("/v1/models?backend={backend}")).await
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
        let espera = (cuerpo.mode.as_deref() == Some("wait")).then_some(espera_s);
        self.post_turno("/v1/prompt", cuerpo, espera).await
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
        let espera = (cuerpo.mode.as_deref() == Some("wait")).then_some(espera_s);
        self.post_turno("/v1/delegate", cuerpo, espera).await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_plazo_de_un_turno_calza_con_lo_que_el_hub_lo_retiene() {
        // La conversación lanza y sigue: el hub contesta al tiro.
        assert_eq!(limite_turno(None, false), LANZAR_S);
        // De fuera (sin token propio, o tras un 401), el hub espera al hijo: el
        // cliente no puede cortar antes, o el subagente queda creado y el modelo
        // recibe «error sending request».
        assert!(limite_turno(None, true) > RETENCION_EXTERNA_S);
        // Con `wait`, lo pedido más el margen, sea quien sea.
        assert_eq!(limite_turno(Some(120), false), 135);
        assert_eq!(limite_turno(Some(120), true), 135);
    }

    #[test]
    fn el_token_de_la_conversacion_va_primero_y_el_de_hub_json_de_respaldo() {
        assert_eq!(
            elegir_tokens(Some("de-sesion".into()), "compartido".into()),
            ("de-sesion".to_string(), Some("compartido".to_string()))
        );
        // Fuera de Atic Code no hay token de conversación: va el de `hub.json`, sin respaldo.
        assert_eq!(elegir_tokens(None, "compartido".into()), ("compartido".to_string(), None));
        assert_eq!(elegir_tokens(Some("  ".into()), "compartido".into()), ("compartido".to_string(), None));
        assert_eq!(elegir_tokens(Some("compartido".into()), "compartido".into()), ("compartido".to_string(), None));
    }

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
