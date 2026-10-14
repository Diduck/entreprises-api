//! Erreurs de l'API au format RFC 9457 (`application/problem+json`).

use axum::{
    body::Body,
    extract::{
        Request,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

/// Préfixe des URI du champ `type`.
const TYPE_BASE: &str = "https://api.exemple.fr/erreurs/";
const PROBLEM_JSON: &str = "application/problem+json";

/// Un champ refusé dans une réponse 422.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct FieldError {
    #[schema(example = "adresse.code_postal")]
    pub champ: String,
    #[schema(example = "5 chiffres attendus, ex. 79190.")]
    pub message: String,
}

impl FieldError {
    pub fn new(champ: impl Into<String>, message: impl Into<String>) -> Self {
        Self { champ: champ.into(), message: message.into() }
    }
}

/// Corps d'une réponse d'erreur (RFC 9457).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Problem {
    #[serde(rename = "type")]
    #[schema(example = "https://api.exemple.fr/erreurs/introuvable")]
    pub type_: String,
    #[schema(example = "Entreprise introuvable")]
    pub title: String,
    #[schema(example = 404)]
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = "Aucune entreprise avec le SIREN 999999999.")]
    pub detail: Option<String>,
    /// Chemin et query string de la requête.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = "/api/v1/entreprises/999999999")]
    pub instance: Option<String>,
    /// Champs refusés, uniquement sur les 422.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<FieldError>>,
}

impl Problem {
    fn new(status: StatusCode, slug: &str, title: &str, detail: Option<String>) -> Self {
        Self {
            type_: format!("{TYPE_BASE}{slug}"),
            title: title.to_owned(),
            status: status.as_u16(),
            detail,
            instance: None,
            errors: None,
        }
    }

    /// Problème générique pour une réponse d'erreur produite hors des handlers
    /// (routage, extracteurs Axum, timeout).
    pub fn depuis_statut(status: StatusCode) -> Self {
        let (slug, title, detail) = match status.as_u16() {
            400 => ("requete-invalide", "Requête invalide", None),
            401 => ("non-authentifie", "Authentification requise", None),
            403 => ("acces-refuse", "Accès refusé", None),
            404 => ("introuvable", "Ressource introuvable", Some("Aucune ressource ne correspond à cette URI.")),
            409 => ("conflit", "Conflit", None),
            405 => {
                ("methode-non-autorisee", "Méthode non autorisée", Some("Méthodes acceptées : voir le header Allow."))
            }
            408 => ("delai-depasse", "Délai dépassé", Some("La requête a pris plus de 10 secondes.")),
            413 => ("corps-trop-volumineux", "Corps de requête trop volumineux", None),
            415 => ("type-media-non-supporte", "Type de contenu non supporté", None),
            422 => ("validation", "Données invalides", None),
            _ if status.is_server_error() => ("erreur-interne", "Erreur interne", None),
            _ => ("erreur", status.canonical_reason().unwrap_or("Erreur"), None),
        };
        Self::new(status, slug, title, detail.map(str::to_owned))
    }
}

