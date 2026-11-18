//! Tests de la documentation OpenAPI / Swagger UI. Aucune requête ne touche la base : le pool est paresseux
//! et ne se connecte jamais, ces tests tournent donc sans MariaDB.

mod common;

use axum::http::{StatusCode, header};
use axum_test::TestServer;
use entreprises_api::{AppState, build_app};
use serde_json::{Value, json};
use sqlx::MySqlPool;

fn serveur() -> TestServer {
    let pool = MySqlPool::connect_lazy("mysql://personne@127.0.0.1:1/aucune").expect("URL valide");
    TestServer::new(build_app(AppState::new(pool, common::jwt())))
}

#[tokio::test]
async fn swagger_ui_servie_sur_docs() {
    let reponse = serveur().get("/docs/").await;
    reponse.assert_status_ok();
    assert!(reponse.header(header::CONTENT_TYPE).to_str().unwrap().starts_with("text/html"));
    assert!(reponse.text().contains("swagger-ui"));
}

#[tokio::test]
async fn specification_couvre_toutes_les_routes() {
    let spec: Value = serveur().get("/api-docs/openapi.json").await.json();
    let mut operations: Vec<String> = spec["paths"]
        .as_object()
        .unwrap()
        .iter()
        .flat_map(|(chemin, methodes)| methodes.as_object().unwrap().keys().map(move |m| format!("{m} {chemin}")))
        .collect();
    operations.sort();
    assert_eq!(
        operations,
        [
            "delete /api/v1/entreprises/{siren}",
            "get /api/v1/entreprises",
            "get /api/v1/entreprises/{siren}",
            "patch /api/v1/entreprises/{siren}",
        ]
    );
}

#[tokio::test]
async fn toutes_les_routes_exigent_keycloak() {
    let spec: Value = serveur().get("/api-docs/openapi.json").await.json();
    let flux = &spec["components"]["securitySchemes"]["keycloak"]["flows"]["authorizationCode"];
    assert_eq!(flux["authorizationUrl"], format!("{}/protocol/openid-connect/auth", common::EMETTEUR));
    assert_eq!(flux["tokenUrl"], format!("{}/protocol/openid-connect/token", common::EMETTEUR));
    for (chemin, methodes) in spec["paths"].as_object().unwrap() {
        for (methode, operation) in methodes.as_object().unwrap() {
            assert_eq!(operation["security"], json!([{ "keycloak": [] }]), "{methode} {chemin}");
        }
    }
}

#[tokio::test]
async fn swagger_ui_configure_pour_le_client_keycloak_avec_pkce() {
    let initialisation = serveur().get("/docs/swagger-initializer.js").await.text();
    assert!(initialisation.contains(r#""clientId": "swagger""#) || initialisation.contains(r#""clientId":"swagger""#));
    assert!(initialisation.contains("usePkceWithAuthorizationCodeGrant"));
}

#[tokio::test]
async fn erreurs_documentees_en_problem_json() {
    let spec: Value = serveur().get("/api-docs/openapi.json").await.json();
    let patch = &spec["paths"]["/api/v1/entreprises/{siren}"]["patch"]["responses"];
    for code in ["400", "401", "403", "404", "415", "422"] {
        assert!(patch[code]["content"]["application/problem+json"].is_object(), "PATCH {code}");
    }
}

#[tokio::test]
async fn routes_de_l_api_toujours_protegees() {
    serveur().get("/api/v1/entreprises/036320596").expect_failure().await.assert_status(StatusCode::UNAUTHORIZED);
}
