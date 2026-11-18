//! Handlers des exigences E1 à E5 (la connexion se fait dans Keycloak). L'extracteur d'authentification est
//! toujours le premier argument ; les autres passent par `WithRejection` pour que leurs rejets sortent en
//! problem+json.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use axum_extra::extract::WithRejection;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    AppState,
    auth::{Admin, Lecteur},
    errors::{ApiError, Problem},
    models::{Entreprise, Fields, Meta, Page, PatchEntreprise, Recherche, SearchParams, vers_json},
    normalize, repository,
};

#[derive(Debug, Default, Deserialize)]
pub struct FieldsParam {
    fields: Option<String>,
}

/// E1 — `GET /api/v1/entreprises/{siren}`.
#[utoipa::path(
    get,
    path = "/api/v1/entreprises/{siren}",
    tag = "Entreprises",
    summary = "Retrouver une entreprise par son SIREN",
    params(("siren" = String, Path, description = "SIREN, 9 chiffres au plus (zéros de tête facultatifs)", example = "036320596"), ("fields" = Option<String>, Query, description = "Champs à renvoyer, séparés par des virgules : siren, denomination, nom, prenom, activite_principale, tranche_effectifs, etat_administratif, date_creation, adresse, date_derniere_modification, links", example = "siren,denomination")),
    responses(
        (status = 200, description = "L'entreprise (seulement les champs demandés avec `fields`)", body = Entreprise),
        (status = 400, description = "SIREN ou `fields` invalide", body = Problem, content_type = "application/problem+json"),
        (status = 401, description = "Token absent, invalide ou expiré", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "SIREN inconnu ou supprimé", body = Problem, content_type = "application/problem+json"),
    ),
    security(("keycloak" = [])),
)]
pub async fn obtenir(
    _: Lecteur,
    State(state): State<AppState>,
    WithRejection(Path(siren), _): WithRejection<Path<String>, ApiError>,
    WithRejection(Query(params), _): WithRejection<Query<FieldsParam>, ApiError>,
) -> Result<Json<Value>, ApiError> {
    let siren = lire_siren(&siren)?;
    let fields = Fields::parse(params.fields.as_deref())?;
    let ligne = repository::trouver(&state.pool, siren)
        .await?
        .ok_or_else(|| ApiError::Introuvable(normalize::format_siren(siren)))?;
    Ok(Json(vers_json(Entreprise::from(ligne), fields.as_ref())))
}

/// E2 / E3 — `GET /api/v1/entreprises?activite_principale=…&denomination=…`.
#[utoipa::path(
    get,
    path = "/api/v1/entreprises",
    tag = "Entreprises",
    summary = "Chercher des entreprises par code activité et/ou dénomination",
    description = "`activite_principale` ou `denomination` est obligatoire. Les filtres se cumulent (ET) ; dans un filtre, \
        la virgule vaut OU (20 valeurs distinctes au plus). Pagination : `offset + limit ≤ 10 000`.",
    params(
        ("activite_principale" = Option<String>, Query, description = "Codes NAF exacts, séparés par des virgules", example = "10.71C"),
        ("denomination" = Option<String>, Query, description = "3 à 252 caractères, sans tenir compte de la casse ni des accents ; cherché aussi dans nom et prénom", example = "boulangerie"),
        ("etat_administratif" = Option<String>, Query, description = "`active` et/ou `cessee`"),
        ("sort" = Option<String>, Query, description = "`siren` (défaut), `denomination` ou `date_creation` ; préfixe `-` pour décroissant", example = "-date_creation"),
        ("offset" = Option<i64>, Query, description = "Position du premier résultat (défaut 0)", minimum = 0),
        ("limit" = Option<i64>, Query, description = "Taille de la page (défaut 20)", minimum = 1, maximum = 100),
        ("fields" = Option<String>, Query, description = "Champs à renvoyer, séparés par des virgules : siren, denomination, nom, prenom, activite_principale, tranche_effectifs, etat_administratif, date_creation, adresse, date_derniere_modification, links", example = "siren,denomination"),
    ),
    responses(
        (status = 200, description = "Une page de résultats avec `meta` et les liens `self`, `next`, `prev`", body = Page),
        (status = 400, description = "Filtre manquant, terme trop court ou trop long, pagination ou tri invalide", body = Problem, content_type = "application/problem+json"),
        (status = 401, description = "Token absent, invalide ou expiré", body = Problem, content_type = "application/problem+json"),
    ),
    security(("keycloak" = [])),
)]
pub async fn rechercher(
    _: Lecteur,
    State(state): State<AppState>,
    WithRejection(Query(params), _): WithRejection<Query<SearchParams>, ApiError>,
) -> Result<Json<Page>, ApiError> {
    let recherche = Recherche::depuis(params)?;
    let (total, lignes) = repository::rechercher(&state.pool, &recherche).await?;
    let data = lignes.into_iter().map(|ligne| vers_json(Entreprise::from(ligne), recherche.fields.as_ref())).collect();
    Ok(Json(Page {
        data,
        meta: Meta { total, offset: recherche.offset, limit: recherche.limit },
        links: recherche.liens(total),
    }))
}

