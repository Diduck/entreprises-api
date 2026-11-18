//! Tests d'intégration de la vérification des access tokens émis par Keycloak (signés RS256 par sa clé privée,
//! vérifiés par l'API avec sa clé publique). Keycloak est remplacé par une paire de clés de test (`common`).
//! Aucune requête ne touche la base : le pool est paresseux et ne se connecte jamais.

mod common;

use axum::http::{StatusCode, header};
use axum_test::{TestResponse, TestServer};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use entreprises_api::{AppState, build_app};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Value, json};
use sqlx::MySqlPool;

use common::{CLE_PIRATE, CLE_PRIVEE, CLE_PUBLIQUE, KID};

const EPI_DOR: &str = "/api/v1/entreprises/036320596";

fn serveur() -> TestServer {
    let pool = MySqlPool::connect_lazy("mysql://personne@127.0.0.1:1/aucune").expect("URL valide");
    TestServer::new(build_app(AppState::new(pool, common::jwt())))
}

/// Route admin, appelée avec un SIREN invalide : un token accepté donne 400 (sans toucher la base),
/// un token refusé donne 401 ou 403.
async fn supprimer(token: &str) -> TestResponse {
    serveur().delete("/api/v1/entreprises/abc").authorization_bearer(token).expect_failure().await
}

fn probleme(reponse: &TestResponse, status: StatusCode) -> Value {
    reponse.assert_status(status);
    assert_eq!(reponse.content_type(), "application/problem+json");
    reponse.json()
}

fn token_refuse(reponse: &TestResponse) {
    let corps = probleme(reponse, StatusCode::UNAUTHORIZED);
    assert_eq!(corps["title"], "Token invalide ou expiré");
    assert_eq!(reponse.header(header::WWW_AUTHENTICATE), r#"Bearer error="invalid_token""#);
}

#[tokio::test]
async fn token_keycloak_admin_accepte() {
    probleme(&supprimer(&common::admin()).await, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn token_lecteur_403_sur_une_action_admin() {
    let corps = probleme(&supprimer(&common::lecteur()).await, StatusCode::FORBIDDEN);
    assert_eq!(corps["title"], "Action réservée aux administrateurs");
}

#[tokio::test]
async fn compte_sans_role_du_realm_403() {
    let token = common::token(&["default-roles-entreprises", "offline_access"]);
    let corps = probleme(&supprimer(&token).await, StatusCode::FORBIDDEN);
    assert_eq!(corps["title"], "Aucun droit sur cette API");
    let reponse = serveur().get("/api/v1/entreprises/abc").authorization_bearer(&token).expect_failure().await;
    probleme(&reponse, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn sans_token_401() {
    let reponse = serveur().get(EPI_DOR).expect_failure().await;
    let corps = probleme(&reponse, StatusCode::UNAUTHORIZED);
    assert_eq!(corps["title"], "Authentification requise");
    assert_eq!(reponse.header(header::WWW_AUTHENTICATE), "Bearer");
}

#[tokio::test]
async fn token_signe_par_une_autre_cle_privee_401() {
    // Même `kid`, mais signé par une clé privée que Keycloak ne possède pas.
    token_refuse(&supprimer(&common::signer(&common::claims(&["admin"]), CLE_PIRATE, KID)).await);
}

#[tokio::test]
async fn kid_inconnu_401() {
    token_refuse(&supprimer(&common::signer(&common::claims(&["admin"]), CLE_PRIVEE, "autre-cle")).await);
}

#[tokio::test]
async fn token_expire_401() {
    let mut claims = common::claims(&["admin"]);
    claims["exp"] = json!(claims["iat"].as_u64().unwrap() - 1);
    token_refuse(&supprimer(&common::signer(&claims, CLE_PRIVEE, KID)).await);
}

#[tokio::test]
async fn autre_emetteur_ou_autre_audience_401() {
    let mut autre_realm = common::claims(&["admin"]);
    autre_realm["iss"] = json!("http://keycloak.test/realms/autre");
    token_refuse(&supprimer(&common::signer(&autre_realm, CLE_PRIVEE, KID)).await);

    // Token d'un autre client du realm, qui n'a pas l'audience `entreprises-api`.
    let mut autre_audience = common::claims(&["admin"]);
    autre_audience["aud"] = json!("account");
    token_refuse(&supprimer(&common::signer(&autre_audience, CLE_PRIVEE, KID)).await);
}

#[tokio::test]
async fn role_ajoute_apres_signature_401() {
    let lecteur = common::lecteur();
    let parties: Vec<&str> = lecteur.split('.').collect();
    let contenu_admin = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&common::claims(&["admin"])).unwrap());
    // Contenu « admin » avec la signature du token lecteur : la signature ne correspond plus.
    token_refuse(&supprimer(&format!("{}.{contenu_admin}.{}", parties[0], parties[2])).await);
    token_refuse(&supprimer(&format!("{}.{}.", parties[0], parties[1])).await);
    token_refuse(&supprimer("pas-un-jwt").await);
}

#[tokio::test]
async fn algorithme_none_refuse() {
    let entete = URL_SAFE_NO_PAD.encode(format!(r#"{{"alg":"none","typ":"JWT","kid":"{KID}"}}"#));
    let contenu = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&common::claims(&["admin"])).unwrap());
    token_refuse(&supprimer(&format!("{entete}.{contenu}.")).await);
}

#[tokio::test]
async fn hs256_signe_avec_la_cle_publique_refuse() {
    // Attaque classique : la clé publique est connue de tous ; un serveur qui accepterait HS256
    // vérifierait ce token avec elle comme secret partagé.
    let mut entete = Header::new(Algorithm::HS256);
    entete.kid = Some(KID.to_owned());
    let token =
        jsonwebtoken::encode(&entete, &common::claims(&["admin"]), &EncodingKey::from_secret(CLE_PUBLIQUE)).unwrap();
    token_refuse(&supprimer(&token).await);
}
