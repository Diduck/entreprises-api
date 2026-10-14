//! Tests d'intégration de l'authentification : connexion (`POST /api/v1/tokens`), JWT, comptes utilisateurs.
//! Chaque test part d'une base MariaDB neuve et migrée (`#[sqlx::test]`) avec deux comptes réels.

use std::time::Duration;

use axum::{
    body::Bytes,
    http::{StatusCode, header},
};
use axum_test::{TestResponse, TestServer};
use entreprises_api::{
    AppState,
    auth::{Identite, Jwt, Role},
    build_app, utilisateurs,
};
use serde_json::{Value, json};
use sqlx::MySqlPool;

const SECRET: &str = "secret-jwt-des-tests-d-authentification";
const ADMIN_EMAIL: &str = "admin@exemple.fr";
const ADMIN_MDP: &str = "mot-de-passe-admin";
const LECTEUR_EMAIL: &str = "lecteur@exemple.fr";
const LECTEUR_MDP: &str = "mot-de-passe-lecteur";
const EPI_DOR: &str = "/api/v1/entreprises/036320596";

fn jwt() -> Jwt {
    Jwt::new(SECRET, Duration::from_secs(900)).expect("secret de test")
}

async fn creer_compte(pool: &MySqlPool, email: &str, mot_de_passe: &str, role: &str) -> i64 {
    let nouveau = utilisateurs::NouvelUtilisateur::valider(email, mot_de_passe.to_owned(), role, Vec::new()).unwrap();
    let empreinte = utilisateurs::hacher(mot_de_passe);
    utilisateurs::creer(pool, &nouveau, &empreinte).await.unwrap().expect("compte créé").id
}

async fn serveur(pool: &MySqlPool) -> TestServer {
    creer_compte(pool, ADMIN_EMAIL, ADMIN_MDP, "admin").await;
    creer_compte(pool, LECTEUR_EMAIL, LECTEUR_MDP, "lecteur").await;
    TestServer::new(build_app(AppState::new(pool.clone(), jwt())))
}

async fn connexion(api: &TestServer, email: &str, mot_de_passe: &str) -> TestResponse {
    api.post("/api/v1/tokens").json(&json!({ "email": email, "mot_de_passe": mot_de_passe })).await
}

/// Se connecte et renvoie le JWT.
async fn token(api: &TestServer, email: &str, mot_de_passe: &str) -> String {
    let reponse = connexion(api, email, mot_de_passe).await;
    reponse.assert_status_ok();
    reponse.json::<Value>()["access_token"].as_str().unwrap().to_owned()
}

fn probleme(reponse: &TestResponse, status: StatusCode) -> Value {
    reponse.assert_status(status);
    assert_eq!(reponse.content_type(), "application/problem+json");
    reponse.json()
}

// ---------------------------------------------------------------------------
// Connexion
// ---------------------------------------------------------------------------