/// E4 — `DELETE /api/v1/entreprises/{siren}` (droit à l'oubli).
#[utoipa::path(
    delete,
    path = "/api/v1/entreprises/{siren}",
    tag = "Entreprises",
    summary = "Supprimer une entreprise (droit à l'oubli)",
    description = "Suppression définitive : l'entreprise n'est pas recréée par un réimport.",
    params(("siren" = String, Path, description = "SIREN, 9 chiffres au plus (zéros de tête facultatifs)", example = "036320596")),
    responses(
        (status = 204, description = "Entreprise supprimée"),
        (status = 400, description = "SIREN invalide", body = Problem, content_type = "application/problem+json"),
        (status = 401, description = "Token absent, invalide ou expiré", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Compte lecteur : action réservée aux administrateurs", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "SIREN inconnu ou déjà supprimé", body = Problem, content_type = "application/problem+json"),
    ),
    security(("keycloak" = [])),
)]
pub async fn supprimer(
    Admin(admin): Admin,
    State(state): State<AppState>,
    WithRejection(Path(siren), _): WithRejection<Path<String>, ApiError>,
) -> Result<StatusCode, ApiError> {
    let siren = lire_siren(&siren)?;
    if !repository::supprimer(&state.pool, siren).await? {
        return Err(ApiError::Introuvable(normalize::format_siren(siren)));
    }
    tracing::info!(siren = %normalize::format_siren(siren), par = %admin.id, "entreprise supprimée (droit à l'oubli)");
    Ok(StatusCode::NO_CONTENT)
}

/// E5 — `PATCH /api/v1/entreprises/{siren}` (déménagement, changement de nom, erreur de saisie).
#[utoipa::path(
    patch,
    path = "/api/v1/entreprises/{siren}",
    tag = "Entreprises",
    summary = "Modifier une entreprise",
    description = "Modification partielle : un champ absent reste inchangé, un champ à `null` est vidé. \
        Tous les champs refusés sont renvoyés d'un coup dans la 422.",
    params(("siren" = String, Path, description = "SIREN, 9 chiffres au plus (zéros de tête facultatifs)", example = "036320596")),
    request_body(content = PatchEntreprise, description = "Champs à modifier"),
    responses(
        (status = 200, description = "L'entreprise après modification", body = Entreprise),
        (status = 400, description = "SIREN invalide ou corps qui n'est pas un objet JSON", body = Problem, content_type = "application/problem+json"),
        (status = 401, description = "Token absent, invalide ou expiré", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Compte lecteur : action réservée aux administrateurs", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "SIREN inconnu ou supprimé", body = Problem, content_type = "application/problem+json"),
        (status = 415, description = "Header `Content-Type: application/json` manquant", body = Problem, content_type = "application/problem+json"),
        (status = 422, description = "Champs refusés, listés dans `errors`", body = Problem, content_type = "application/problem+json"),
    ),
    security(("keycloak" = [])),
)]
pub async fn modifier(
    Admin(admin): Admin,
    State(state): State<AppState>,
    WithRejection(Path(siren), _): WithRejection<Path<String>, ApiError>,
    WithRejection(Json(corps), _): WithRejection<Json<Value>, ApiError>,
) -> Result<Json<Value>, ApiError> {
    let siren = lire_siren(&siren)?;
    let patch = PatchEntreprise::depuis_json(corps)?;
    let ligne = repository::modifier(&state.pool, siren, &patch)
        .await?
        .ok_or_else(|| ApiError::Introuvable(normalize::format_siren(siren)))?;
    tracing::info!(siren = %normalize::format_siren(siren), par = %admin.id, "entreprise modifiée");
    Ok(Json(vers_json(Entreprise::from(ligne), None)))
}

/// SIREN de l'URI : chiffres uniquement, 9 au plus (plus strict que `Path<u32>`, qui accepte `+123`
/// et `0000000001`).
fn lire_siren(brut: &str) -> Result<i64, ApiError> {
    normalize::parse_siren(brut).ok_or_else(|| {
        ApiError::requete_invalide(
            "SIREN invalide",
            format!("« {brut} » : le SIREN compte 9 chiffres au plus (les zéros devant peuvent être omis)."),
        )
    })
}
