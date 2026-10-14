//! Tests d'intégration E1 à E5 : chaque test part d'une base MariaDB neuve (`#[sqlx::test]`),
//! migrée, remplie avec 16 entreprises réelles (`tests/fixtures/entreprises.sql`) puis importée.
//!
//! Dans l'extrait : `10.71C` → 5 entreprises (3 actives), `15.8C` → 2,
//! « boulangerie » → 4 (2 actives), `000325175` → l'entrepreneur JANOYER Thierry.

use axum::{
    body::Bytes,
    http::{StatusCode, header},
};
use axum_test::{TestResponse, TestServer};
use std::{sync::LazyLock, time::Duration};

use entreprises_api::{
    AppState,
    auth::{Identite, Jwt, Role},
    build_app, import,
};
use serde_json::{Value, json};
use sqlx::MySqlPool;

const SECRET: &str = "secret-jwt-des-tests-d-integration-api";
/// JWT signés avec le secret des tests (les comptes eux-mêmes sont testés dans `tests/auth.rs`).
static ADMIN: LazyLock<String> = LazyLock::new(|| jwt().emettre(Identite { id: 1, role: Role::Admin }));
static LECTEUR: LazyLock<String> = LazyLock::new(|| jwt().emettre(Identite { id: 2, role: Role::Lecteur }));

fn jwt() -> Jwt {
    Jwt::new(SECRET, Duration::from_secs(900)).expect("secret de test")
}
const FIXTURE: &str = include_str!("fixtures/entreprises.sql");
const EPI_DOR: &str = "/api/v1/entreprises/036320596";
const RECHERCHE: &str = "/api/v1/entreprises";

async fn serveur(pool: &MySqlPool) -> TestServer {
    import::recopier(pool).await.expect("import de la fixture");
    TestServer::new(build_app(AppState::new(pool.clone(), jwt())))
}

/// Recharge le fichier SQL de l'extrait puis relance l'import (réimport).
async fn reimporter(pool: &MySqlPool) -> import::Rapport {
    import::charger_fichier(pool, FIXTURE).await.expect("rechargement");
    import::recopier(pool).await.expect("réimport")
}

/// Vérifie une réponse d'erreur RFC 9457 et renvoie son corps.
fn probleme(reponse: &TestResponse, status: StatusCode) -> Value {
    reponse.assert_status(status);
    assert_eq!(reponse.content_type(), "application/problem+json");
    let corps: Value = reponse.json();
    assert_eq!(corps["status"], status.as_u16());
    assert!(corps["type"].as_str().unwrap().starts_with("https://"));
    assert!(corps["title"].is_string());
    assert!(corps["instance"].is_string());
    corps
}

fn sirens(reponse: &TestResponse) -> Vec<String> {
    let corps: Value = reponse.json();
    corps["data"].as_array().unwrap().iter().map(|e| e["siren"].as_str().unwrap().to_owned()).collect()
}

fn total(reponse: &TestResponse) -> i64 {
    reponse.assert_status_ok();
    reponse.json::<Value>()["meta"]["total"].as_i64().unwrap()
}

// ---------------------------------------------------------------------------
// E1 — Retrouver une société par son identifiant
// ---------------------------------------------------------------------------

#[sqlx::test(fixtures("entreprises"))]
async fn e1_renvoie_l_entreprise(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse = api.get(EPI_DOR).authorization_bearer(&*LECTEUR).await;
    reponse.assert_status_ok();
    assert_eq!(reponse.content_type(), "application/json");
    reponse.assert_json(&json!({
        "siren": "036320596",
        "denomination": "BOULANGERIE PATISSERIE L'EPI D'OR",
        "nom": null,
        "prenom": null,
        "activite_principale": "10.71C",
        "tranche_effectifs": "03",
        "etat_administratif": "active",
        "date_creation": "1963-01-01",
        "adresse": null,
        "date_derniere_modification": null,
        "links": { "self": "/api/v1/entreprises/036320596" }
    }));
}

#[sqlx::test(fixtures("entreprises"))]
async fn e1_zeros_de_tete_facultatifs(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let court: Value = api.get("/api/v1/entreprises/325175").authorization_bearer(&*LECTEUR).await.json();
    let long: Value = api.get("/api/v1/entreprises/000325175").authorization_bearer(&*LECTEUR).await.json();
    assert_eq!(court, long);
    assert_eq!(court["siren"], "000325175");
    assert_eq!(court["nom"], "JANOYER");
    assert_eq!(court["tranche_effectifs"], "NN");
}

