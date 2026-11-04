//! El sidecar contra un hub de mentira: lista y delega por `hub_client`.

mod common;

#[tokio::test]
async fn el_directorio_tiene_forma_y_la_delegacion_vuelve_con_sesion() {
    let (_port, hub_json, _pedidos) = common::arrancar_con_pedidos();
    std::env::set_var("ATIC_HUB_JSON", &hub_json);

    let hub = atic_mcp::hub_client::locate()
        .await
        .expect("el hub falso respira");
    let agentes = hub.agents().await.expect("lista");
    assert_eq!(agentes.agents.len(), 4);
    assert_eq!(agentes.agents[0].id, "claude-code");

    let recado = hub
        .delegate(
            &atic_mcp::payload::DelegateRequest {
                backend: "auto".into(),
                kind: None,
                cwd: None,
                text: "hola".into(),
                mode: Some("wait".into()),
                role: Some("general".into()),
                permission_mode: None,
                model: None,
                label: None,
                parent: None,
                depth: 0,
                root: None,
                host: None,
                wait_s: Some(5),
            },
            5,
        )
        .await
        .expect("delega");
    assert_eq!(recado.status, atic_mcp::payload::OutcomeStatus::Timeout);
    assert_eq!(recado.session, "s1", "el traspaso siempre trae sesión");
    assert!(
        recado.hint.as_deref().unwrap_or("").contains("Termina tu turno"),
        "el hint dice cómo seguir"
    );

    // Cerrar la sesión es aditivo: el hub falso responde la forma del contrato.
    let cerrada = hub
        .close(&atic_mcp::payload::CloseRequest {
            session: "s1".into(),
        })
        .await
        .expect("cierra");
    assert_eq!(cerrada.status, "closed");
    assert_eq!(cerrada.session, "s1");
}

#[test]
fn el_copy_de_hub_ausente_es_el_del_contrato() {
    assert_eq!(
        atic_mcp::hub_client::HUB_MISSING,
        "Atic Code no está abierto: ábrelo para delegar (sin Atic no hay quién apruebe los permisos del otro agente)."
    );
}

#[test]
fn delegate_lista_solo_el_hub_y_publica_modo_y_rol() {
    use atic_mcp::tools::{herramientas, ModoServidor};
    let lista = herramientas(ModoServidor::Delegate);
    assert!(lista.iter().all(|t| !t.name.starts_with("atic_editor_")));
    let delegado = lista.iter().find(|t| t.name == "atic_delegate").unwrap();
    let esquema = serde_json::to_value(&delegado.input_schema).unwrap();
    assert_eq!(esquema["required"], serde_json::json!(["text"]));
    assert_eq!(esquema["properties"]["mode"]["default"], "async");
    assert!(esquema["properties"].get("role").is_some());
    assert_eq!(esquema["additionalProperties"], false);
    for valor in ["async", "wait", "implementation", "research", "review", "design", "test", "general"] {
        assert!(esquema.to_string().contains(&format!("\"{valor}\"")), "falta {valor}");
    }

    let prompt = lista.iter().find(|t| t.name == "atic_prompt").unwrap();
    let esquema = serde_json::to_value(&prompt.input_schema).unwrap();
    assert_eq!(esquema["required"], serde_json::json!(["session", "text"]));
    assert_eq!(esquema["properties"]["mode"]["default"], "async");
    assert!(esquema["properties"].get("wait").is_none());
}

#[test]
fn rechaza_modos_y_roles_fuera_del_contrato() {
    use atic_mcp::tools::{Delegar, MandarMensaje};
    assert!(serde_json::from_value::<Delegar>(serde_json::json!({
        "text": "hola", "mode": "background"
    })).is_err());
    assert!(serde_json::from_value::<Delegar>(serde_json::json!({
        "text": "hola", "role": "inventado"
    })).is_err());
    assert!(serde_json::from_value::<MandarMensaje>(serde_json::json!({
        "session": "s1", "text": "hola", "mode": "background"
    })).is_err());
}
