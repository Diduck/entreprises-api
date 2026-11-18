//! Documentation OpenAPI générée par `utoipa` depuis les handlers et les modèles, servie par Swagger UI.
//!
//! - `GET /docs` : Swagger UI ; le bouton « Authorize » ouvre la page de connexion Keycloak
//!   (OAuth2 Authorization Code + PKCE, client public `swagger`) ;
//! - `GET /api-docs/openapi.json` : la spécification brute.

use axum::Router;
use utoipa::{
    OpenApi,
    openapi::security::{AuthorizationCode, Flow, OAuth2, Scopes, SecurityScheme},
};
use utoipa_swagger_ui::{SwaggerUi, oauth};

use crate::{
    AppState,
    errors::{FieldError, Problem},
    handlers,
    models::{Adresse, Entreprise, Links, Meta, Page, PageLinks, PatchAdresse, PatchEntreprise},
};

/// Nom du schéma de sécurité référencé par `security(("keycloak" = []))` dans les handlers.
pub const KEYCLOAK: &str = "keycloak";
/// Client public du realm, déclaré dans `keycloak/realm-entreprises.json`.
pub const CLIENT_SWAGGER: &str = "swagger";

#[derive(OpenApi)]
#[openapi(
    info(
        title = "API REST Entreprises",
        description = "Données Sirene de l'INSEE (Licence Ouverte 2.0) : 99 999 entreprises.\n\n\
            **Connexion** : bouton **Authorize** → **Authorize** → page de connexion Keycloak \
            (ex. `lecteur@demo.fr` / `lecteur-demo-mdp`). La même connexion vaut pour l'API Python.\n\n\
            Keycloak signe les tokens avec sa clé privée (RS256) ; l'API les vérifie avec sa clé publique.\n\n\
            Le rôle `lecteur` permet les `GET` ; `admin` permet en plus `PATCH` et `DELETE`. \
            Les comptes se gèrent dans la console Keycloak.\n\n\
            Toutes les erreurs sont en `application/problem+json` (RFC 9457).",
        license(name = "Données : Licence Ouverte 2.0", url = "https://www.etalab.gouv.fr/licence-ouverte-open-licence/"),
    ),
    servers((url = "/", description = "Ce serveur")),
    paths(
        handlers::obtenir,
        handlers::rechercher,
        handlers::modifier,
        handlers::supprimer,
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
    )),
    tags(
        (name = "Entreprises", description = "Consultation (lecteur) et modification (admin) des entreprises"),
    ),
)]
pub struct ApiDoc;

/// Spécification complète : celle des handlers plus le schéma OAuth2 de Keycloak, dont les adresses dépendent
/// de l'émetteur configuré (`OIDC_ISSUER`).
pub fn specification(emetteur: &str) -> utoipa::openapi::OpenApi {
    let mut openapi = ApiDoc::openapi();
    let flux = AuthorizationCode::new(
        format!("{emetteur}/protocol/openid-connect/auth"),
        format!("{emetteur}/protocol/openid-connect/token"),
        Scopes::new(),
    );
    let mut schema = OAuth2::new([Flow::AuthorizationCode(flux)]);
    schema.description = Some(
        "Connexion Keycloak (client `swagger`, PKCE). Access token RS256 valable 15 minutes, \
        accepté aussi par l'API Python."
            .into(),
    );
    openapi
        .components
        .get_or_insert_with(Default::default)
        .add_security_scheme(KEYCLOAK, SecurityScheme::OAuth2(schema));
    openapi
}

/// Routes de la documentation : Swagger UI sur `/docs`, spécification sur `/api-docs/openapi.json`.
pub fn routes(emetteur: &str) -> Router<AppState> {
    SwaggerUi::new("/docs")
        .url("/api-docs/openapi.json", specification(emetteur))
        .oauth(oauth::Config::new().client_id(CLIENT_SWAGGER).use_pkce_with_authorization_code_grant(true))
        .into()
}