#[sqlx::test(fixtures("entreprises"))]
async fn connexion_renvoie_un_jwt_utilisable(pool: MySqlPool) {
    entreprises_api::import::recopier(&pool).await.unwrap();
    let api = serveur(&pool).await;
    let reponse = connexion(&api, LECTEUR_EMAIL, LECTEUR_MDP).await;
    reponse.assert_status_ok();
    assert_eq!(reponse.header(header::CACHE_CONTROL), "no-store");
    let corps: Value = reponse.json();
    assert_eq!(corps["token_type"], "Bearer");
    assert_eq!(corps["expires_in"], 900);
    let token = corps["access_token"].as_str().unwrap();
    assert_eq!(token.split('.').count(), 3, "un JWT a trois parties");

    api.get(EPI_DOR).authorization_bearer(token).await.assert_status_ok();
    // Rôle lecteur inscrit dans le token : pas d'écriture.
    probleme(&api.delete(EPI_DOR).authorization_bearer(token).await, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn connexion_email_insensible_a_la_casse(pool: MySqlPool) {
    let api = serveur(&pool).await;
    connexion(&api, "  ADMIN@Exemple.FR ", ADMIN_MDP).await.assert_status_ok();
}

#[sqlx::test]
async fn mauvais_mot_de_passe_ou_email_inconnu_meme_reponse_401(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let mauvais_mdp = connexion(&api, ADMIN_EMAIL, "mauvais-mot-de-passe").await;
    let inconnu = connexion(&api, "personne@exemple.fr", ADMIN_MDP).await;
    for reponse in [&mauvais_mdp, &inconnu] {
        let corps = probleme(reponse, StatusCode::UNAUTHORIZED);
        assert_eq!(corps["title"], "Identifiants invalides");
        assert_eq!(reponse.header(header::WWW_AUTHENTICATE), "Bearer");
    }
    // On ne peut pas savoir si c'est l'email ou le mot de passe qui est faux.
    let sans_instance = |r: &TestResponse| {
        let mut corps: Value = r.json();
        corps.as_object_mut().unwrap().remove("instance");
        corps
    };
    assert_eq!(sans_instance(&mauvais_mdp), sans_instance(&inconnu));
}

#[sqlx::test]
async fn connexion_corps_invalide(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let corps = probleme(
        &api.post("/api/v1/tokens").json(&json!({ "email": ADMIN_EMAIL })).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    );
    assert_eq!(corps["errors"][0]["champ"], "mot_de_passe");
    probleme(&api.post("/api/v1/tokens").json(&json!(["x"])).await, StatusCode::BAD_REQUEST);
    let reponse = api.post("/api/v1/tokens").bytes(Bytes::from_static(br#"{"email":"a"}"#)).await;
    probleme(&reponse, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let reponse = api.get("/api/v1/tokens").await;
    probleme(&reponse, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(reponse.header(header::ALLOW), "POST");
}

// ---------------------------------------------------------------------------
// Vérification du JWT
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn jwt_signe_par_une_autre_cle_401_invalid_token(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let pirate = Jwt::new("une-cle-que-le-serveur-ne-connait-pas", Duration::from_secs(900)).unwrap();
    let token = pirate.emettre(Identite { id: 1, role: Role::Admin });
    let reponse = api.get(EPI_DOR).authorization_bearer(&token).await;
    let corps = probleme(&reponse, StatusCode::UNAUTHORIZED);
    assert_eq!(corps["title"], "Token invalide ou expiré");
    assert_eq!(reponse.header(header::WWW_AUTHENTICATE), r#"Bearer error="invalid_token""#);
}

#[sqlx::test]
async fn jwt_expire_401(pool: MySqlPool) {
    creer_compte(&pool, ADMIN_EMAIL, ADMIN_MDP, "admin").await;
    let courte = Jwt::new(SECRET, Duration::ZERO).unwrap();
    let api = TestServer::new(build_app(AppState::new(pool.clone(), courte)));
    let token = token(&api, ADMIN_EMAIL, ADMIN_MDP).await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    probleme(&api.get(EPI_DOR).authorization_bearer(&token).await, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// Comptes
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn admin_cree_un_compte_qui_peut_se_connecter(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let admin = token(&api, ADMIN_EMAIL, ADMIN_MDP).await;
    let reponse = api
        .post("/api/v1/utilisateurs")
        .authorization_bearer(&admin)
        .json(&json!({ "email": "Nouveau@Exemple.fr", "mot_de_passe": "un-mot-de-passe-solide", "role": "lecteur" }))
        .await;
    reponse.assert_status(StatusCode::CREATED);
    let corps: Value = reponse.json();
    let location = reponse.header(header::LOCATION).to_str().unwrap().to_owned();
    assert_eq!(corps["email"], "nouveau@exemple.fr");
    assert_eq!(corps["role"], "lecteur");
    assert_eq!(corps["links"]["self"], location);
    assert!(corps.get("mot_de_passe").is_none(), "le mot de passe n'est jamais renvoyé");
    assert!(!reponse.text().contains("argon2"), "l'empreinte n'est jamais renvoyée");

    let lu: Value = api.get(&location).authorization_bearer(&admin).await.json();
    assert_eq!(lu, corps);
    token(&api, "nouveau@exemple.fr", "un-mot-de-passe-solide").await;
}

#[sqlx::test]
async fn email_deja_utilise_409(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let admin = token(&api, ADMIN_EMAIL, ADMIN_MDP).await;
    let reponse = api
        .post("/api/v1/utilisateurs")
        .authorization_bearer(&admin)
        .json(&json!({ "email": "LECTEUR@exemple.fr", "mot_de_passe": "un-mot-de-passe-solide", "role": "admin" }))
        .await;
    assert_eq!(probleme(&reponse, StatusCode::CONFLICT)["title"], "Email déjà utilisé");
}

#[sqlx::test]
async fn creation_reservee_aux_admins(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let lecteur = token(&api, LECTEUR_EMAIL, LECTEUR_MDP).await;
    let corps = json!({ "email": "x@exemple.fr", "mot_de_passe": "un-mot-de-passe-solide", "role": "admin" });
    probleme(&api.post("/api/v1/utilisateurs").json(&corps).await, StatusCode::UNAUTHORIZED);
    probleme(
        &api.post("/api/v1/utilisateurs").authorization_bearer(&lecteur).json(&corps).await,
        StatusCode::FORBIDDEN,
    );
    probleme(&api.get("/api/v1/utilisateurs/1").authorization_bearer(&lecteur).await, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn creation_liste_tous_les_champs_refuses(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let admin = token(&api, ADMIN_EMAIL, ADMIN_MDP).await;
    let reponse = api
        .post("/api/v1/utilisateurs")
        .authorization_bearer(&admin)
        .json(&json!({ "email": "pas-un-email", "mot_de_passe": "court", "role": "superadmin" }))
        .await;
    let corps = probleme(&reponse, StatusCode::UNPROCESSABLE_ENTITY);
    let champs: Vec<&str> = corps["errors"].as_array().unwrap().iter().map(|e| e["champ"].as_str().unwrap()).collect();
    assert_eq!(champs, ["email", "mot_de_passe", "role"]);
}

#[sqlx::test]
async fn lecture_d_un_compte(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let admin = token(&api, ADMIN_EMAIL, ADMIN_MDP).await;
    probleme(&api.get("/api/v1/utilisateurs/999").authorization_bearer(&admin).await, StatusCode::NOT_FOUND);
    probleme(&api.get("/api/v1/utilisateurs/abc").authorization_bearer(&admin).await, StatusCode::BAD_REQUEST);
    probleme(&api.get("/api/v1/utilisateurs/0").authorization_bearer(&admin).await, StatusCode::BAD_REQUEST);
}
