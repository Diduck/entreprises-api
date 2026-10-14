//! Configuration par variables d'environnement (ou fichier `.env`) et connexion MariaDB.

use std::time::Duration;

use anyhow::Context;
use sqlx::{MySqlPool, mysql::MySqlPoolOptions};

use crate::auth::{Jwt, SECRET_MIN};

/// Base du `docker-compose.yml` (`docker compose up -d`).
pub const DATABASE_URL_DEFAUT: &str = "mysql://entreprises:entreprises-demo@127.0.0.1:3308/entreprises";
pub const BIND_ADDR_DEFAUT: &str = "127.0.0.1:3000";
/// Durée de validité d'un JWT, en minutes.
pub const JWT_DUREE_DEFAUT: u64 = 15;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,
    pub jwt: Jwt,
}

impl Config {
    /// Lit `DATABASE_URL`, `BIND_ADDR`, `JWT_SECRET` (obligatoire) et `JWT_DUREE_MINUTES`.
    pub fn depuis_env() -> anyhow::Result<Self> {
        let secret = std::env::var("JWT_SECRET").context("JWT_SECRET manquant : copier .env.example en .env")?;
        let minutes = match std::env::var("JWT_DUREE_MINUTES") {
            Ok(valeur) => {
                valeur.trim().parse::<u64>().ok().filter(|m| (1..=24 * 60).contains(m)).with_context(|| {
                    format!("JWT_DUREE_MINUTES invalide : « {valeur} » (entier de 1 à 1440 attendu)")
                })?
            }
            Err(_) => JWT_DUREE_DEFAUT,
        };
        let jwt = Jwt::new(&secret, Duration::from_secs(minutes * 60)).map_err(|e| {
            anyhow::anyhow!("JWT_SECRET trop court : {e}. En générer un avec `openssl rand -hex {SECRET_MIN}`.")
        })?;
        Ok(Self {
            database_url: database_url(),
            bind_addr: std::env::var("BIND_ADDR").unwrap_or_else(|_| BIND_ADDR_DEFAUT.to_owned()),
            jwt,
        })
    }
}

/// Charge `.env` s'il existe ; un fichier mal formé est une erreur (et non un silence).
pub fn charger_dotenv() -> anyhow::Result<()> {
    match dotenvy::dotenv() {
        Err(erreur) if !erreur.not_found() => Err(erreur).context("fichier .env mal formé"),
        _ => Ok(()),
    }
}

pub fn database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DATABASE_URL_DEFAUT.to_owned())
}

/// Pool MariaDB. La base doit exister : elle est créée par le conteneur (`docker compose up -d`).
pub async fn connecter(database_url: &str) -> anyhow::Result<MySqlPool> {
    MySqlPoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
        .with_context(|| {
            format!(
                "connexion impossible à {} : MariaDB est-elle lancée ? (`docker compose up -d`)",
                masquer_mot_de_passe(database_url)
            )
        })
}

/// URL affichable (messages, logs) : `mysql://user:***@hôte/base`.
pub fn masquer_mot_de_passe(url: &str) -> String {
    let Some((schema, reste)) = url.split_once("://") else { return url.to_owned() };
    match reste.rsplit_once('@') {
        Some((identifiants, hote)) => {
            let utilisateur = identifiants.split_once(':').map_or(identifiants, |(u, _)| u);
            format!("{schema}://{utilisateur}:***@{hote}")
        }
        None => url.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mot_de_passe_masque() {
        assert_eq!(masquer_mot_de_passe("mysql://moi:secret@db:3306/base"), "mysql://moi:***@db:3306/base");
        assert_eq!(masquer_mot_de_passe("mysql://db/base"), "mysql://db/base");
    }
}
