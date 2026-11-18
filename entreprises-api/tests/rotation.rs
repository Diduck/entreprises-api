//! Rotation des clés de Keycloak : scénario « la clé privée a fuité ».
//!
//! Un faux Keycloak (petit serveur HTTP local) publie un JWKS qu'on modifie en cours de test, comme un
//! administrateur qui remplace la clé dans la console Keycloak. Durée de confiance des clés réduite à 1 s
//! (5 min en vrai) pour ne pas attendre.

mod common;

use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use axum::{Router, extract::State, http::StatusCode, routing::get};
use axum_test::TestServer;
use entreprises_api::{
    AppState,
    auth::{Jwt, Refus},
    build_app,
};
use sqlx::MySqlPool;

use common::{AUDIENCE, CLE_PIRATE, CLE_PRIVEE, EMETTEUR, KID};

const JWKS_CLE_DE_TEST: &str = include_str!("fixtures/jwks-cle-de-test.json");
/// Nouvelle clé de Keycloak après remplacement. Ici, la clé « pirate » des autres tests joue ce rôle.
const JWKS_NOUVELLE_CLE: &str = include_str!("fixtures/jwks-nouvelle-cle.json");
const DUREE_MAX: Duration = Duration::from_secs(1);

/// JWKS publié par le faux Keycloak ; `None` : Keycloak arrêté (503).
type Publie = Arc<RwLock<Option<&'static str>>>;

/// Démarre un faux Keycloak qui sert le JWKS de `publie`. Renvoie l'adresse du JWKS.
async fn faux_keycloak(publie: Publie) -> String {
    async fn certs(State(publie): State<Publie>) -> Result<&'static str, StatusCode> {
        publie.read().unwrap().ok_or(StatusCode::SERVICE_UNAVAILABLE)
    }
    let ecoute = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let adresse = ecoute.local_addr().unwrap();
    let app = Router::new().route("/certs", get(certs)).with_state(publie);
    tokio::spawn(async move { axum::serve(ecoute, app).await.unwrap() });
    format!("http://{adresse}/certs")
}

async fn verificateur(publie: &Publie) -> Jwt {
    let jwt = Jwt::new(EMETTEUR, AUDIENCE, &faux_keycloak(publie.clone()).await).avec_duree_max(DUREE_MAX);
    jwt.charger_cles().await.expect("JWKS du faux Keycloak");
    jwt
}

fn ancien_token() -> String {
    common::signer(&common::claims(&["admin"]), CLE_PRIVEE, KID)
}

fn nouveau_token() -> String {
    common::signer(&common::claims(&["admin"]), CLE_PIRATE, "nouvelle-cle")
}

#[tokio::test]
async fn cle_retiree_refusee_apres_la_duree_max() {
    let publie: Publie = Arc::new(RwLock::new(Some(JWKS_CLE_DE_TEST)));
    let jwt = verificateur(&publie).await;
    assert!(jwt.verifier(&ancien_token()).await.is_ok());

    // L'administrateur remplace la clé dans Keycloak : l'ancienne disparaît du JWKS.
    *publie.write().unwrap() = Some(JWKS_NOUVELLE_CLE);
    // La nouvelle clé est acceptée dès le rechargement suivant (kid inconnu)…
    tokio::time::sleep(DUREE_MAX + Duration::from_millis(100)).await; // délai anti-rafale, borné par la durée max
    assert!(jwt.verifier(&nouveau_token()).await.is_ok());
    // …et l'ancienne, déjà retirée du jeu rechargé, est refusée.
    assert_eq!(jwt.verifier(&ancien_token()).await, Err(Refus::TokenInvalide));
}

#[tokio::test]
async fn cle_retiree_refusee_meme_sans_nouveau_kid() {
    // Personne n'envoie de token signé par la nouvelle clé : seule la durée max déclenche le rechargement.
    let publie: Publie = Arc::new(RwLock::new(Some(JWKS_CLE_DE_TEST)));
    let jwt = verificateur(&publie).await;
    assert!(jwt.verifier(&ancien_token()).await.is_ok());

    *publie.write().unwrap() = Some(JWKS_NOUVELLE_CLE);
    // Avant la durée max : clé encore en mémoire (c'est le délai maximal d'exposition).
    assert!(jwt.verifier(&ancien_token()).await.is_ok());
    tokio::time::sleep(DUREE_MAX + Duration::from_millis(100)).await;
    assert_eq!(jwt.verifier(&ancien_token()).await, Err(Refus::TokenInvalide));
}

#[tokio::test]
async fn keycloak_injoignable_apres_la_duree_max_503() {
    let publie: Publie = Arc::new(RwLock::new(Some(JWKS_CLE_DE_TEST)));
    let jwt = verificateur(&publie).await;
    *publie.write().unwrap() = None;
    // Clés encore récentes : le service continue.
    assert!(jwt.verifier(&ancien_token()).await.is_ok());
    tokio::time::sleep(DUREE_MAX + Duration::from_millis(100)).await;
    assert_eq!(jwt.verifier(&ancien_token()).await, Err(Refus::ClesIndisponibles));

    // Côté HTTP : 503 en problem+json, et non 401 (le token n'est pas en cause).
    let pool = MySqlPool::connect_lazy("mysql://personne@127.0.0.1:1/aucune").unwrap();
    let api = TestServer::new(build_app(AppState::new(pool, jwt)));
    let reponse = api.get("/api/v1/entreprises/abc").authorization_bearer(ancien_token()).expect_failure().await;
    reponse.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reponse.content_type(), "application/problem+json");

    // Keycloak revient : tout repart, sans redémarrer l'API (après le délai anti-rafale).
    *publie.write().unwrap() = Some(JWKS_CLE_DE_TEST);
    tokio::time::sleep(DUREE_MAX + Duration::from_millis(100)).await;
    let reponse = api.get("/api/v1/entreprises/abc").authorization_bearer(ancien_token()).expect_failure().await;
    reponse.assert_status(StatusCode::BAD_REQUEST);
}
