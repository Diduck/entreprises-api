//! Documentation OpenAPI générée par `utoipa` depuis les handlers et les modèles, servie par Swagger UI.
//!
//! - `GET /docs` : Swagger UI (bouton « Authorize » pour coller le `access_token`) ;
//! - `GET /api-docs/openapi.json` : la spécification brute.

use axum::Router;
use utoipa::{
    Modify, OpenApi,
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    AppState,
    auth::Role,
    errors::{FieldError, Problem},
    handlers,
    models::{Adresse, Entreprise, Links, Meta, Page, PageLinks, PatchAdresse, PatchEntreprise},
    utilisateurs::{Connexion, Liens, NouvelUtilisateur, Utilisateur},
};

/// Nom du schéma de sécurité référencé par `security(("jwt" = []))` dans les handlers.
pub const JWT: &str = "jwt";

#[derive(OpenApi)]
#[openapi(
    info(
        title = "API REST Entreprises",
        description = "Données Sirene de l'INSEE (Licence Ouverte 2.0) : 99 999 entreprises.\n\n\
            1. Se connecter avec `POST /api/v1/tokens` (ex. `lecteur@demo.fr` / `lecteur-demo-mdp`).\n\
            2. Cliquer **Authorize** et coller le `access_token` reçu.\n\n\
            Le rôle `lecteur` permet les `GET` ; `admin` permet en plus `PATCH`, `DELETE` et la gestion des comptes.\n\n\
            Toutes les erreurs sont en `application/problem+json` (RFC 9457).",
        license(name = "Données : Licence Ouverte 2.0", url = "https://www.etalab.gouv.fr/licence-ouverte-open-licence/"),
    ),
    servers((url = "/", description = "Ce serveur")),
    paths(
        handlers::obtenir,
        handlers::rechercher,
        handlers::modifier,
        handlers::supprimer,
        handlers::connexion,
        handlers::creer_utilisateur,
        handlers::obtenir_utilisateur,
    ),
    components(schemas(
        Entreprise,
        Adresse,
        Links,
        Page,
        Meta,
        PageLinks,
        PatchEntreprise,
        PatchAdresse,
        Problem,
        FieldError,
        Connexion,
        handlers::TokenEmis,
        NouvelUtilisateur,
        Role,
        Utilisateur,
        Liens,
    )),
    modifiers(&SecuriteJwt),
    tags(
        // Dans cet ordre dans Swagger UI : on commence par se connecter.
        (name = "Authentification", description = "Obtenir un JWT : à faire en premier, puis cliquer Authorize"),
        (name = "Entreprises", description = "Consultation (lecteur) et modification (admin) des entreprises"),
        (name = "Utilisateurs", description = "Gestion des comptes (admin)"),
    ),
)]
pub struct ApiDoc;

/// Déclare le schéma Bearer JWT (non exprimable dans l'attribut `#[openapi]`).
struct SecuriteJwt;

impl Modify for SecuriteJwt {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            JWT,
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .description(Some("`access_token` renvoyé par `POST /api/v1/tokens`, valable 15 minutes."))
                    .build(),
            ),
        );
    }
}

/// Routes de la documentation : Swagger UI sur `/docs`, spécification sur `/api-docs/openapi.json`.
pub fn routes() -> Router<AppState> {
    SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()).into()
}
