//! El sidecar contra un hub de mentira: lista y delega por `hub_client`.

mod common;

#[tokio::test]
async fn el_directorio_tiene_forma_y_la_delegacion_vuelve_con_sesion() {
    let (_port, hub_json) = common::arrancar();
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
        recado.hint.as_deref().unwrap_or("").contains("atic_wait"),
        "el hint dice cómo seguir"
    );
}

#[test]
fn el_copy_de_hub_ausente_es_el_del_contrato() {
    assert_eq!(
        atic_mcp::hub_client::HUB_MISSING,
        "Atic no está abierto. Ábrelo desde la bandeja para delegar: sin Atic no hay quién apruebe los permisos del otro agente."
    );
}
