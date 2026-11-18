//! Authentification OAuth2 : les tokens sont émis par Keycloak, l'API ne fait que les vérifier.
//!
//! Keycloak signe chaque access token (JWT) avec sa **clé privée** (RS256). L'API ne connaît que la
//! **clé publique** correspondante, publiée par Keycloak au format JWKS (`OIDC_JWKS_URL`) : elle peut vérifier
//! une signature, jamais en fabriquer une. Les deux API (Rust et Python) font la même vérification, d'où une
//! seule connexion pour les deux.
//!
//! Les clés sont chargées au démarrage, puis rechargées :
//! - quand un token arrive avec un `kid` inconnu (nouvelle clé dans Keycloak), au plus une fois toutes les 10 s ;
//! - dès qu'elles ont plus de 5 minutes (`CLES_DUREE_MAX`). Une clé retirée de Keycloak (par exemple parce que
//!   la clé privée a fuité) n'est donc plus acceptée au bout de 5 minutes au plus.
//!
//! Si Keycloak est injoignable alors que les clés ont plus de 5 minutes, l'API refuse les tokens (503) plutôt
//! que de continuer à faire confiance à des clés peut-être retirées.
//!
//! Les extracteurs `Lecteur` et `Admin` sont placés en premier argument des handlers (et non un middleware) :
//! le routage répond d'abord 404/405, puis l'authentification 401/403, avant toute validation 400/422.
//!
//! Le token contient l'identifiant Keycloak de l'utilisateur (`sub`) et ses rôles (`realm_access.roles`) :
//! l'API le vérifie sans appeler Keycloak. Conséquence : un compte désactivé ou dont le rôle change garde son
//! accès jusqu'à l'expiration du token (15 min, réglée dans le realm).

use std::{
    collections::HashMap,
    sync::RwLock,
    time::{Duration, Instant},
};

use tokio::sync::Mutex;

use axum::{extract::FromRequestParts, http::request::Parts};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, jwk::Jwk};
use serde::Deserialize;

use crate::{AppState, errors::ApiError};

/// Délai minimal entre deux tentatives de rechargement des clés publiques.
const RECHARGEMENT_MIN: Duration = Duration::from_secs(10);
/// Au-delà, les clés publiques ne sont plus crues sans avoir été rechargées chez Keycloak.
pub const CLES_DUREE_MAX: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Lecteur,
    Admin,
}

/// Contenu (claims) utile d'un access token Keycloak.
#[derive(Debug, Deserialize)]
struct Claims {
    /// Identifiant (UUID) de l'utilisateur dans Keycloak.
    sub: String,
    #[serde(default)]
    realm_access: RealmAccess,
}

#[derive(Debug, Default, Deserialize)]
struct RealmAccess {
    #[serde(default)]
    roles: Vec<String>,
}

/// Utilisateur authentifié, lu dans un token valide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identite {
    /// Identifiant Keycloak (`sub`).
    pub id: String,
    /// `None` : token valide, mais sans le rôle `lecteur` ni `admin` du realm.
    pub role: Option<Role>,
}

/// Pourquoi un token est refusé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refus {
    /// Mal formé, falsifié, expiré, autre émetteur ou audience, clé inconnue : 401.
    TokenInvalide,
    /// Clés publiques trop anciennes et Keycloak injoignable : impossible de vérifier, 503.
    ClesIndisponibles,
}

/// Clés publiques connues, par identifiant (`kid`).
struct Cles {
    par_kid: HashMap<String, DecodingKey>,
    /// Dernier téléchargement réussi du JWKS. `None` : jamais chargées.
    chargees_le: Option<Instant>,
    /// Dernière tentative de téléchargement, réussie ou non.
    essai_le: Option<Instant>,
}

/// Vérification des access tokens : émetteur, audience, algorithme et clés publiques attendus.
pub struct Jwt {
    validation: Validation,
    emetteur: String,
    /// Adresse des clés publiques (JWKS). `None` : clés fixes (tests).
    jwks_url: Option<String>,
    http: reqwest::Client,
    cles: RwLock<Cles>,
    /// Un seul rechargement à la fois : les requêtes concurrentes attendent son résultat.
    rechargement: Mutex<()>,
    duree_max: Duration,
    /// Délai entre deux tentatives : `RECHARGEMENT_MIN`, jamais plus long que `duree_max`.
    anti_rafale: Duration,
}

/// Affiche la configuration, jamais les clés.
impl std::fmt::Debug for Jwt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jwt")
            .field("emetteur", &self.emetteur)
            .field("jwks_url", &self.jwks_url)
            .finish_non_exhaustive()
    }
}

impl Jwt {
    /// Tokens émis par `emetteur` (`iss`), destinés à `audience` (`aud`), signés par une clé de `jwks_url`.
    pub fn new(emetteur: &str, audience: &str, jwks_url: &str) -> Self {
        Self::construire(emetteur, audience, Some(jwks_url.to_owned()), HashMap::new())
    }

