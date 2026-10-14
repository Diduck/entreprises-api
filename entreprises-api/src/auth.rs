//! Authentification par JWT : émission à la connexion (`POST /api/v1/tokens`), vérification à chaque requête.
//!
//! Les extracteurs `Lecteur` et `Admin` sont placés en premier argument des handlers (et non un middleware) :
//! le routage répond d'abord 404/405, puis l'authentification 401/403, avant toute validation 400/422.
//!
//! Le JWT est signé en HS256 avec `JWT_SECRET`. Il contient l'identifiant de l'utilisateur (`sub`), son rôle et
//! sa date d'expiration : l'API le vérifie sans lire la base. Conséquence : un compte supprimé ou dont le rôle
//! change garde son accès jusqu'à l'expiration du token (15 min par défaut).

use std::time::Duration;

use axum::{extract::FromRequestParts, http::request::Parts};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, get_current_timestamp};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{AppState, errors::ApiError};

/// Émetteur inscrit dans chaque token (`iss`) et exigé à la vérification.
const EMETTEUR: &str = "entreprises-api";
/// Longueur minimale de `JWT_SECRET` : 256 bits, la taille de la signature HS256.
pub const SECRET_MIN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Lecteur,
    Admin,
}

impl Role {
    pub fn parse(texte: &str) -> Option<Self> {
        match texte {
            "admin" => Some(Self::Admin),
            "lecteur" => Some(Self::Lecteur),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Lecteur => "lecteur",
        }
    }
}

/// Contenu (claims) du JWT. Lisible par quiconque détient le token : aucune donnée secrète.
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    /// Identifiant de l'utilisateur.
    sub: String,
    role: Role,
    iss: String,
    /// Émis le (secondes depuis 1970).
    iat: u64,
    /// Expire le (secondes depuis 1970).
    exp: u64,
}

/// Utilisateur authentifié, lu dans un JWT valide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identite {
    pub id: i64,
    pub role: Role,
}

/// Clés et règles de signature des JWT.
#[derive(Clone)]
pub struct Jwt {
    encodage: EncodingKey,
    decodage: DecodingKey,
    validation: Validation,
    duree: Duration,
}

/// Affiche la durée, jamais les clés.
impl std::fmt::Debug for Jwt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jwt").field("duree", &self.duree).finish_non_exhaustive()
    }
}

impl Jwt {
    /// `secret` : au moins 32 octets ; `duree` : durée de validité des tokens émis.
    pub fn new(secret: &str, duree: Duration) -> Result<Self, String> {
        if secret.len() < SECRET_MIN {
            return Err(format!("{} caractères au moins attendus, {} reçus", SECRET_MIN, secret.len()));
        }
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[EMETTEUR]);
        validation.set_required_spec_claims(&["exp", "iss", "sub"]);
        // Pas de tolérance : un token expiré est refusé à la seconde près.
        validation.leeway = 0;
        Ok(Self {
            encodage: EncodingKey::from_secret(secret.as_bytes()),
            decodage: DecodingKey::from_secret(secret.as_bytes()),
            validation,
            duree,
        })
    }

    pub fn duree(&self) -> Duration {
        self.duree
    }

    /// Signe un token pour cet utilisateur, valable `duree` à partir de maintenant.
    pub fn emettre(&self, identite: Identite) -> String {
        let maintenant = get_current_timestamp();
        let claims = Claims {
            sub: identite.id.to_string(),
            role: identite.role,
            iss: EMETTEUR.to_owned(),
            iat: maintenant,
            exp: maintenant + self.duree.as_secs(),
        };
        jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &self.encodage)
            .expect("signature HS256 d'un JSON sérialisable")
    }

    /// Vérifie signature, algorithme (HS256 uniquement), émetteur et expiration.
    pub fn verifier(&self, token: &str) -> Option<Identite> {
        let donnees = jsonwebtoken::decode::<Claims>(token, &self.decodage, &self.validation).ok()?;
        let id = donnees.claims.sub.parse().ok()?;
        Some(Identite { id, role: donnees.claims.role })
    }
}

async fn identite_de_la_requete(parts: &mut Parts, state: &AppState) -> Result<Identite, ApiError> {
    let token_fourni = parts.headers.contains_key(axum::http::header::AUTHORIZATION);
    let TypedHeader(Authorization(bearer)) = TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
        .await
        .map_err(|_| ApiError::NonAuthentifie { token_fourni })?;
    state.jwt.verifier(bearer.token()).ok_or(ApiError::NonAuthentifie { token_fourni })
}

/// Accès en lecture : rôle lecteur ou admin.
#[derive(Debug)]
pub struct Lecteur(pub Identite);

impl FromRequestParts<AppState> for Lecteur {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        identite_de_la_requete(parts, state).await.map(Self)
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
            Role::Admin => Ok(Self(identite)),
            Role::Lecteur => Err(ApiError::AccesRefuse),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "un-secret-de-test-assez-long-pour-hs256";

    fn jwt() -> Jwt {
        Jwt::new(SECRET, Duration::from_secs(900)).unwrap()
    }

    #[test]
    fn un_token_emis_est_accepte() {
        let jwt = jwt();
        let identite = Identite { id: 7, role: Role::Admin };
        assert_eq!(jwt.verifier(&jwt.emettre(identite)), Some(identite));
    }

    #[test]
    fn secret_trop_court_refuse() {
        assert!(Jwt::new("court", Duration::from_secs(900)).is_err());
    }

    #[test]
    fn token_expire_refuse() {
        let jwt = Jwt::new(SECRET, Duration::ZERO).unwrap();
        let token = jwt.emettre(Identite { id: 1, role: Role::Lecteur });
        std::thread::sleep(Duration::from_millis(1100));
        assert_eq!(jwt.verifier(&token), None);
    }

    #[test]
    fn token_signe_avec_un_autre_secret_refuse() {
        let autre = Jwt::new("un-autre-secret-de-test-tout-aussi-long", Duration::from_secs(900)).unwrap();
        let token = autre.emettre(Identite { id: 1, role: Role::Admin });
        assert_eq!(jwt().verifier(&token), None);
    }

    #[test]
    fn contenu_modifie_ou_signature_absente_refuse() {
        let jwt = jwt();
        let lecteur = jwt.emettre(Identite { id: 1, role: Role::Lecteur });
        let admin = jwt.emettre(Identite { id: 1, role: Role::Admin });
        let parties_lecteur: Vec<&str> = lecteur.split('.').collect();
        let contenu_admin = admin.split('.').nth(1).unwrap();
        // Contenu « admin » avec la signature du token lecteur : la signature ne correspond plus.
        let falsifie = format!("{}.{contenu_admin}.{}", parties_lecteur[0], parties_lecteur[2]);
        assert_eq!(jwt.verifier(&falsifie), None);
        assert_eq!(jwt.verifier(&format!("{}.{}.", parties_lecteur[0], parties_lecteur[1])), None);
        assert_eq!(jwt.verifier("pas-un-jwt"), None);
    }

    #[test]
    fn algorithme_none_refuse() {
        // En-tête {"alg":"none","typ":"JWT"} : un token sans signature ne doit jamais passer.
        let entete = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0";
        let jwt = jwt();
        let token = jwt.emettre(Identite { id: 1, role: Role::Admin });
        let contenu = token.split('.').nth(1).unwrap();
        assert_eq!(jwt.verifier(&format!("{entete}.{contenu}.")), None);
    }
}
