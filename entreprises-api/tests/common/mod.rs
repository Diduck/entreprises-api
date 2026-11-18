//! Un faux Keycloak pour les tests : une paire de clés RSA propre aux tests (`tests/fixtures/cle-test-*.pem`).
//! La clé privée signe les tokens comme le ferait Keycloak ; l'API ne reçoit que la clé publique.
//! Ces clés ne servent qu'aux tests : elles ne doivent jamais être utilisées ailleurs.

#![allow(dead_code)]

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, get_current_timestamp};
use serde_json::{Value, json};

use entreprises_api::auth::Jwt;

pub const EMETTEUR: &str = "http://keycloak.test/realms/entreprises";
pub const AUDIENCE: &str = "entreprises-api";
pub const KID: &str = "cle-de-test";

pub const CLE_PRIVEE: &[u8] = include_bytes!("../fixtures/cle-test-privee.pem");
pub const CLE_PUBLIQUE: &[u8] = include_bytes!("../fixtures/cle-test-publique.pem");
/// Une autre clé privée, inconnue de l'API.
pub const CLE_PIRATE: &[u8] = include_bytes!("../fixtures/cle-pirate-privee.pem");

/// Vérificateur de l'API, configuré avec la clé publique de test.
pub fn jwt() -> Jwt {
    Jwt::avec_cle_publique(EMETTEUR, AUDIENCE, KID, DecodingKey::from_rsa_pem(CLE_PUBLIQUE).expect("clé publique"))
}

/// Contenu d'un access token Keycloak valable 15 min, avec ces rôles de realm.
pub fn claims(roles: &[&str]) -> Value {
    let maintenant = get_current_timestamp();
    json!({
        "iss": EMETTEUR,
        "aud": [AUDIENCE, "account"],
        "sub": "8a8541a0-0695-4c00-a204-159bdfa475af",
        "azp": "swagger",
        "iat": maintenant,
        "exp": maintenant + 900,
        "realm_access": { "roles": roles },
    })
}

/// Signe `claims` en RS256 avec `cle_privee`, sous l'identifiant de clé `kid`.
pub fn signer(claims: &Value, cle_privee: &[u8], kid: &str) -> String {
    let mut entete = Header::new(Algorithm::RS256);
    entete.kid = Some(kid.to_owned());
    jsonwebtoken::encode(&entete, claims, &EncodingKey::from_rsa_pem(cle_privee).expect("clé privée"))
        .expect("signature RS256")
}

/// Token valide, signé par le « Keycloak » de test.
pub fn token(roles: &[&str]) -> String {
    signer(&claims(roles), CLE_PRIVEE, KID)
}

pub fn admin() -> String {
    token(&["default-roles-entreprises", "admin", "lecteur"])
}

pub fn lecteur() -> String {
    token(&["default-roles-entreprises", "lecteur"])
}