    /// Clé publique fixe, sans JWKS à télécharger : pour les tests.
    pub fn avec_cle_publique(emetteur: &str, audience: &str, kid: &str, cle: DecodingKey) -> Self {
        Self::construire(emetteur, audience, None, HashMap::from([(kid.to_owned(), cle)]))
    }

    fn construire(
        emetteur: &str,
        audience: &str,
        jwks_url: Option<String>,
        par_kid: HashMap<String, DecodingKey>,
    ) -> Self {
        // RS256 uniquement : ni `none`, ni HS256 (qui permettrait de signer avec la clé publique, connue de tous).
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[emetteur]);
        validation.set_audience(&[audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        // Pas de tolérance : un token expiré est refusé à la seconde près.
        validation.leeway = 0;
        Self {
            validation,
            emetteur: emetteur.to_owned(),
            jwks_url,
            http: reqwest::Client::builder().timeout(Duration::from_secs(5)).build().expect("client HTTP"),
            cles: RwLock::new(Cles { par_kid, chargees_le: None, essai_le: None }),
            rechargement: Mutex::new(()),
            duree_max: CLES_DUREE_MAX,
            anti_rafale: RECHARGEMENT_MIN,
        }
    }

    /// Durée de confiance des clés publiques (5 min par défaut) : plus courte dans les tests.
    pub fn avec_duree_max(mut self, duree: Duration) -> Self {
        self.duree_max = duree;
        self.anti_rafale = RECHARGEMENT_MIN.min(duree);
        self
    }

    pub fn emetteur(&self) -> &str {
        &self.emetteur
    }

    /// Télécharge les clés publiques de signature. Renvoie leur nombre.
    pub async fn charger_cles(&self) -> Result<usize, String> {
        let Some(url) = &self.jwks_url else { return Ok(self.cles.read().expect("verrou des clés").par_kid.len()) };
        self.cles.write().expect("verrou des clés").essai_le = Some(Instant::now());
        let reponse = self.http.get(url).send().await.map_err(|e| format!("{url} injoignable : {e}"))?;
        if !reponse.status().is_success() {
            return Err(format!("{url} a répondu {}", reponse.status()));
        }
        let corps = reponse.bytes().await.map_err(|e| format!("lecture de {url} : {e}"))?;
        let par_kid = cles_de_signature(&corps).map_err(|e| format!("JWKS illisible ({url}) : {e}"))?;
        let nombre = par_kid.len();
        let maintenant = Some(Instant::now());
        // Remplace tout le jeu : une clé absente du JWKS (retirée dans Keycloak) disparaît ici.
        *self.cles.write().expect("verrou des clés") = Cles { par_kid, chargees_le: maintenant, essai_le: maintenant };
        Ok(nombre)
    }

    /// État des clés pour `kid` : la clé si elle est connue, et si le jeu de clés est encore assez récent.
    fn lire(&self, kid: &str) -> (Option<DecodingKey>, bool, Option<Instant>) {
        let cles = self.cles.read().expect("verrou des clés");
        // Clés fixes (tests sans JWKS) : toujours à jour.
        let a_jour = self.jwks_url.is_none() || cles.chargees_le.is_some_and(|t| t.elapsed() < self.duree_max);
        (cles.par_kid.get(kid).cloned(), a_jour, cles.essai_le)
    }

    /// Clé publique `kid`. Recharge le JWKS si les clés ont plus de `duree_max`, ou si `kid` est inconnu
    /// (au plus une tentative toutes les 10 s).
    async fn cle(&self, kid: &str) -> Result<DecodingKey, Refus> {
        if let (Some(cle), true, _) = self.lire(kid) {
            return Ok(cle);
        }
        if self.jwks_url.is_none() {
            return Err(Refus::TokenInvalide);
        }
        // Une seule requête recharge ; les autres attendent puis relisent le résultat.
        let _garde = self.rechargement.lock().await;
        let (cle, a_jour, essai_le) = self.lire(kid);
        match (cle, a_jour) {
            (Some(cle), true) => return Ok(cle),
            _ if essai_le.is_some_and(|t| t.elapsed() < self.anti_rafale) => {
                // Tentative trop récente : on ne martèle pas Keycloak.
                return Err(if a_jour { Refus::TokenInvalide } else { Refus::ClesIndisponibles });
            }
            _ => {}
        }
        if let Err(erreur) = self.charger_cles().await {
            tracing::warn!("clés publiques de Keycloak non rechargées : {erreur}");
        }
        match self.lire(kid) {
            (Some(cle), true, _) => Ok(cle),
            (_, true, _) => Err(Refus::TokenInvalide),
            (_, false, _) => Err(Refus::ClesIndisponibles),
        }
    }

    /// Vérifie signature (RS256, clé publique de Keycloak), émetteur, audience et expiration.
    pub async fn verifier(&self, token: &str) -> Result<Identite, Refus> {
        let entete = jsonwebtoken::decode_header(token).map_err(|_| Refus::TokenInvalide)?;
        if entete.alg != Algorithm::RS256 {
            return Err(Refus::TokenInvalide);
        }
        let cle = self.cle(&entete.kid.ok_or(Refus::TokenInvalide)?).await?;
        let claims =
            jsonwebtoken::decode::<Claims>(token, &cle, &self.validation).map_err(|_| Refus::TokenInvalide)?.claims;
        let roles = &claims.realm_access.roles;
        let role = if roles.iter().any(|r| r == "admin") {
            Some(Role::Admin)
        } else if roles.iter().any(|r| r == "lecteur") {
            Some(Role::Lecteur)
        } else {
            None
        };
        Ok(Identite { id: claims.sub, role })
    }
}

/// Clés de signature RSA d'un document JWKS. Les autres (chiffrement `use: enc`, autres algorithmes) sont ignorées.
fn cles_de_signature(jwks: &[u8]) -> Result<HashMap<String, DecodingKey>, String> {
    #[derive(Deserialize)]
    struct Jwks {
        keys: Vec<serde_json::Value>,
    }
    let jwks: Jwks = serde_json::from_slice(jwks).map_err(|e| e.to_string())?;
    let mut par_kid = HashMap::new();
    for brute in jwks.keys {
        if brute.get("use").and_then(|u| u.as_str()).is_some_and(|u| u != "sig") {
            continue;
        }
        let Ok(jwk) = serde_json::from_value::<Jwk>(brute) else { continue };
        let (Some(kid), Ok(cle)) = (jwk.common.key_id.clone(), DecodingKey::from_jwk(&jwk)) else { continue };
        par_kid.insert(kid, cle);
    }
    if par_kid.is_empty() {
        return Err("aucune clé de signature".into());
    }
    Ok(par_kid)
}

async fn identite_de_la_requete(parts: &mut Parts, state: &AppState) -> Result<Identite, ApiError> {
    let token_fourni = parts.headers.contains_key(axum::http::header::AUTHORIZATION);
    let TypedHeader(Authorization(bearer)) = TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
        .await
        .map_err(|_| ApiError::NonAuthentifie { token_fourni })?;
    state.jwt.verifier(bearer.token()).await.map_err(|refus| match refus {
        Refus::TokenInvalide => ApiError::NonAuthentifie { token_fourni },
        Refus::ClesIndisponibles => ApiError::FournisseurIndisponible,
    })
}

/// Accès en lecture : rôle lecteur ou admin, sinon 403.
#[derive(Debug)]
pub struct Lecteur(pub Identite);

impl FromRequestParts<AppState> for Lecteur {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let identite = identite_de_la_requete(parts, state).await?;
        match identite.role {
            Some(_) => Ok(Self(identite)),
            None => Err(ApiError::SansRole),
        }
    }
}

