//! Tests de la documentation OpenAPI / Swagger UI. Aucune requête ne touche la base : le pool est paresseux
//! et ne se connecte jamais, ces tests tournent donc sans MariaDB.

use std::time::Duration;

use axum::http::{StatusCode, header};
use axum_test::TestServer;
use entreprises_api::{AppState, auth::Jwt, build_app};
use serde_json::{Value, json};
use sqlx::MySqlPool;

fn serveur() -> TestServer {
    let pool = MySqlPool::connect_lazy("mysql://personne@127.0.0.1:1/aucune").expect("URL valide");
    let jwt = Jwt::new("secret-jwt-des-tests-de-la-documentation", Duration::from_secs(900)).unwrap();
    TestServer::new(build_app(AppState::new(pool, jwt)))
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
            "get /api/v1/utilisateurs/{id}",
            "patch /api/v1/entreprises/{siren}",
            "post /api/v1/tokens",
            "post /api/v1/utilisateurs",
        ]
    );
}

#[tokio::test]
async fn seule_la_connexion_est_publique() {
    let spec: Value = serveur().get("/api-docs/openapi.json").await.json();
    assert_eq!(spec["components"]["securitySchemes"]["jwt"]["scheme"], "bearer");
    for (chemin, methodes) in spec["paths"].as_object().unwrap() {
        for (methode, operation) in methodes.as_object().unwrap() {
            // `[{}]` : aucune authentification ; `[{"jwt": []}]` : Bearer exigé.
            let attendu = if chemin == "/api/v1/tokens" { json!([{}]) } else { json!([{ "jwt": [] }]) };
            assert_eq!(operation["security"], attendu, "{methode} {chemin}");
        }
    }
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