/// Toutes les erreurs renvoyées par les handlers et les extracteurs.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// 400 — paramètre, SIREN ou corps de requête invalide.
    #[error("{title} : {detail}")]
    RequeteInvalide { title: &'static str, detail: String },
    /// 401 — JWT absent, invalide ou expiré.
    #[error("authentification requise")]
    NonAuthentifie { token_fourni: bool },
    /// 401 — connexion refusée : email inconnu ou mot de passe faux (sans dire lequel).
    #[error("identifiants invalides")]
    IdentifiantsInvalides,
    /// 403 — compte lecteur sur une action admin.
    #[error("action réservée aux administrateurs")]
    AccesRefuse,
    /// 404 — entreprise inconnue ou supprimée.
    #[error("entreprise introuvable : {0}")]
    Introuvable(String),
    /// 404 — utilisateur inconnu.
    #[error("utilisateur introuvable : {0}")]
    UtilisateurIntrouvable(i64),
    /// 409 — email déjà utilisé par un autre compte.
    #[error("email déjà utilisé")]
    EmailDejaUtilise,
    /// 415 — `Content-Type: application/json` manquant.
    #[error("type de contenu non supporté")]
    TypeMediaNonSupporte,
    /// 422 — un ou plusieurs champs refusés.
    #[error("données invalides")]
    Validation(Vec<FieldError>),
    /// Autre rejet d'un extracteur Axum (413…) : statut conservé, détail en français.
    #[error("{detail}")]
    Rejet { status: StatusCode, detail: String },
    /// 500 — erreur de base de données : détaillée dans les logs, jamais dans la réponse.
    #[error(transparent)]
    Base(#[from] sqlx::Error),
}

impl ApiError {
    pub fn requete_invalide(title: &'static str, detail: impl Into<String>) -> Self {
        Self::RequeteInvalide { title, detail: detail.into() }
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::RequeteInvalide { .. } => StatusCode::BAD_REQUEST,
            Self::NonAuthentifie { .. } | Self::IdentifiantsInvalides => StatusCode::UNAUTHORIZED,
            Self::AccesRefuse => StatusCode::FORBIDDEN,
            Self::Introuvable(_) | Self::UtilisateurIntrouvable(_) => StatusCode::NOT_FOUND,
            Self::EmailDejaUtilise => StatusCode::CONFLICT,
            Self::TypeMediaNonSupporte => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Rejet { status, .. } => *status,
            Self::Base(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn problem(&self) -> Problem {
        let status = self.status();
        match self {
            Self::RequeteInvalide { title, detail } => {
                Problem::new(status, "requete-invalide", title, Some(detail.clone()))
            }
            Self::NonAuthentifie { token_fourni: false } => Problem::new(
                status,
                "non-authentifie",
                "Authentification requise",
                Some("Se connecter via POST /api/v1/tokens, puis envoyer le header Authorization: Bearer <token>.".into()),
            ),
            Self::NonAuthentifie { token_fourni: true } => Problem::new(
                status,
                "non-authentifie",
                "Token invalide ou expiré",
                Some("Le token est mal formé, expiré ou signé par une autre clé : se reconnecter via POST /api/v1/tokens.".into()),
            ),
            Self::IdentifiantsInvalides => Problem::new(
                status,
                "identifiants-invalides",
                "Identifiants invalides",
                Some("Email ou mot de passe incorrect.".into()),
            ),
            Self::AccesRefuse => Problem::new(
                status,
                "acces-refuse",
                "Action réservée aux administrateurs",
                Some("Ce compte a le rôle lecteur : seules les lectures (GET) sont permises.".into()),
            ),
            Self::UtilisateurIntrouvable(id) => Problem::new(
                status,
                "introuvable",
                "Utilisateur introuvable",
                Some(format!("Aucun utilisateur avec l'identifiant {id}.")),
            ),
            Self::EmailDejaUtilise => Problem::new(
                status,
                "conflit",
                "Email déjà utilisé",
                Some("Un compte existe déjà avec cet email.".into()),
            ),
            Self::Introuvable(siren) => Problem::new(
                status,
                "introuvable",
                "Entreprise introuvable",
                Some(format!("Aucune entreprise avec le SIREN {siren}.")),
            ),
            Self::TypeMediaNonSupporte => Problem::new(
                status,
                "type-media-non-supporte",
                "Type de contenu non supporté",
                Some("Envoyer le corps en JSON avec le header Content-Type: application/json.".into()),
            ),
            Self::Validation(errors) => {
                let mut problem = Problem::new(
                    status,
                    "validation",
                    "Données invalides",
                    Some(format!("{} champ(s) refusé(s).", errors.len())),
                );
                problem.errors = Some(errors.clone());
                problem
            }
            Self::Rejet { detail, .. } => {
                let mut problem = Problem::depuis_statut(status);
                problem.detail = Some(detail.clone());
                problem
            }
            Self::Base(_) => Problem::new(
                status,
                "erreur-interne",
                "Erreur interne",
                Some("Une erreur inattendue est survenue. Elle a été journalisée.".into()),
            ),
        }
    }
}

// Rejets des extracteurs Axum (texte brut en anglais) convertis via `WithRejection` d'axum-extra.

impl From<JsonRejection> for ApiError {
    fn from(rejet: JsonRejection) -> Self {
        match rejet {
            JsonRejection::MissingJsonContentType(_) => Self::TypeMediaNonSupporte,
            JsonRejection::JsonSyntaxError(erreur) => {
                let detail = match position_json(&erreur.body_text()) {
                    Some((ligne, colonne)) => {
                        format!("Le corps n'est pas un JSON valide (ligne {ligne}, colonne {colonne}).")
                    }
                    None => "Le corps n'est pas un JSON valide.".to_owned(),
                };
                Self::requete_invalide("JSON invalide", detail)
            }
            autre if autre.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                Self::Rejet { status: autre.status(), detail: "Le corps de la requête dépasse 2 Mo.".into() }
            }
            autre => Self::Rejet { status: autre.status(), detail: "Corps de requête illisible.".into() },
        }
    }
}

impl From<QueryRejection> for ApiError {
    fn from(rejet: QueryRejection) -> Self {
        // Seule erreur possible avec des paramètres tous optionnels et textuels : un paramètre répété.
        let texte = rejet.body_text();
        let detail = match texte.split('`').nth(1) {
            Some(parametre) if texte.contains("duplicate field") => {
                format!(
                    "Le paramètre « {parametre} » apparaît plusieurs fois : utiliser la virgule pour plusieurs valeurs."
                )
            }
            _ => "Query string mal formée.".to_owned(),
        };
        Self::requete_invalide("Paramètre invalide", detail)
    }
}

impl From<PathRejection> for ApiError {
    fn from(rejet: PathRejection) -> Self {
        if rejet.status() == StatusCode::BAD_REQUEST {
            Self::requete_invalide("SIREN invalide", "Le SIREN de l'URI est mal encodé : 9 chiffres au plus attendus.")
        } else {
            Self::Rejet { status: rejet.status(), detail: "Paramètre de chemin manquant.".into() }
        }
    }
}

/// Ligne et colonne d'une erreur JSON, lues dans le message d'Axum (« … at line 2 column 10 »).
fn position_json(message: &str) -> Option<(usize, usize)> {
    let (_, position) = message.rsplit_once(" at line ")?;
    let (ligne, colonne) = position.split_once(" column ")?;
    Some((ligne.trim().parse().ok()?, colonne.trim().parse().ok()?))
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let Self::Base(erreur) = &self {
            tracing::error!(error = %erreur, "erreur de base de données");
        }
        let mut response = reponse_problem(self.status(), self.problem());
        let defi = match self {
            Self::NonAuthentifie { token_fourni: true } => Some(r#"Bearer error="invalid_token""#),
            Self::NonAuthentifie { token_fourni: false } | Self::IdentifiantsInvalides => Some("Bearer"),
            _ => None,
        };
        if let Some(valeur) = defi {
            response.headers_mut().insert(header::WWW_AUTHENTICATE, HeaderValue::from_static(valeur));
        }
        response
    }
}

fn reponse_problem(status: StatusCode, problem: Problem) -> Response {
    let corps = serde_json::to_vec(&problem).unwrap_or_default();
    let mut response = (status, [(header::CONTENT_TYPE, PROBLEM_JSON)], corps).into_response();
    // Le middleware `problem_details` s'en sert pour compléter `instance`.
    response.extensions_mut().insert(problem);
    response
}

/// Middleware : toute réponse 4xx/5xx sort en `application/problem+json` avec `instance`.
///
/// Couvre les erreurs des handlers comme celles produites par Axum ou tower-http
/// (404 de route, 405, rejets d'extracteurs, 408 du timeout), en gardant leurs headers (`Allow`…).
pub async fn problem_details(request: Request, next: Next) -> Response {
    let instance =
        request.uri().path_and_query().map_or_else(|| request.uri().path().to_owned(), |pq| pq.as_str().to_owned());
    let response = next.run(request).await;
    let status = response.status();
    if !status.is_client_error() && !status.is_server_error() {
        return response;
    }

    let (mut parts, _) = response.into_parts();
    let mut problem = parts.extensions.remove::<Problem>().unwrap_or_else(|| Problem::depuis_statut(status));
    problem.instance = Some(instance);
    parts.headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(PROBLEM_JSON));
    parts.headers.remove(header::CONTENT_LENGTH);
    let corps = serde_json::to_vec(&problem).unwrap_or_default();
    Response::from_parts(parts, Body::from(corps))
}

/// Réponse aux URI qui ne correspondent à aucune route.
pub async fn route_inconnue() -> Response {
    reponse_problem(StatusCode::NOT_FOUND, Problem::depuis_statut(StatusCode::NOT_FOUND))
}

/// Réponse aux méthodes non proposées (`POST`, `PUT`…). Axum ajoute le header `Allow`.
pub async fn methode_non_autorisee() -> Response {
    reponse_problem(StatusCode::METHOD_NOT_ALLOWED, Problem::depuis_statut(StatusCode::METHOD_NOT_ALLOWED))
}