/// Accès en écriture : rôle admin uniquement, sinon 403.
#[derive(Debug)]
pub struct Admin(pub Identite);

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let identite = identite_de_la_requete(parts, state).await?;
        match identite.role {
            Some(Role::Admin) => Ok(Self(identite)),
            Some(Role::Lecteur) => Err(ApiError::AccesRefuse),
            None => Err(ApiError::SansRole),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwks_keycloak_garde_les_seules_cles_de_signature() {
        // Forme d'un JWKS Keycloak : une clé de signature RS256 et une clé de chiffrement RSA-OAEP.
        // Module de la clé publique de tests/fixtures/cle-test-publique.pem.
        let n = "jbLek2HzuhUK-gmxQTL4DvGyGB-gIdKvVCAdcqTR6KrFfLD5aufEqv611WQz_EW8fpxMgd1kYKHH0sSRz7tZzx50og4ag78gwFKvGsNCC01lK6ZE-QZgMFMgnBFVMCfMwvkIRhcWwtDxzDmY-rfB-n7Q_7NxzOnwxThJcR5q4SbO5-YXPRKhepGPH1CTtyAdvxepP0LcOYEH-mli3EvM1Dr72fSgQMbHSHJ-1gVJYNbGO6RsuSpWUQ_G_H9mRvaetUZYS81iXncUyiTNnZCS2nnUIlP6qi0CEsybnndeAB28SpB2OLTxKuwAgkhcsBicIV-mAT9xRB0pjIF-6fINGw";
        let jwks = format!(
            r#"{{"keys":[
                {{"kid":"sig-1","kty":"RSA","alg":"RS256","use":"sig","n":"{n}","e":"AQAB"}},
                {{"kid":"enc-1","kty":"RSA","alg":"RSA-OAEP","use":"enc","n":"{n}","e":"AQAB"}}
            ]}}"#
        );
        let cles = cles_de_signature(jwks.as_bytes()).unwrap();
        assert_eq!(cles.keys().collect::<Vec<_>>(), ["sig-1"]);
    }

    #[test]
    fn jwks_sans_cle_de_signature_refuse() {
        assert!(cles_de_signature(br#"{"keys":[]}"#).is_err());
        assert!(cles_de_signature(b"pas du json").is_err());
    }
}
