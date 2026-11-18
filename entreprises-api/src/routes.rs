//! Déclaration des routes (syntaxe Axum 0.8 : `/{siren}`, jamais `/:siren`).

use axum::{Router, routing::get};

use crate::{AppState, handlers};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/entreprises", get(handlers::rechercher)).route(
        "/api/v1/entreprises/{siren}",
        get(handlers::obtenir).patch(handlers::modifier).delete(handlers::supprimer),
    )
}
