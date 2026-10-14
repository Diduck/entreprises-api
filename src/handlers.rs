//! Handlers des exigences E1 à E5, de la connexion et des comptes. L'extracteur d'authentification est
//! toujours le premier argument ; les autres passent par `WithRejection` pour que leurs rejets sortent en
//! problem+json.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use axum_extra::extract::WithRejection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::{
    AppState,
    auth::{Admin, Identite, Lecteur},
    errors::{ApiError, Problem},
    models::{Entreprise, Fields, Meta, Page, PatchEntreprise, Recherche, SearchParams, vers_json},
    normalize, repository,
    utilisateurs::{self, Connexion, NouvelUtilisateur, Utilisateur},
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
    security(("jwt" = [])),
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
    security(("jwt" = [])),
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
    security(("jwt" = [])),
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
    tracing::info!(siren = %normalize::format_siren(siren), par = admin.id, "entreprise supprimée (droit à l'oubli)");
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
    security(("jwt" = [])),
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
    tracing::info!(siren = %normalize::format_siren(siren), par = admin.id, "entreprise modifiée");
    Ok(Json(vers_json(Entreprise::from(ligne), None)))
}

/// Réponse de `POST /api/v1/tokens` (noms de champs de la RFC 6749, connus des clients OAuth).
#[derive(Debug, Serialize, ToSchema)]
pub struct TokenEmis {
    #[schema(example = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl")]
    access_token: String,
    #[schema(value_type = String, example = "Bearer")]
    token_type: &'static str,
    /// Durée de validité en secondes.
    #[schema(example = 900)]
    expires_in: u64,
}

/// Connexion — `POST /api/v1/tokens` : email + mot de passe → JWT.
#[utoipa::path(
    post,
    path = "/api/v1/tokens",
    tag = "Authentification",
    summary = "Se connecter et obtenir un JWT",
    description = "Comptes de démonstration : `lecteur@demo.fr` / `lecteur-demo-mdp`, `admin@demo.fr` / `admin-demo-mdp`.",
    request_body = Connexion,
    responses(
        (status = 200, description = "Token à envoyer dans `Authorization: Bearer <access_token>`", body = TokenEmis,
            headers(("Cache-Control" = String, description = "`no-store`"))),
        (status = 400, description = "Corps qui n'est pas un objet JSON", body = Problem, content_type = "application/problem+json"),
        (status = 401, description = "Email ou mot de passe incorrect", body = Problem, content_type = "application/problem+json"),
        (status = 415, description = "Header `Content-Type: application/json` manquant", body = Problem, content_type = "application/problem+json"),
        (status = 422, description = "Champs refusés, listés dans `errors`", body = Problem, content_type = "application/problem+json"),
    ),
    security(()),
)]
pub async fn connexion(
    State(state): State<AppState>,
    WithRejection(Json(corps), _): WithRejection<Json<Value>, ApiError>,
) -> Result<Response, ApiError> {
    let Connexion { email, mot_de_passe } = Connexion::depuis_json(corps)?;
    let compte =
        utilisateurs::authentifier(&state.pool, &email, mot_de_passe).await?.ok_or(ApiError::IdentifiantsInvalides)?;
    let token = state.jwt.emettre(Identite { id: compte.id, role: compte.role() });
    tracing::info!(utilisateur = compte.id, "connexion");
    let mut reponse =
        Json(TokenEmis { access_token: token, token_type: "Bearer", expires_in: state.jwt.duree().as_secs() })
            .into_response();
    // Un token ne doit être gardé par aucun cache (RFC 6749 § 5.1).
    reponse.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(reponse)
}

/// `POST /api/v1/utilisateurs` (admin) : crée un compte.
#[utoipa::path(
    post,
    path = "/api/v1/utilisateurs",
    tag = "Utilisateurs",
    summary = "Créer un compte",
    request_body = NouvelUtilisateur,
    responses(
        (status = 201, description = "Compte créé", body = Utilisateur,
            headers(("Location" = String, description = "URI du compte, ex. `/api/v1/utilisateurs/3`"))),
        (status = 400, description = "Corps qui n'est pas un objet JSON", body = Problem, content_type = "application/problem+json"),
        (status = 401, description = "Token absent, invalide ou expiré", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Compte lecteur : action réservée aux administrateurs", body = Problem, content_type = "application/problem+json"),
        (status = 409, description = "Email déjà utilisé", body = Problem, content_type = "application/problem+json"),
        (status = 415, description = "Header `Content-Type: application/json` manquant", body = Problem, content_type = "application/problem+json"),
        (status = 422, description = "Champs refusés, listés dans `errors`", body = Problem, content_type = "application/problem+json"),
    ),
    security(("jwt" = [])),
)]
pub async fn creer_utilisateur(
    Admin(admin): Admin,
    State(state): State<AppState>,
    WithRejection(Json(corps), _): WithRejection<Json<Value>, ApiError>,
) -> Result<Response, ApiError> {
    let nouveau = NouvelUtilisateur::depuis_json(corps)?;
    let mot_de_passe = nouveau.mot_de_passe.clone();
    let empreinte = tokio::task::spawn_blocking(move || utilisateurs::hacher(&mot_de_passe))
        .await
        .expect("hachage du mot de passe");
    let compte = utilisateurs::creer(&state.pool, &nouveau, &empreinte).await?.ok_or(ApiError::EmailDejaUtilise)?;
    tracing::info!(utilisateur = compte.id, role = %compte.role, par = admin.id, "compte créé");
    let location = HeaderValue::try_from(utilisateurs::chemin(compte.id)).expect("chemin ASCII");
    Ok((StatusCode::CREATED, [(header::LOCATION, location)], Json(Utilisateur::from(compte))).into_response())
}

/// `GET /api/v1/utilisateurs/{id}` (admin).
#[utoipa::path(
    get,
    path = "/api/v1/utilisateurs/{id}",
    tag = "Utilisateurs",
    summary = "Lire un compte",
    params(("id" = i64, Path, description = "Identifiant du compte", minimum = 1, example = 1)),
    responses(
        (status = 200, description = "Le compte, sans son mot de passe", body = Utilisateur),
        (status = 400, description = "Identifiant qui n'est pas un entier positif", body = Problem, content_type = "application/problem+json"),
        (status = 401, description = "Token absent, invalide ou expiré", body = Problem, content_type = "application/problem+json"),
        (status = 403, description = "Compte lecteur : action réservée aux administrateurs", body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "Aucun compte avec cet identifiant", body = Problem, content_type = "application/problem+json"),
    ),
    security(("jwt" = [])),
)]
pub async fn obtenir_utilisateur(
    _: Admin,
    State(state): State<AppState>,
    WithRejection(Path(id), _): WithRejection<Path<String>, ApiError>,
) -> Result<Json<Utilisateur>, ApiError> {
    let id: i64 = id.parse().ok().filter(|id| *id > 0).ok_or_else(|| {
        ApiError::requete_invalide("Identifiant invalide", format!("« {id} » : entier positif attendu."))
    })?;
    let compte = utilisateurs::par_id(&state.pool, id).await?.ok_or(ApiError::UtilisateurIntrouvable(id))?;
    Ok(Json(Utilisateur::from(compte)))
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