#[sqlx::test(fixtures("entreprises"))]
async fn e1_siren_invalide_400(pool: MySqlPool) {
    let api = serveur(&pool).await;
    for siren in ["abc", "12a45", "1234567890", "-1"] {
        let reponse = api.get(&format!("/api/v1/entreprises/{siren}")).authorization_bearer(&*LECTEUR).await;
        probleme(&reponse, StatusCode::BAD_REQUEST);
    }
}

#[sqlx::test(fixtures("entreprises"))]
async fn e1_siren_inconnu_404(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse = api.get("/api/v1/entreprises/999999999").authorization_bearer(&*LECTEUR).await;
    let corps = probleme(&reponse, StatusCode::NOT_FOUND);
    assert_eq!(corps["title"], "Entreprise introuvable");
    assert_eq!(corps["instance"], "/api/v1/entreprises/999999999");
}

#[sqlx::test(fixtures("entreprises"))]
async fn e1_fields_allege_la_reponse(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse =
        api.get(EPI_DOR).add_query_param("fields", "siren,denomination").authorization_bearer(&*LECTEUR).await;
    reponse.assert_json(&json!({ "siren": "036320596", "denomination": "BOULANGERIE PATISSERIE L'EPI D'OR" }));
    let reponse = api.get(EPI_DOR).add_query_param("fields", "siren,couleur").authorization_bearer(&*LECTEUR).await;
    probleme(&reponse, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Droits et méthodes
// ---------------------------------------------------------------------------

#[sqlx::test(fixtures("entreprises"))]
async fn sans_token_401_avec_www_authenticate(pool: MySqlPool) {
    let api = serveur(&pool).await;
    for reponse in [
        api.get(EPI_DOR).await,
        api.get(RECHERCHE).add_query_param("denomination", "boulangerie").await,
        api.patch(EPI_DOR).json(&json!({ "nom": "X" })).await,
        api.delete(EPI_DOR).await,
    ] {
        probleme(&reponse, StatusCode::UNAUTHORIZED);
        assert_eq!(reponse.header(header::WWW_AUTHENTICATE), "Bearer");
    }
}

#[sqlx::test(fixtures("entreprises"))]
async fn token_inconnu_ou_mal_forme_401(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse = api.get(EPI_DOR).authorization_bearer("token-inconnu").await;
    probleme(&reponse, StatusCode::UNAUTHORIZED);
    assert!(reponse.header(header::WWW_AUTHENTICATE).to_str().unwrap().starts_with("Bearer"));
    let reponse = api.get(EPI_DOR).add_header(header::AUTHORIZATION, "Basic YWRtaW46YWRtaW4=").await;
    probleme(&reponse, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(fixtures("entreprises"))]
async fn lecteur_refuse_en_ecriture_403_meme_avec_corps_invalide(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse = api.patch(EPI_DOR).authorization_bearer(&*LECTEUR).json(&json!({ "siren": "1" })).await;
    let corps = probleme(&reponse, StatusCode::FORBIDDEN);
    assert_eq!(corps["title"], "Action réservée aux administrateurs");
    let reponse = api.patch(EPI_DOR).authorization_bearer(&*LECTEUR).text("pas du json").await;
    probleme(&reponse, StatusCode::FORBIDDEN);
    let reponse = api.delete(EPI_DOR).authorization_bearer(&*LECTEUR).await;
    probleme(&reponse, StatusCode::FORBIDDEN);
    api.get(EPI_DOR).authorization_bearer(&*LECTEUR).await.assert_status_ok();
}

#[sqlx::test(fixtures("entreprises"))]
async fn post_et_put_405_avec_allow(pool: MySqlPool) {
    let api = serveur(&pool).await;
    // Sans token : la méthode est refusée avant l'authentification.
    let reponse = api.post(RECHERCHE).json(&json!({ "siren": "123456789" })).await;
    probleme(&reponse, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(reponse.header(header::ALLOW), "GET,HEAD");

    let reponse = api.put(EPI_DOR).authorization_bearer(&*ADMIN).json(&json!({})).await;
    probleme(&reponse, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(reponse.header(header::ALLOW), "GET,HEAD,PATCH,DELETE");
}

#[sqlx::test(fixtures("entreprises"))]
async fn route_inconnue_404_problem_json(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse = api.get("/api/v1/societes").authorization_bearer(&*LECTEUR).await;
    probleme(&reponse, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// E2 — Chercher par code activité
// ---------------------------------------------------------------------------

#[sqlx::test(fixtures("entreprises"))]
async fn e2_par_code_activite(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let recherche = |query: &'static str| api.get(&format!("{RECHERCHE}?{query}")).authorization_bearer(&*LECTEUR);
    assert_eq!(total(&recherche("activite_principale=10.71C").await), 5);
    assert_eq!(total(&recherche("activite_principale=10.71C&etat_administratif=active").await), 3);
    assert_eq!(total(&recherche("activite_principale=10.71C&etat_administratif=cessee").await), 2);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e2_anciens_codes_trouvables(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse =
        api.get(RECHERCHE).add_query_param("activite_principale", "70.3C").authorization_bearer(&*LECTEUR).await;
    assert_eq!(sirens(&reponse), ["007445281"]);
    let reponse =
        api.get(RECHERCHE).add_query_param("activite_principale", "22.02").authorization_bearer(&*LECTEUR).await;
    assert_eq!(sirens(&reponse), ["005410220"]);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e2_virgule_union_des_codes(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse =
        api.get(RECHERCHE).add_query_param("activite_principale", "10.71C,15.8C").authorization_bearer(&*LECTEUR).await;
    assert_eq!(total(&reponse), 7);
}

// ---------------------------------------------------------------------------
// E3 — Chercher par dénomination
// ---------------------------------------------------------------------------

#[sqlx::test(fixtures("entreprises"))]
async fn e3_par_denomination(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse =
        api.get(RECHERCHE).add_query_param("denomination", "boulangerie").authorization_bearer(&*LECTEUR).await;
    assert_eq!(total(&reponse), 4);
    assert_eq!(sirens(&reponse), ["007050057", "013944129", "015851611", "036320596"]);
    let reponse = api
        .get(RECHERCHE)
        .add_query_param("denomination", "boulangerie")
        .add_query_param("etat_administratif", "active")
        .authorization_bearer(&*LECTEUR)
        .await;
    assert_eq!(total(&reponse), 2);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e3_ignore_casse_et_accents(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reference =
        api.get(RECHERCHE).add_query_param("denomination", "boulangerie").authorization_bearer(&*LECTEUR).await;
    let accentue =
        api.get(RECHERCHE).add_query_param("denomination", "Boulangérie").authorization_bearer(&*LECTEUR).await;
    assert_eq!(sirens(&reference), sirens(&accentue));
    let reponse = api.get(RECHERCHE).add_query_param("denomination", "épi d'OR").authorization_bearer(&*LECTEUR).await;
    assert_eq!(sirens(&reponse), ["036320596"]);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e3_cherche_aussi_nom_et_prenom(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse = api.get(RECHERCHE).add_query_param("denomination", "janoyer").authorization_bearer(&*LECTEUR).await;
    assert_eq!(sirens(&reponse), ["000325175"]);
    let reponse = api.get(RECHERCHE).add_query_param("denomination", "thierry").authorization_bearer(&*LECTEUR).await;
    assert_eq!(sirens(&reponse), ["000325175"]);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e3_filtre_manquant_ou_trop_court_400(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let corps = probleme(&api.get(RECHERCHE).authorization_bearer(&*LECTEUR).await, StatusCode::BAD_REQUEST);
    assert_eq!(corps["title"], "Filtre manquant");
    let reponse =
        api.get(RECHERCHE).add_query_param("etat_administratif", "active").authorization_bearer(&*LECTEUR).await;
    probleme(&reponse, StatusCode::BAD_REQUEST);
    let reponse = api.get(RECHERCHE).add_query_param("denomination", "bo").authorization_bearer(&*LECTEUR).await;
    probleme(&reponse, StatusCode::BAD_REQUEST);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e3_jokers_like_neutralises(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse = api.get(RECHERCHE).add_query_param("denomination", "%%%").authorization_bearer(&*LECTEUR).await;
    assert_eq!(total(&reponse), 0);
    let reponse = api.get(RECHERCHE).add_query_param("denomination", "___").authorization_bearer(&*LECTEUR).await;
    assert_eq!(total(&reponse), 0);
}

// ---------------------------------------------------------------------------
// Pagination et tri
// ---------------------------------------------------------------------------

#[sqlx::test(fixtures("entreprises"))]
async fn pagination_liens_et_limites(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let page = |offset: &'static str, limit: &'static str| {
        api.get(RECHERCHE)
            .add_query_param("activite_principale", "10.71C")
            .add_query_param("offset", offset)
            .add_query_param("limit", limit)
            .authorization_bearer(&*LECTEUR)
    };

    let premiere: Value = page("0", "2").await.json();
    assert_eq!(premiere["meta"], json!({ "total": 5, "offset": 0, "limit": 2 }));
    assert_eq!(premiere["data"].as_array().unwrap().len(), 2);
    assert_eq!(premiere["links"]["self"], "/api/v1/entreprises?activite_principale=10.71C&offset=0&limit=2");
    assert_eq!(premiere["links"]["next"], "/api/v1/entreprises?activite_principale=10.71C&offset=2&limit=2");
    assert!(premiere["links"].get("prev").is_none());

    let derniere: Value = page("4", "2").await.json();
    assert_eq!(derniere["data"].as_array().unwrap().len(), 1);
    assert!(derniere["links"].get("next").is_none());
    assert_eq!(derniere["links"]["prev"], "/api/v1/entreprises?activite_principale=10.71C&offset=2&limit=2");

    let au_dela: Value = page("40", "2").await.json();
    assert_eq!(au_dela["data"], json!([]));

    probleme(&page("0", "101").await, StatusCode::BAD_REQUEST);
    probleme(&page("0", "0").await, StatusCode::BAD_REQUEST);
    probleme(&page("-1", "10").await, StatusCode::BAD_REQUEST);
    probleme(&page("9950", "51").await, StatusCode::BAD_REQUEST);
    probleme(&page("abc", "10").await, StatusCode::BAD_REQUEST);
    page("9950", "50").await.assert_status_ok();
}

#[sqlx::test(fixtures("entreprises"))]
async fn tri_liste_blanche(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let trie = |sort: &'static str| {
        api.get(RECHERCHE)
            .add_query_param("activite_principale", "10.71C")
            .add_query_param("sort", sort)
            .authorization_bearer(&*LECTEUR)
    };
    assert_eq!(sirens(&trie("-siren").await), ["036320596", "024044943", "016850422", "007140627", "006680219"]);
    let par_date: Value = trie("date_creation").await.json();
    let dates: Vec<&str> =
        par_date["data"].as_array().unwrap().iter().filter_map(|e| e["date_creation"].as_str()).collect();
    assert!(dates.is_sorted());
    probleme(&trie("siren; DROP TABLE entreprises").await, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// E4 — Supprimer une entreprise (droit à l'oubli)
// ---------------------------------------------------------------------------

#[sqlx::test(fixtures("entreprises"))]
async fn e4_suppression_puis_404(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse = api.delete(EPI_DOR).authorization_bearer(&*ADMIN).await;
    reponse.assert_status(StatusCode::NO_CONTENT);
    assert!(reponse.as_bytes().is_empty());

    probleme(&api.get(EPI_DOR).authorization_bearer(&*LECTEUR).await, StatusCode::NOT_FOUND);
    probleme(&api.delete(EPI_DOR).authorization_bearer(&*ADMIN).await, StatusCode::NOT_FOUND);
    let reponse =
        api.get(RECHERCHE).add_query_param("denomination", "boulangerie").authorization_bearer(&*LECTEUR).await;
    assert!(!sirens(&reponse).contains(&"036320596".to_owned()));
    let reponse =
        api.get(RECHERCHE).add_query_param("activite_principale", "10.71C").authorization_bearer(&*LECTEUR).await;
    assert_eq!(total(&reponse), 4);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e4_seul_le_siren_reste(pool: MySqlPool) {
    let api = serveur(&pool).await;
    api.delete(EPI_DOR).authorization_bearer(&*ADMIN).await.assert_status(StatusCode::NO_CONTENT);
    let restant: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM entreprises WHERE siren = 36320596").fetch_one(&pool).await.unwrap();
    assert_eq!(restant, 0);
    let oubli: (i64, String) =
        sqlx::query_as("SELECT siren, supprime_le FROM siren_supprimes").fetch_one(&pool).await.unwrap();
    assert_eq!(oubli.0, 36_320_596);
    assert!(oubli.1.ends_with('Z'));
}

#[sqlx::test(fixtures("entreprises"))]
async fn e4_reimport_ne_recree_pas(pool: MySqlPool) {
    let api = serveur(&pool).await;
    api.delete(EPI_DOR).authorization_bearer(&*ADMIN).await.assert_status(StatusCode::NO_CONTENT);
    let rapport = reimporter(&pool).await;
    assert_eq!((rapport.lues, rapport.inserees, rapport.conservees, rapport.oubliees), (16, 0, 15, 1));
    probleme(&api.get(EPI_DOR).authorization_bearer(&*LECTEUR).await, StatusCode::NOT_FOUND);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e4_siren_inconnu_404(pool: MySqlPool) {
    let api = serveur(&pool).await;
    probleme(&api.delete("/api/v1/entreprises/999999999").authorization_bearer(&*ADMIN).await, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// E5 — Modifier une entreprise
// ---------------------------------------------------------------------------

#[sqlx::test(fixtures("entreprises"))]
async fn e5_demenagement(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let adresse = json!({
        "numero_voie": "12",
        "type_voie": "RUE",
        "libelle_voie": "DE LA GARE",
        "code_postal": "79190",
        "commune": "SAUZE-VAUSSAIS"
    });
    let reponse = api.patch(EPI_DOR).authorization_bearer(&*ADMIN).json(&json!({ "adresse": adresse })).await;
    reponse.assert_status_ok();
    let corps: Value = reponse.json();
    assert_eq!(corps["adresse"], adresse);
    assert_eq!(corps["denomination"], "BOULANGERIE PATISSERIE L'EPI D'OR");

    // Correction d'un seul champ de l'adresse : les autres restent.
    let corps: Value = api
        .patch(EPI_DOR)
        .authorization_bearer(&*ADMIN)
        .json(&json!({ "adresse": { "numero_voie": "14" } }))
        .await
        .json();
    assert_eq!(corps["adresse"]["numero_voie"], "14");
    assert_eq!(corps["adresse"]["commune"], "SAUZE-VAUSSAIS");

    // `adresse: null` efface toute l'adresse.
    let corps: Value = api.patch(EPI_DOR).authorization_bearer(&*ADMIN).json(&json!({ "adresse": null })).await.json();
    assert_eq!(corps["adresse"], Value::Null);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e5_changement_de_nom_ne_touche_que_ce_champ(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let avant: Value = api.get(EPI_DOR).authorization_bearer(&*LECTEUR).await.json();
    let reponse = api.patch(EPI_DOR).authorization_bearer(&*ADMIN).json(&json!({ "denomination": "L'EPI D'OR" })).await;
    reponse.assert_status_ok();
    let apres: Value = reponse.json();
    assert_eq!(apres["denomination"], "L'EPI D'OR");
    for champ in [
        "siren",
        "nom",
        "prenom",
        "activite_principale",
        "tranche_effectifs",
        "etat_administratif",
        "date_creation",
        "adresse",
    ] {
        assert_eq!(avant[champ], apres[champ], "{champ}");
    }
    assert_eq!(api.get(EPI_DOR).authorization_bearer(&*LECTEUR).await.json::<Value>(), apres);

    // La recherche suit le nouveau nom.
    let reponse =
        api.get(RECHERCHE).add_query_param("denomination", "boulangerie").authorization_bearer(&*LECTEUR).await;
    assert!(!sirens(&reponse).contains(&"036320596".to_owned()));
    let reponse =
        api.get(RECHERCHE).add_query_param("denomination", "l'épi d'or").authorization_bearer(&*LECTEUR).await;
    assert_eq!(sirens(&reponse), ["036320596"]);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e5_null_vide_absent_inchange(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let corps: Value = api
        .patch("/api/v1/entreprises/325175")
        .authorization_bearer(&*ADMIN)
        .json(&json!({ "prenom": null, "activite_principale": "32.13Z" }))
        .await
        .json();
    assert_eq!(corps["prenom"], Value::Null);
    assert_eq!(corps["nom"], "JANOYER");
    assert_eq!(corps["activite_principale"], "32.13Z");
    let reponse = api.get(RECHERCHE).add_query_param("denomination", "thierry").authorization_bearer(&*LECTEUR).await;
    assert_eq!(total(&reponse), 0);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e5_date_derniere_modification(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let corps: Value =
        api.patch(EPI_DOR).authorization_bearer(&*ADMIN).json(&json!({ "etat_administratif": "cessee" })).await.json();
    assert_eq!(corps["etat_administratif"], "cessee");
    let date = corps["date_derniere_modification"].as_str().expect("date renseignée");
    assert_eq!(date.len(), "2026-09-23T14:05:00Z".len());
    assert!(date.ends_with('Z') && date.as_bytes()[10] == b'T');
}

#[sqlx::test(fixtures("entreprises"))]
async fn e5_422_liste_des_champs_refuses(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse = api
        .patch(EPI_DOR)
        .authorization_bearer(&*ADMIN)
        .json(&json!({
            "siren": "036320597",
            "activite_principale": "70.3C",
            "adresse": { "code_postal": "7919" },
            "denomination": "X".repeat(131)
        }))
        .await;
    let corps = probleme(&reponse, StatusCode::UNPROCESSABLE_ENTITY);
    let champs: Vec<&str> = corps["errors"].as_array().unwrap().iter().map(|e| e["champ"].as_str().unwrap()).collect();
    assert_eq!(champs, ["activite_principale", "adresse.code_postal", "denomination", "siren"]);
    assert!(corps["errors"][0]["message"].is_string());

    // Rien n'a été modifié.
    let actuelle: Value = api.get(EPI_DOR).authorization_bearer(&*LECTEUR).await.json();
    assert_eq!(actuelle["date_derniere_modification"], Value::Null);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e5_content_type_et_json_invalides(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse = api.patch(EPI_DOR).authorization_bearer(&*ADMIN).bytes(Bytes::from_static(br#"{"nom":"X"}"#)).await;
    probleme(&reponse, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let reponse = api
        .patch(EPI_DOR)
        .authorization_bearer(&*ADMIN)
        .content_type("application/json")
        .bytes(Bytes::from_static(b"{ nom: "))
        .await;
    probleme(&reponse, StatusCode::BAD_REQUEST);
    let reponse = api.patch(EPI_DOR).authorization_bearer(&*ADMIN).json(&json!(["nom"])).await;
    probleme(&reponse, StatusCode::BAD_REQUEST);
    let reponse = api.patch(EPI_DOR).authorization_bearer(&*ADMIN).json(&json!({})).await;
    probleme(&reponse, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e5_siren_inconnu_ou_invalide(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse =
        api.patch("/api/v1/entreprises/999999999").authorization_bearer(&*ADMIN).json(&json!({ "nom": "X" })).await;
    probleme(&reponse, StatusCode::NOT_FOUND);
    let reponse = api.patch("/api/v1/entreprises/abc").authorization_bearer(&*ADMIN).json(&json!({ "nom": "X" })).await;
    probleme(&reponse, StatusCode::BAD_REQUEST);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e5_reimport_garde_les_modifications(pool: MySqlPool) {
    let api = serveur(&pool).await;
    api.patch(EPI_DOR)
        .authorization_bearer(&*ADMIN)
        .json(&json!({ "denomination": "L'EPI D'OR" }))
        .await
        .assert_status_ok();
    let rapport = reimporter(&pool).await;
    assert_eq!((rapport.inserees, rapport.conservees), (0, 16));
    let corps: Value = api.get(EPI_DOR).authorization_bearer(&*LECTEUR).await.json();
    assert_eq!(corps["denomination"], "L'EPI D'OR");
}

// ---------------------------------------------------------------------------
// Robustesse (constats de l'audit)
// ---------------------------------------------------------------------------

#[sqlx::test(fixtures("entreprises"))]
async fn offset_geant_400_sans_debordement(pool: MySqlPool) {
    let api = serveur(&pool).await;
    for offset in ["9223372036854775807", "9223372036854775800", "99999999999999999999"] {
        let reponse = api
            .get(RECHERCHE)
            .add_query_param("denomination", "boulangerie")
            .add_query_param("offset", offset)
            .add_query_param("limit", "100")
            .authorization_bearer(&*LECTEUR)
            .await;
        probleme(&reponse, StatusCode::BAD_REQUEST);
    }
}

#[sqlx::test(fixtures("entreprises"))]
async fn denomination_de_controle_ou_trop_longue_400(pool: MySqlPool) {
    let api = serveur(&pool).await;
    for terme in ["\0\0\0".to_owned(), "a".repeat(253), "北".to_owned(), "_".repeat(60_000)] {
        let reponse = api.get(RECHERCHE).add_query_param("denomination", &terme).authorization_bearer(&*LECTEUR).await;
        probleme(&reponse, StatusCode::BAD_REQUEST);
    }
    let reponse =
        api.get(RECHERCHE).add_query_param("denomination", "boulangẽrie").authorization_bearer(&*LECTEUR).await;
    assert_eq!(total(&reponse), 4);
}

#[sqlx::test(fixtures("entreprises"))]
async fn options_405_sans_cors(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse = api.method(axum::http::Method::OPTIONS, EPI_DOR).await;
    probleme(&reponse, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(reponse.header(header::ALLOW), "GET,HEAD,PATCH,DELETE");
    assert!(reponse.maybe_header(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    probleme(&api.method(axum::http::Method::OPTIONS, "/").await, StatusCode::NOT_FOUND);
}

#[sqlx::test(fixtures("entreprises"))]
async fn rejets_axum_en_francais(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse =
        api.get(&format!("{RECHERCHE}?denomination=abc&denomination=def")).authorization_bearer(&*LECTEUR).await;
    let corps = probleme(&reponse, StatusCode::BAD_REQUEST);
    assert!(corps["detail"].as_str().unwrap().contains("« denomination » apparaît plusieurs fois"));

    let reponse = api
        .patch(EPI_DOR)
        .authorization_bearer(&*ADMIN)
        .content_type("application/json")
        .bytes(Bytes::from_static(b"{\n  \"nom\": }"))
        .await;
    let corps = probleme(&reponse, StatusCode::BAD_REQUEST);
    assert_eq!(corps["detail"], "Le corps n'est pas un JSON valide (ligne 2, colonne 10).");

    let reponse = api.get("/api/v1/entreprises/%FF").authorization_bearer(&*LECTEUR).await;
    let corps = probleme(&reponse, StatusCode::BAD_REQUEST);
    assert_eq!(corps["title"], "SIREN invalide");

    let reponse = api.delete("/api/v1/entreprises/abc").authorization_bearer(&*ADMIN).await;
    probleme(&reponse, StatusCode::BAD_REQUEST);
}

#[sqlx::test(fixtures("entreprises"))]
async fn e5_chiffres_non_ascii_422(pool: MySqlPool) {
    let api = serveur(&pool).await;
    let reponse = api
        .patch(EPI_DOR)
        .authorization_bearer(&*ADMIN)
        .json(&json!({ "activite_principale": "١٠.٧١C", "adresse": { "code_postal": "７９１９０" } }))
        .await;
    let corps = probleme(&reponse, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(corps["errors"].as_array().unwrap().len(), 2);
}

#[sqlx::test(fixtures("entreprises"))]
async fn reset_puis_reimport_repart_de_zero(pool: MySqlPool) {
    let api = serveur(&pool).await;
    api.delete(EPI_DOR).authorization_bearer(&*ADMIN).await.assert_status(StatusCode::NO_CONTENT);
    api.patch("/api/v1/entreprises/325175").authorization_bearer(&*ADMIN).json(&json!({ "prenom": null })).await;
    import::reinitialiser(&pool).await.unwrap();
    let rapport = reimporter(&pool).await;
    assert_eq!((rapport.inserees, rapport.oubliees), (16, 0));
    api.get(EPI_DOR).authorization_bearer(&*LECTEUR).await.assert_status_ok();
    let corps: Value = api.get("/api/v1/entreprises/325175").authorization_bearer(&*LECTEUR).await.json();
    assert_eq!(corps["prenom"], "THIERRY");
}
