//! Déclaration des routes (syntaxe Axum 0.8 : `/{siren}`, jamais `/:siren`).

use axum::{
    Router,
    routing::{get, post},
};

use crate::{AppState, handlers};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/entreprises", get(handlers::rechercher))
        .route(
            "/api/v1/entreprises/{siren}",
            get(handlers::obtenir).patch(handlers::modifier).delete(handlers::supprimer),
        )
        .route("/api/v1/tokens", post(handlers::connexion))
        .route("/api/v1/utilisateurs", post(handlers::creer_utilisateur))
        .route("/api/v1/utilisateurs/{id}", get(handlers::obtenir_utilisateur))
}
