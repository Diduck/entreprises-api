//! Configuration par variables d'environnement (ou fichier `.env`) et connexion MariaDB.

use std::time::Duration;

use anyhow::Context;
use sqlx::{MySqlPool, mysql::MySqlPoolOptions};

use crate::auth::Jwt;

/// Base du `compose.yml` (`docker compose up -d`).
pub const DATABASE_URL_DEFAUT: &str = "mysql://entreprises:entreprises-demo@127.0.0.1:3308/entreprises";
pub const BIND_ADDR_DEFAUT: &str = "127.0.0.1:3000";
/// Realm Keycloak du `compose.yml` : émetteur (`iss`) des tokens.
pub const OIDC_ISSUER_DEFAUT: &str = "http://127.0.0.1:8080/realms/entreprises";
/// Valeur attendue dans le champ `aud` des tokens (ajoutée par le client `swagger` du realm).
pub const OIDC_AUDIENCE_DEFAUT: &str = "entreprises-api";

#[derive(Debug)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,
    pub jwt: Jwt,
}

impl Config {
    /// Lit `DATABASE_URL`, `BIND_ADDR`, `OIDC_ISSUER`, `OIDC_AUDIENCE` et `OIDC_JWKS_URL`
    /// (par défaut : `<OIDC_ISSUER>/protocol/openid-connect/certs`, l'adresse Keycloak).
    pub fn depuis_env() -> anyhow::Result<Self> {
        let emetteur = variable("OIDC_ISSUER", OIDC_ISSUER_DEFAUT);
        let emetteur = emetteur.trim_end_matches('/');
        anyhow::ensure!(
            emetteur.starts_with("http://") || emetteur.starts_with("https://"),
            "OIDC_ISSUER invalide : « {emetteur} » (URL du realm attendue, ex. {OIDC_ISSUER_DEFAUT})"
        );
        let jwks_url = variable("OIDC_JWKS_URL", &format!("{emetteur}/protocol/openid-connect/certs"));
        let audience = variable("OIDC_AUDIENCE", OIDC_AUDIENCE_DEFAUT);
        Ok(Self {
            database_url: database_url(),
            bind_addr: variable("BIND_ADDR", BIND_ADDR_DEFAUT),
            jwt: Jwt::new(emetteur, &audience, &jwks_url),
        })
    }
}

fn variable(nom: &str, defaut: &str) -> String {
    std::env::var(nom).ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| defaut.to_owned())
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
