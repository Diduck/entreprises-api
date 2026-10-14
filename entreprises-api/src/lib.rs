//! API REST Entreprises : données Sirene (INSEE) servies par Axum, SQLx et MariaDB.

pub mod auth;
pub mod config;
pub mod errors;
pub mod handlers;
pub mod import;
pub mod models;
pub mod normalize;
pub mod openapi;
pub mod repository;
pub mod routes;
pub mod utilisateurs;

use std::{sync::Arc, time::Duration};

use axum::{Router, extract::Request, http::StatusCode, middleware};
use sqlx::{MySqlPool, migrate::Migrator};
use tower_http::{
    timeout::TimeoutLayer,
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::Level;

use crate::auth::Jwt;

/// Migrations du dossier `migrations/`, embarquées à la compilation.
pub static MIGRATOR: Migrator = sqlx::migrate!();

/// Au-delà, une requête est coupée avec un 408.
pub const DELAI_MAX: Duration = Duration::from_secs(10);

/// État partagé par les handlers.
#[derive(Clone)]
pub struct AppState {
    pub pool: MySqlPool,
    /// Signature et vérification des JWT.
    pub jwt: Arc<Jwt>,
}

impl AppState {
    pub fn new(pool: MySqlPool, jwt: Jwt) -> Self {
        Self { pool, jwt: Arc::new(jwt) }
    }
}

/// Application complète, réutilisée par `main` et par les tests.
pub fn build_app(state: AppState) -> Router {
    routes::routes()
        .merge(openapi::routes())
        .fallback(errors::route_inconnue)
        .method_not_allowed_fallback(errors::methode_non_autorisee)
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, DELAI_MAX))
        .layer(middleware::from_fn(errors::problem_details))
        .layer(
            // Méthode, chemin (sans la query string, qui peut contenir un nom), statut et durée.
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request| {
                    tracing::info_span!("requete", methode = %request.method(), chemin = %request.uri().path())
                })
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}
